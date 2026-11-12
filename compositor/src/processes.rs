//! The desktop's processes (ADR-0045): everything the compositor starts, with
//! its state and the last of its output, so Settings can show them, restart
//! them and read their logs. The wallpaper and the bar are *kept*: when one
//! exits it's started again, unless it was stopped on purpose or keeps
//! dying. Each child's output is read here and copied to the compositor's
//! stderr, prefixed with its name.
//!
//! The list is global, not part of `State`: children are started from
//! threads too (the volume flash), and reaped and read on threads of their
//! own. `dbus.rs` serves it on the session bus.

pub mod dbus;

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// Lines of output kept per process.
const LOG_LINES: usize = 500;
/// Processes that ran once and exited, kept in the list for their logs.
const KEEP_EXITED: usize = 20;
/// A kept piece starts again this long after it exits...
const RESTART_AFTER: Duration = Duration::from_secs(1);
/// ...unless it has exited this many times within `GIVE_UP_WITHIN`.
const GIVE_UP_AFTER: usize = 5;
const GIVE_UP_WITHIN: Duration = Duration::from_secs(30);

static PROCESSES: Mutex<Processes> = Mutex::new(Processes {
    list: Vec::new(),
    next: 1,
});

fn processes() -> MutexGuard<'static, Processes> {
    PROCESSES.lock().unwrap_or_else(|e| e.into_inner())
}

struct Processes {
    list: Vec<Process>,
    next: u32,
}

impl Processes {
    fn get(&mut self, id: u32) -> Option<&mut Process> {
        self.list.iter_mut().find(|p| p.id == id)
    }

    /// Forgets the oldest of the processes that ran once and are done.
    fn prune(&mut self) {
        let done = |p: &Process| !p.kept && p.pid.is_none();
        let mut extra = self
            .list
            .iter()
            .filter(|p| done(p))
            .count()
            .saturating_sub(KEEP_EXITED);
        self.list.retain(|p| {
            let drop = extra > 0 && done(p);
            extra -= drop as usize;
            !drop
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Running,
    /// How it ended: "exited with 1", "killed by signal 9".
    Exited(String),
    /// Stopped from Settings; kept pieces stay down.
    Stopped,
    /// A kept piece that kept dying, and how it last ended.
    GaveUp(String),
}

impl Status {
    pub fn describe(&self) -> String {
        match self {
            Status::Running => "running".into(),
            Status::Exited(how) => how.clone(),
            Status::Stopped => "stopped".into(),
            Status::GaveUp(how) => format!("gave up: {how}"),
        }
    }
}

struct Process {
    id: u32,
    name: String,
    command: String,
    /// The compositor's socket, to start it again with.
    socket: OsString,
    /// Variables of its own, from whoever asked for it (the bar's skin).
    env: Vec<(String, String)>,
    kept: bool,
    /// Also its process group's ID.
    pid: Option<u32>,
    status: Status,
    restarts: u32,
    started: SystemTime,
    /// When a kept piece last exited, to give up on one that keeps dying.
    exits: VecDeque<Instant>,
    log: VecDeque<String>,
    /// The sequence number of `log`'s first line.
    first: u64,
    /// Counts runs, so a reaper of an earlier one leaves a later alone.
    run: u32,
    /// What to do when the running one exits: stay down, or start again.
    stopping: bool,
    restarting: bool,
}

impl Process {
    fn push(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
            self.first += 1;
        }
        self.log.push_back(line);
    }
}

/// What the bus is told about a process.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: u32,
    pub name: String,
    pub command: String,
    /// 0 when it isn't running.
    pub pid: u32,
    pub status: String,
    pub kept: bool,
    pub restarts: u32,
    /// When it last started, in seconds since the epoch.
    pub started: u64,
}

/// The name a command line is shown by: its program's file name.
fn name_of(command: &str) -> String {
    let program = command.split_whitespace().next().unwrap_or(command);
    program.rsplit('/').next().unwrap_or(program).to_string()
}

/// Variables a caller can't set: the compositor's own say which desktop
/// this is.
const RESERVED: [&str; 4] = ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DISPLAY", "PATH"];

/// Starts `command_line` as a client of the compositor at `socket`; `kept`
/// starts it again whenever it exits.
pub fn spawn(socket: &OsStr, command_line: &str, kept: bool) -> u32 {
    spawn_with(socket, command_line, kept, Vec::new())
}

/// [`spawn`], with variables of its own besides the compositor's.
pub fn spawn_with(
    socket: &OsStr,
    command_line: &str,
    kept: bool,
    env: Vec<(String, String)>,
) -> u32 {
    let env = env
        .into_iter()
        .filter(|(k, _)| !RESERVED.contains(&k.as_str()) && !k.starts_with("XDG_"))
        .collect();
    let mut processes = processes();
    let id = processes.next;
    processes.next += 1;
    processes.list.push(Process {
        id,
        name: name_of(command_line),
        command: command_line.to_string(),
        socket: socket.to_os_string(),
        env,
        kept,
        pid: None,
        status: Status::Exited("not started".into()),
        restarts: 0,
        started: SystemTime::now(),
        exits: VecDeque::new(),
        log: VecDeque::new(),
        first: 0,
        run: 0,
        stopping: false,
        restarting: false,
    });
    start(&mut processes, id);
    processes.prune();
    id
}

/// Runs process `id`, with threads reading its output and reaping it.
fn start(processes: &mut Processes, id: u32) {
    let Some(p) = processes.get(id) else { return };
    let mut command = crate::clients::command(&p.socket, &p.command);
    command.envs(p.env.iter().map(|(k, v)| (k, v)));
    // A group of its own, so stopping it reaches the program behind sh -c.
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("patina-compositor: can't run {}: {e}", p.command);
            p.pid = None;
            p.status = Status::Exited(format!("couldn't start: {e}"));
            return;
        }
    };
    if p.run > 0 {
        p.restarts += 1;
        p.push(format!("── started again ({}) ──", p.command));
    }
    p.run += 1;
    p.pid = Some(child.id());
    p.status = Status::Running;
    p.started = SystemTime::now();
    let (run, name) = (p.run, p.name.clone());
    if let Some(out) = child.stdout.take() {
        read(id, name.clone(), out);
    }
    if let Some(err) = child.stderr.take() {
        read(id, name.clone(), err);
    }
    let reaper = std::thread::Builder::new()
        .name(format!("reap {name}"))
        .spawn(move || reap(id, run, child));
    if let Err(e) = reaper {
        eprintln!("patina-compositor: can't watch {name}: {e}");
    }
}

/// Copies `pipe`'s lines into process `id`'s log and the compositor's
/// stderr, until everything holding it open has closed it.
fn read(id: u32, name: String, pipe: impl Read + Send + 'static) {
    let thread = std::thread::Builder::new()
        .name(format!("read {name}"))
        .spawn(move || {
            let mut pipe = BufReader::new(pipe);
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                match pipe.read_until(b'\n', &mut bytes) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                let line = String::from_utf8_lossy(&bytes);
                let line = line.trim_end_matches(['\n', '\r']);
                eprintln!("{name}: {line}");
                if let Some(p) = processes().get(id) {
                    p.push(line.to_string());
                }
            }
        });
    if let Err(e) = thread {
        eprintln!("patina-compositor: can't read a process's output: {e}");
    }
}

/// How a process ended, in words.
fn ended(status: std::io::Result<ExitStatus>) -> String {
    match status {
        Ok(s) => match (s.code(), s.signal()) {
            (Some(code), _) => format!("exited with {code}"),
            (None, Some(signal)) => format!("killed by signal {signal}"),
            _ => s.to_string(),
        },
        Err(e) => format!("lost: {e}"),
    }
}

/// Waits for run `run` of process `id` to end, then decides what's next.
fn reap(id: u32, run: u32, mut child: Child) {
    let how = ended(child.wait());
    let again = {
        let mut processes = processes();
        let Some(p) = processes.get(id) else { return };
        if p.run != run {
            return;
        }
        p.pid = None;
        p.push(format!("── {how} ──"));
        if std::mem::take(&mut p.stopping) {
            p.restarting = false;
            p.status = Status::Stopped;
            None
        } else if std::mem::take(&mut p.restarting) {
            Some(Duration::ZERO)
        } else if p.kept {
            let now = Instant::now();
            p.exits.push_back(now);
            while p
                .exits
                .front()
                .is_some_and(|t| now.duration_since(*t) > GIVE_UP_WITHIN)
            {
                p.exits.pop_front();
            }
            if p.exits.len() >= GIVE_UP_AFTER {
                eprintln!(
                    "patina-compositor: {} keeps exiting ({how}); not starting it again",
                    p.name
                );
                p.status = Status::GaveUp(how);
                None
            } else {
                eprintln!("patina-compositor: {} {how}; starting it again", p.name);
                p.status = Status::Exited(how);
                Some(RESTART_AFTER)
            }
        } else {
            p.status = Status::Exited(how);
            None
        }
    };
    let Some(after) = again else {
        processes().prune();
        return;
    };
    std::thread::sleep(after);
    let mut processes = processes();
    // Unless it was started or stopped from Settings meanwhile.
    if processes
        .get(id)
        .is_some_and(|p| p.run == run && matches!(p.status, Status::Exited(_) | Status::Running))
    {
        start(&mut processes, id);
    }
}

/// Sends SIGTERM to the process group led by `pid`.
fn terminate(pid: u32) -> Result<(), String> {
    let group = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| format!("no such process group: {pid}"))?;
    rustix::process::kill_process_group(group, rustix::process::Signal::TERM)
        .map_err(|e| e.to_string())
}

/// All the processes, oldest first.
pub fn list() -> Vec<Entry> {
    processes()
        .list
        .iter()
        .map(|p| Entry {
            id: p.id,
            name: p.name.clone(),
            command: p.command.clone(),
            pid: p.pid.unwrap_or(0),
            status: p.status.describe(),
            kept: p.kept,
            restarts: p.restarts,
            started: p
                .started
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        })
        .collect()
}

/// Stops process `id` if it's running and starts it again, or starts it if
/// it isn't.
pub fn restart(id: u32) -> Result<(), String> {
    let mut processes = processes();
    let p = processes
        .get(id)
        .ok_or_else(|| format!("no process {id}"))?;
    p.exits.clear();
    p.stopping = false;
    match p.pid {
        Some(pid) => {
            p.restarting = true;
            terminate(pid)
        }
        None => {
            start(&mut processes, id);
            Ok(())
        }
    }
}

/// Stops process `id`; a kept one stays down until it's restarted.
pub fn stop(id: u32) -> Result<(), String> {
    let mut processes = processes();
    let p = processes
        .get(id)
        .ok_or_else(|| format!("no process {id}"))?;
    let Some(pid) = p.pid else { return Ok(()) };
    p.restarting = false;
    p.stopping = true;
    terminate(pid)
}

/// Asks everything that's running to quit, as the compositor does on its way
/// out (a restart or a log-out), so nothing it started outlives it: a program
/// that has no Wayland connection to lose, like the tiler or game mode's
/// daemon, would otherwise be left running, one more with each restart. Kept
/// pieces stay down.
pub fn stop_all() {
    stop_matching(|_| true);
}

fn stop_matching(which: impl Fn(&Process) -> bool) {
    let mut processes = processes();
    for p in processes.list.iter_mut().filter(|p| which(p)) {
        let Some(pid) = p.pid else { continue };
        p.restarting = false;
        p.stopping = true;
        let _ = terminate(pid);
    }
}

/// Process `id`'s log from line `from` on, and the line to ask from next.
pub fn log(id: u32, from: u64) -> Option<(u64, Vec<String>)> {
    let mut processes = processes();
    let p = processes.get(id)?;
    let next = p.first + p.log.len() as u64;
    let skip = from.saturating_sub(p.first).min(p.log.len() as u64) as usize;
    Some((next, p.log.iter().skip(skip).cloned().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(id: u32, done: impl Fn(&Entry) -> bool) -> Entry {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let entry = list().into_iter().find(|e| e.id == id).expect("listed");
            if done(&entry) {
                return entry;
            }
            assert!(Instant::now() < deadline, "{entry:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn last_id() -> u32 {
        processes().next - 1
    }

    #[test]
    fn names_are_the_programs_file_name() {
        assert_eq!(name_of("patina-volume --flash"), "patina-volume");
        assert_eq!(name_of("/usr/bin/foot -e top"), "foot");
    }

    #[test]
    fn output_is_logged_and_a_run_once_exit_is_kept() {
        spawn(
            OsStr::new("test"),
            "echo hello; echo oops >&2; exit 3",
            false,
        );
        let id = last_id();
        let entry = wait_for(id, |e| e.status == "exited with 3");
        assert_eq!(entry.pid, 0);
        // The lines arrive on their own threads; wait for them too.
        let deadline = Instant::now() + Duration::from_secs(5);
        let lines = loop {
            let (_, lines) = log(id, 0).unwrap();
            if lines.len() == 3 || Instant::now() > deadline {
                break lines;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(lines.contains(&"hello".to_string()), "{lines:?}");
        assert!(lines.contains(&"oops".to_string()), "{lines:?}");
        let (next, rest) = log(id, 2).unwrap();
        assert_eq!(next, 3);
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn stopping_what_matches_reaches_every_group_and_keeps_kept_pieces_down() {
        spawn(OsStr::new("test"), "sleep 30; true", true);
        let kept = last_id();
        spawn(OsStr::new("test"), "sleep 30; true", false);
        let once = last_id();
        spawn(OsStr::new("test"), "sleep 30; true", false);
        let other = last_id();
        let pids: Vec<u32> = [kept, once]
            .iter()
            .map(|&id| wait_for(id, |e| e.pid != 0).pid)
            .collect();
        wait_for(other, |e| e.pid != 0);
        stop_matching(|p| p.id == kept || p.id == once);
        wait_for(kept, |e| e.status == "stopped");
        wait_for(once, |e| e.pid == 0);
        for pid in pids {
            let group = rustix::process::Pid::from_raw(pid as i32).unwrap();
            assert!(rustix::process::test_kill_process_group(group).is_err());
        }
        // Not what the filter left out.
        assert_ne!(list().into_iter().find(|e| e.id == other).unwrap().pid, 0);
        stop(other).unwrap();
        wait_for(other, |e| e.pid == 0);
    }

    #[test]
    fn a_kept_piece_comes_back_until_stopped() {
        spawn(OsStr::new("test"), "sleep 0.1", true);
        let id = last_id();
        wait_for(id, |e| e.restarts >= 1 && e.status == "running");
        // Stopping reaches sleep, not just the sh in front of it.
        spawn(OsStr::new("test"), "sleep 30; true", true);
        let long = last_id();
        let pid = wait_for(long, |e| e.pid != 0).pid;
        stop(long).unwrap();
        let entry = wait_for(long, |e| e.status == "stopped");
        assert_eq!(entry.restarts, 0);
        let group = rustix::process::Pid::from_raw(pid as i32).unwrap();
        assert!(rustix::process::test_kill_process_group(group).is_err());
        restart(long).unwrap();
        wait_for(long, |e| e.status == "running" && e.restarts == 1);
        stop(long).unwrap();
        stop(id).unwrap();
        wait_for(id, |e| {
            e.status == "stopped" || e.status.starts_with("gave up")
        });
    }
}

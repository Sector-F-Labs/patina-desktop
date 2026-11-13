//! The work of game mode (ADR-0134): while a fullscreen window has the keyboard,
//! ask the processes the user listed to quit and run their commands; when it
//! leaves, run the commands for leaving. Nothing is put back but by those.
//!
//! The decision (`Gate`) is pure. The work runs on a thread of its own, since a
//! command can take a while, one transition at a time. A marker file in
//! `XDG_RUNTIME_DIR` says game mode is on, for Settings to show.

use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::Config;

/// How long a command may take before it's stopped.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Transitions run one at a time, so a quick leave waits for the enter.
static TRANSITION: Mutex<()> = Mutex::new(());

/// `$XDG_RUNTIME_DIR/patina/game-mode`: there while game mode is on.
pub fn marker_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(dir).join("patina").join("game-mode"))
}

/// `$XDG_RUNTIME_DIR/patina/game-mode.toggle`: Settings and `--toggle` make it
/// to switch game mode by hand; the daemon removes it and does so.
pub fn toggle_request_path() -> Option<PathBuf> {
    marker_path().map(|p| p.with_file_name("game-mode.toggle"))
}

/// Whether a toggle was asked for, taking the request.
pub fn take_toggle_request() -> bool {
    toggle_request_path().is_some_and(|p| std::fs::remove_file(p).is_ok())
}

/// Makes this the one daemon: one still running from an earlier session (the
/// compositor doesn't stop its helpers when it restarts) is asked to quit, so
/// two never run the same commands. Its process id is kept in
/// `$XDG_RUNTIME_DIR/patina/game-mode.pid`.
pub fn take_over() {
    let Some(path) = marker_path().map(|p| p.with_extension("pid")) else {
        return;
    };
    let me = std::process::id();
    if let Some(old) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .filter(|&pid| pid != me && is_game_mode(pid))
    {
        terminate(old);
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, me.to_string());
}

/// Whether `pid` is a patina-game-mode, not something that has its number now.
fn is_game_mode(pid: u32) -> bool {
    // The kernel keeps 15 characters of the name: "patina-game-mod".
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .is_ok_and(|c| name_matches("patina-game-mode", c.trim_end_matches('\n')))
}

/// A marker left by an earlier run is stale: nothing here knows it is on.
pub fn clear_marker() {
    if let Some(p) = marker_path() {
        let _ = std::fs::remove_file(p);
    }
}

/// When game mode is on or off: it changes only once what's wanted has
/// differed from it for the whole delay.
#[derive(Debug, Default)]
pub struct Gate {
    pub on: bool,
    differing_since: Option<Instant>,
}

impl Gate {
    /// Takes what's wanted now; says what to change to, if it's time.
    pub fn step(&mut self, want: bool, delay: Duration, now: Instant) -> Option<bool> {
        if want == self.on {
            self.differing_since = None;
            return None;
        }
        let since = *self.differing_since.get_or_insert(now);
        if now.duration_since(since) >= delay {
            self.on = want;
            self.differing_since = None;
            return Some(want);
        }
        None
    }
}

#[derive(Debug, Default)]
pub struct GameMode {
    pub gate: Gate,
    /// The user's say, from the toggle: `None` leaves it to the fullscreen window.
    pub manual: Option<bool>,
    /// A press just changed what's wanted: that change doesn't wait out the delay.
    pressed: bool,
}

impl GameMode {
    /// What's wanted: the toggle if it's been used, else whether a fullscreen
    /// window has the keyboard, when automatic is on.
    pub fn wanted(&self, settings: &Config, fullscreen: bool) -> bool {
        self.manual.unwrap_or(settings.enabled && fullscreen)
    }

    /// Switches it by hand, whatever is on screen; a second press hands it back.
    pub fn toggle(&mut self) {
        self.manual = match self.manual {
            Some(_) => None,
            None => Some(!self.gate.on),
        };
        self.pressed = true;
    }

    /// How long a change in what's wanted has to last: none when the user asked
    /// for it just now, or switched the whole thing off.
    fn delay(&self, settings: &Config) -> Duration {
        if self.pressed || !settings.enabled {
            Duration::ZERO
        } else {
            Duration::from_secs(settings.delay)
        }
    }

    /// Takes what's on screen; starts the work on a change.
    pub fn update(&mut self, settings: &Config, fullscreen: bool, now: Instant) {
        let want = self.wanted(settings, fullscreen);
        let delay = self.delay(settings);
        self.pressed = false;
        let settings = settings.clone();
        match self.gate.step(want, delay, now) {
            Some(true) => spawn_transition(move || {
                notify("Game mode on");
                enter(&settings);
            }),
            Some(false) => spawn_transition(move || {
                leave(&settings);
                notify("Game mode off");
            }),
            None => {}
        }
    }
}

fn spawn_transition(work: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("game-mode".into())
        .spawn(move || {
            let _guard = TRANSITION.lock().unwrap_or_else(|e| e.into_inner());
            work();
        });
}

fn set_marker(on: bool) {
    let Some(path) = marker_path() else { return };
    if on {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, "");
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// Shows `text` at the top of the screen, without waiting for it: it's this
/// program run again with `--toast`, so a surface that goes wrong can't take the
/// watching with it. By name, from the path, as the compositor starts the
/// others: after an upgrade replaces the file, a running program's own path
/// reads "(deleted)".
fn notify(text: &str) {
    let child = Command::new("patina-game-mode")
        .args(["--toast", text])
        .stdin(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        // Reaped when it's gone, so it doesn't stay a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Asks the listed processes to quit, then runs the commands for entering.
pub fn enter(settings: &Config) {
    set_marker(true);
    for pid in find(&settings.kill) {
        terminate(pid);
    }
    for command in &settings.on_enter {
        run(command);
    }
}

/// Runs the commands for leaving.
pub fn leave(settings: &Config) {
    for command in &settings.on_exit {
        run(command);
    }
    set_marker(false);
}

fn terminate(pid: u32) -> bool {
    rustix::process::Pid::from_raw(pid as i32).is_some_and(|pid| {
        rustix::process::kill_process(pid, rustix::process::Signal::TERM).is_ok()
    })
}

/// The ids of the user's own processes named in `names` (the kernel keeps 15
/// characters of a name), but for this one.
fn find(names: &[String]) -> Vec<u32> {
    if names.is_empty() {
        return Vec::new();
    }
    let me = std::process::id();
    let uid = rustix::process::getuid().as_raw();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me || entry.metadata().map(|m| m.uid()).ok() != Some(uid) {
            continue;
        }
        let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) else {
            continue;
        };
        if names
            .iter()
            .any(|n| name_matches(n, comm.trim_end_matches('\n')))
        {
            found.push(pid);
        }
    }
    found
}

/// Whether the user's `name` is the process called `comm`.
pub fn name_matches(name: &str, comm: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let kept: String = name.bytes().take(15).map(char::from).collect();
    kept == comm
}

/// Runs `command` through sh(1), stopped if it takes too long. Its trouble
/// goes to the log.
fn run(command: &str) {
    let mut sh = Command::new("sh");
    sh.arg("-c").arg(command);
    match capture(sh) {
        Ok(true) => {}
        Ok(false) => eprintln!("patina-game-mode: `{command}` failed"),
        Err(e) => eprintln!("patina-game-mode: `{command}`: {e}"),
    }
}

/// Runs it in a group of its own, gives it `COMMAND_TIMEOUT`, and says whether
/// it succeeded.
fn capture(mut command: Command) -> Result<bool, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    // Drained, so a chatty command isn't stuck on a full pipe.
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_string(&mut out);
        }
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() > COMMAND_TIMEOUT => {
                if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                    let _ =
                        rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
                }
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let _ = reader.join();
    status
        .map(|s| s.success())
        .ok_or_else(|| "took too long, stopped".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Config {
        Config {
            enabled: true,
            ..Config::default()
        }
    }

    #[test]
    fn names_are_matched_whole_up_to_the_kernels_limit() {
        assert!(name_matches("rust-analyzer", "rust-analyzer"));
        assert!(!name_matches("rust", "rust-analyzer"));
        assert!(!name_matches("", ""));
        assert!(name_matches("a-very-long-program-name", "a-very-long-pro"));
    }

    #[test]
    fn the_gate_waits_out_the_delay_both_ways() {
        let t = Instant::now();
        let s = Duration::from_secs;
        let mut gate = Gate::default();
        assert_eq!(gate.step(true, s(5), t), None);
        assert_eq!(gate.step(true, s(5), t + s(4)), None);
        // Gone for a moment before the delay: starts over.
        assert_eq!(gate.step(false, s(5), t + s(4)), None);
        assert_eq!(gate.step(true, s(5), t + s(5)), None);
        assert_eq!(gate.step(true, s(5), t + s(10)), Some(true));
        assert_eq!(gate.step(true, s(5), t + s(11)), None);
        assert_eq!(gate.step(false, s(5), t + s(12)), None);
        assert_eq!(gate.step(false, s(5), t + s(17)), Some(false));
        // No delay is at once.
        assert_eq!(gate.step(true, Duration::ZERO, t + s(18)), Some(true));
    }

    #[test]
    fn automatic_and_the_toggle_decide_what_is_wanted() {
        let mut s = settings();
        let mut mode = GameMode::default();
        assert!(mode.wanted(&s, true));
        assert!(!mode.wanted(&s, false));
        s.enabled = false;
        assert!(!mode.wanted(&s, true));
        // By hand it works with nothing on screen, then hands it back.
        mode.toggle();
        assert!(mode.wanted(&s, false));
        mode.toggle();
        assert_eq!(mode.manual, None);
    }

    #[test]
    fn a_press_does_not_wait_out_the_delay_but_the_window_does() {
        let s = settings();
        let mut mode = GameMode::default();
        assert_eq!(mode.delay(&s), Duration::from_secs(s.delay));
        mode.toggle();
        assert_eq!(mode.delay(&s), Duration::ZERO, "on, at once");
        mode.pressed = false;
        // Held on by hand, until the second press hands it back, and that's at once too.
        mode.toggle();
        assert_eq!(mode.delay(&s), Duration::ZERO, "off, at once");
    }

    #[test]
    fn only_a_game_mode_is_taken_for_one() {
        assert!(
            !is_game_mode(std::process::id()),
            "the test binary has another name"
        );
        assert!(!is_game_mode(u32::MAX));
        // What the kernel calls it, cut to 15 characters.
        assert!(name_matches("patina-game-mode", "patina-game-mod"));
    }

    #[test]
    fn entering_quits_the_listed_processes_and_runs_the_commands() {
        let dir = std::env::temp_dir().join(format!("gm-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A program of its own name (a copy of sh, waiting), so the test can't
        // touch anything but itself.
        let spawn = |name: &str| {
            let program = dir.join(name);
            std::fs::copy("/bin/sh", &program).unwrap();
            Command::new(&program)
                .args(["-c", "sleep 60 & wait"])
                .spawn()
                .unwrap()
        };
        let mut killed = spawn("gmtest-kill");
        let mut other = spawn("gmtest-other");
        let marker = dir.join("entered");
        let s = Config {
            kill: vec!["gmtest-kill".into()],
            on_enter: vec![format!("touch {}", marker.display())],
            on_exit: vec![format!("rm {}", marker.display())],
            ..settings()
        };
        enter(&s);
        assert!(marker.exists());
        assert!(killed.wait().is_ok(), "asked to quit");
        assert!(
            other.try_wait().unwrap().is_none(),
            "the rest is left alone"
        );
        leave(&s);
        assert!(!marker.exists());

        let _ = other.kill();
        let _ = other.wait();
        let _ = std::fs::remove_dir_all(dir);
    }
}

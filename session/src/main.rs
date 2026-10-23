//! patina-session: the Patina desktop's login session (ADR-0046). The login
//! screen runs it, and it runs patina-compositor, starting it again when it
//! asks to be (exit status 75, Ctrl+Alt+Shift+Backspace or Restart Desktop
//! in Settings) or when it crashes. A clean exit (Ctrl+Alt+Backspace) ends
//! the session, and so do three crashes within a minute, rather than a loop
//! the login screen never comes back from.
//!
//! The compositor knows it's here by `PATINA_SESSION`, this process's ID.
//! It runs with the environment a login shell sets up (ADR-0083), as a
//! display manager's Xsession gives other desktops: `~/.profile`'s PATH
//! with `~/.local/bin` in it, and the rest.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

/// The compositor's exit status asking to be started again; the
/// compositor's `session.rs` has it too.
const RESTART: i32 = 75;
/// Crashes that end the session, within this long.
const GIVE_UP_AFTER: usize = 3;
const GIVE_UP_WITHIN: Duration = Duration::from_secs(60);
/// The pause before starting a crashed compositor again.
const AFTER_CRASH: Duration = Duration::from_secs(1);
/// How long a login shell gets to print its environment; a profile that
/// hangs (waiting on input, or the network) is given up on.
const PROFILE_WAIT: Duration = Duration::from_secs(5);
/// The shell's own variables, not the session's.
const SHELL_OWN: [&str; 4] = ["SHLVL", "_", "PWD", "OLDPWD"];

const USAGE: &str = "usage: patina-session [compositor argument ...]

Runs patina-compositor with the arguments given (the login screen's entry
gives --session), and starts it again when it restarts or crashes. It ends
when the compositor quits, or crashes three times within a minute.
See docs/running-as-a-session.md.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-session {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let compositor = compositor();
    let mut login = Command::new("/bin/sh");
    login.args(["-l", "-c", "exec env -0"]);
    let env = match login_environment(&mut login, PROFILE_WAIT) {
        Ok(env) => env,
        Err(e) => {
            eprintln!("patina-session: going on without the login profile: {e}");
            Vec::new()
        }
    };
    let ending = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicU32::new(0));
    if let Err(e) = forward_signals(ending.clone(), running.clone()) {
        eprintln!("patina-session: can't watch for signals: {e}");
    }

    let code = supervise(&compositor, &args, &env, &ending, &running);
    forget_lock();
    code
}

/// Runs `compositor` with `args` and `env` until the session ends, starting
/// it again when it restarts or crashes. `ending` is set when the session is
/// told to end; `running` holds the compositor's process ID while it runs.
fn supervise(
    compositor: &Path,
    args: &[String],
    env: &[(OsString, OsString)],
    ending: &AtomicBool,
    running: &AtomicU32,
) -> ExitCode {
    let mut crashes: VecDeque<Instant> = VecDeque::new();
    loop {
        if ending.load(Ordering::SeqCst) {
            break ExitCode::SUCCESS;
        }
        let mut child = match Command::new(compositor)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .env("PATINA_SESSION", std::process::id().to_string())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                eprintln!("patina-session: can't run {}: {e}", compositor.display());
                break ExitCode::FAILURE;
            }
        };
        running.store(child.id(), Ordering::SeqCst);
        let status = child.wait();
        running.store(0, Ordering::SeqCst);
        if ending.load(Ordering::SeqCst) {
            break ExitCode::SUCCESS;
        }
        let how = match status {
            Ok(s) if s.success() => break ExitCode::SUCCESS,
            Ok(s) if s.code() == Some(RESTART) => {
                eprintln!("patina-session: restarting the desktop");
                continue;
            }
            Ok(s) => s.to_string(),
            Err(e) => e.to_string(),
        };
        let now = Instant::now();
        crashes.push_back(now);
        crashes.retain(|t| now.duration_since(*t) <= GIVE_UP_WITHIN);
        if crashes.len() >= GIVE_UP_AFTER {
            eprintln!(
                "patina-session: the compositor crashed ({how}) {GIVE_UP_AFTER} times in a minute; ending the session"
            );
            break ExitCode::FAILURE;
        }
        eprintln!("patina-session: the compositor crashed ({how}); starting it again");
        std::thread::sleep(AFTER_CRASH);
    }
}

/// The environment a login shell sets up (`sh -l`: `/etc/profile`, then
/// `~/.profile`), as `login` prints it with `env -0`; within `wait`, or an
/// error. The profile's own output and questions go nowhere.
fn login_environment(login: &mut Command, wait: Duration) -> Result<Vec<(OsString, OsString)>, String> {
    let mut child = login
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("can't run the login shell: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no output")?;
    // Read on a thread, and waited for no longer than the shell: a program
    // the profile starts in the background can hold the pipe open.
    let (send, read) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        let _ = send.send(out);
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > wait {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("the profile took over {} s", wait.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        return Err(format!("the login shell failed ({status})"));
    }
    let out = read
        .recv_timeout(wait.saturating_sub(started.elapsed()).max(Duration::from_millis(500)))
        .map_err(|_| "a program the profile started kept its output open")?;
    Ok(parse_environment(&out))
}

/// `env -0`'s output, NAME=value each ended by a NUL, without the shell's
/// own variables.
fn parse_environment(out: &[u8]) -> Vec<(OsString, OsString)> {
    out.split(|b| *b == 0)
        .filter_map(|entry| {
            let at = entry.iter().position(|b| *b == b'=')?;
            let (name, value) = (&entry[..at], &entry[at + 1..]);
            let name = std::ffi::OsStr::from_bytes(name);
            if name.is_empty() || SHELL_OWN.iter().any(|own| name == *own) {
                return None;
            }
            Some((name.to_owned(), std::ffi::OsStr::from_bytes(value).to_owned()))
        })
        .collect()
}

/// patina-compositor beside this binary, as `make install` and the build
/// put them, or else from PATH.
fn compositor() -> PathBuf {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("patina-compositor")))
        .filter(|p| p.exists());
    beside.unwrap_or_else(|| PathBuf::from("patina-compositor"))
}

/// When the session is told to end (the login manager's SIGTERM or SIGHUP,
/// or Ctrl+C), passes it on to the compositor and doesn't start it again.
fn forward_signals(ending: Arc<AtomicBool>, running: Arc<AtomicU32>) -> std::io::Result<()> {
    let mut signals = signal_hook::iterator::Signals::new([SIGTERM, SIGHUP, SIGINT])?;
    std::thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            for signal in signals.forever() {
                ending.store(true, Ordering::SeqCst);
                let pid = running.load(Ordering::SeqCst);
                let Some(pid) = i32::try_from(pid)
                    .ok()
                    .and_then(rustix::process::Pid::from_raw)
                else {
                    continue;
                };
                let signal = match signal {
                    SIGHUP => rustix::process::Signal::HUP,
                    SIGINT => rustix::process::Signal::INT,
                    _ => rustix::process::Signal::TERM,
                };
                let _ = rustix::process::kill_process(pid, signal);
            }
        })?;
    Ok(())
}

/// The compositor's note that the screen is locked, kept across restarts,
/// goes with the session (see the compositor's `session.rs`).
fn forget_lock() {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let marker =
            PathBuf::from(dir).join(format!("patina-session-{}.locked", std::process::id()));
        let _ = std::fs::remove_file(marker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supervises a stand-in compositor: `script`, in a scratch folder it
    /// can count its runs in. Whether the session ended well, and the runs.
    fn run(name: &str, script: &str) -> (ExitCode, usize) {
        let dir =
            std::env::temp_dir().join(format!("patina-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let compositor = dir.join("compositor");
        std::fs::write(
            &compositor,
            format!(
                "#!/bin/sh\ncd {}\necho run >> runs\n{script}\n",
                dir.display()
            ),
        )
        .unwrap();
        // Through sh, not run itself: a file just written can't be run
        // while another test's fork still holds it open (ETXTBSY).
        let code = supervise(
            Path::new("sh"),
            &[compositor.to_string_lossy().into_owned()],
            &[(
                OsString::from("PATINA_FROM_PROFILE"),
                OsString::from("yes"),
            )],
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        );
        let runs = std::fs::read_to_string(dir.join("runs"))
            .unwrap()
            .lines()
            .count();
        let _ = std::fs::remove_dir_all(&dir);
        (code, runs)
    }

    #[test]
    fn a_clean_exit_ends_the_session() {
        assert_eq!(run("clean", "exit 0"), (ExitCode::SUCCESS, 1));
    }

    #[test]
    fn a_restart_starts_it_again() {
        // Restarts twice, then quits.
        let script = "[ $(wc -l < runs) -lt 3 ] && exit 75; exit 0";
        assert_eq!(run("restart", script), (ExitCode::SUCCESS, 3));
    }

    #[test]
    fn crashing_three_times_in_a_minute_ends_it() {
        assert_eq!(run("crash", "exit 101"), (ExitCode::FAILURE, 3));
    }

    #[test]
    fn it_starts_again_after_one_crash() {
        let script = "[ $(wc -l < runs) -lt 2 ] && kill -SEGV $$; exit 0";
        assert_eq!(run("once", script), (ExitCode::SUCCESS, 2));
    }

    #[test]
    fn the_compositor_gets_the_login_environment() {
        let script = "[ \"$PATINA_FROM_PROFILE\" = yes ] && exit 0; exit 101";
        assert_eq!(run("env", script).0, ExitCode::SUCCESS);
    }

    #[test]
    fn env_dash_zero_reads_back_without_the_shells_own() {
        let env = parse_environment(b"PATH=/a:/b\0SHLVL=2\0EMPTY=\0X=a=b\nc\0junk\0");
        let env: Vec<(String, String)> = env
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect();
        let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(pairs, [("PATH", "/a:/b"), ("EMPTY", ""), ("X", "a=b\nc")]);
    }

    /// A login shell running `script`, standing in for `sh -l`.
    fn login(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn a_login_shell_gives_its_environment() {
        let env = login_environment(&mut login("exec env -0"), PROFILE_WAIT).unwrap();
        assert!(env.iter().any(|(k, _)| k == "PATH"));
        assert!(!env.iter().any(|(k, _)| k == "SHLVL"));
    }

    #[test]
    fn a_profile_that_hangs_fails_or_holds_the_pipe_is_given_up_on() {
        let short = Duration::from_millis(300);
        let started = Instant::now();
        assert!(login_environment(&mut login("exec sleep 30"), short).is_err());
        assert!(login_environment(&mut login("exit 3"), short).is_err());
        // A background program with the pipe: the shell's done, its
        // output never ends.
        assert!(login_environment(&mut login("sleep 30 & exec env -0"), short).is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn it_tells_the_compositor_it_is_here() {
        let script = "[ \"$PATINA_SESSION\" = $PPID ] && exit 0; exit 101";
        assert_eq!(run("told", script).0, ExitCode::SUCCESS);
    }
}

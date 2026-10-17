//! patina-session: the Patina desktop's login session (ADR-0046). The login
//! screen runs it, and it runs patina-compositor, starting it again when it
//! asks to be (exit status 75, Ctrl+Alt+Shift+Backspace or Restart Desktop
//! in Settings) or when it crashes. A clean exit (Ctrl+Alt+Backspace) ends
//! the session, and so do three crashes within a minute, rather than a loop
//! the login screen never comes back from.
//!
//! The compositor knows it's here by `PATINA_SESSION`, this process's ID.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
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
    let ending = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicU32::new(0));
    if let Err(e) = forward_signals(ending.clone(), running.clone()) {
        eprintln!("patina-session: can't watch for signals: {e}");
    }

    let code = supervise(&compositor, &args, &ending, &running);
    forget_lock();
    code
}

/// Runs `compositor` with `args` until the session ends, starting it again
/// when it restarts or crashes. `ending` is set when the session is told to
/// end; `running` holds the compositor's process ID while it runs.
fn supervise(
    compositor: &Path,
    args: &[String],
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
    fn it_tells_the_compositor_it_is_here() {
        let script = "[ \"$PATINA_SESSION\" = $PPID ] && exit 0; exit 101";
        assert_eq!(run("told", script).0, ExitCode::SUCCESS);
    }
}

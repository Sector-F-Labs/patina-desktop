//! Running under patina-session (ADR-0046), which starts the compositor
//! again when it exits asking to be, or crashes. It says so with
//! `PATINA_SESSION`, its own process ID, which the compositor's clients
//! don't inherit.
//!
//! `dbus.rs` serves locking and logging out to the bar's power menu
//! (ADR-0048).
//!
//! A restart can't keep the lock screen's state, so while the screen is
//! locked a marker in `XDG_RUNTIME_DIR` says it is. A compositor started
//! under patina-session that finds one starts locked, so crashing it never
//! opens the desktop.

pub mod dbus;

use std::path::PathBuf;

use crate::state::State;

/// The exit status that asks patina-session to start the compositor again
/// (EX_TEMPFAIL); `session/src/main.rs` has it too.
pub const RESTART: u8 = 75;
pub const VAR: &str = "PATINA_SESSION";

/// patina-session's ID, when it's what started the compositor.
fn supervisor() -> Option<String> {
    std::env::var(VAR).ok().filter(|v| !v.is_empty())
}

/// Whether patina-session started the compositor, and so would start it
/// again.
pub fn supervised() -> bool {
    supervisor().is_some()
}

/// Where the lock is noted, for this session.
fn locked_marker() -> Option<PathBuf> {
    let id = supervisor()?;
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(dir).join(format!("patina-session-{id}.locked")))
}

/// Notes that the screen is locked, or no longer is.
pub fn mark_locked(locked: bool) {
    let Some(marker) = locked_marker() else {
        return;
    };
    let result = if locked {
        std::fs::write(&marker, b"")
    } else {
        std::fs::remove_file(&marker).or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })
    };
    if let Err(e) = result {
        eprintln!("patina-compositor: {}: {e}", marker.display());
    }
}

fn was_locked() -> bool {
    locked_marker().is_some_and(|m| m.exists())
}

impl State {
    /// Quits so patina-session starts the compositor again: every window
    /// closes, and the desktop comes back. Not without patina-session,
    /// where it would end the session, nor while the screen is locked.
    pub fn restart_desktop(&mut self) -> Result<(), String> {
        if !supervised() {
            return Err("the desktop can only restart under patina-session".into());
        }
        if self.locked() {
            return Err("the screen is locked".into());
        }
        eprintln!("patina-compositor: restarting the desktop");
        self.restart = true;
        self.loop_signal.stop();
        Ok(())
    }

    /// Ends the session: quits cleanly, so patina-session ends too and the
    /// login screen comes back; nested, the window closes.
    pub fn log_out(&mut self) {
        eprintln!("patina-compositor: logging out");
        self.restart = false;
        self.loop_signal.stop();
    }

    /// At start-up: if the compositor before this one left the screen
    /// locked, it starts locked, black until the locker draws.
    pub fn resume_lock(&mut self) {
        if !was_locked() {
            return;
        }
        eprintln!("patina-compositor: the screen was locked; locking it again");
        self.lock = crate::lock::Lock::Locked(None);
        self.needs_redraw = true;
        let command = self.config.lock.command.clone();
        if !command.is_empty() {
            self.spawn_client(&command);
        }
    }
}

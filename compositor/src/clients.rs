//! Starting the desktop's programs as clients of this compositor: the bar
//! at start-up, and the launcher and volume menu from the compositor's own
//! keys. `processes.rs` keeps track of them once started.

use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};

/// How clients, and services started for them, know which desktop this is.
pub const DESKTOP_ENV: [(&str, &str); 3] = [
    ("XDG_CURRENT_DESKTOP", "Patina"),
    ("XDG_SESSION_DESKTOP", "patina"),
    ("XDG_SESSION_TYPE", "wayland"),
];

/// PATH for clients, with this binary's folder first, so the desktop's own
/// programs (patina-bar, patina-launcher, patina-volume, built into the
/// same target folder) are found beside it before any installed copies.
fn client_path() -> OsString {
    let mut dirs: Vec<std::path::PathBuf> = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
        .into_iter()
        .collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// `command`, through sh(1), set up as a client of the compositor whose
/// socket is `socket`.
pub fn command(socket: &OsStr, command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c")
        .arg(command)
        .env("PATH", client_path())
        .env("WAYLAND_DISPLAY", socket)
        .envs(DESKTOP_ENV)
        .env_remove("WAYLAND_SOCKET")
        .env_remove(crate::session::VAR)
        .stdin(Stdio::null());
    // X11 apps reach Xwayland (ADR-0040); without it, not the parent's X.
    match crate::xwayland::display() {
        Some(display) => c.env("DISPLAY", display),
        None => c.env_remove("DISPLAY"),
    };
    c
}

/// Starts `command` as a client, once, listed among the desktop's
/// processes (ADR-0045).
pub fn spawn(socket: &OsStr, command_line: &str) {
    crate::processes::spawn(socket, command_line, false);
}

impl crate::state::State {
    pub fn spawn_client(&self, command_line: &str) {
        spawn(&self.socket_name, command_line);
    }

    /// Starts a piece of the desktop that's started again whenever it
    /// exits: the wallpaper and the bar.
    pub fn keep_client(&self, command_line: &str) {
        crate::processes::spawn(&self.socket_name, command_line, true);
    }
}

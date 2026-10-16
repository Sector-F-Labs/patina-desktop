//! Starting the desktop's programs as clients of this compositor: the bar
//! at start-up, and the launcher and volume menu from the compositor's own
//! keys.

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
        .stdin(Stdio::null());
    // X11 apps reach Xwayland (ADR-0040); without it, not the parent's X.
    match crate::xwayland::display() {
        Some(display) => c.env("DISPLAY", display),
        None => c.env_remove("DISPLAY"),
    };
    c
}

/// Starts `command` as a client, and reaps it on a thread when it ends.
pub fn spawn(socket: &OsStr, command_line: &str) {
    match command(socket, command_line).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("patina-compositor: can't run {command_line}: {e}"),
    }
}

impl crate::state::State {
    pub fn spawn_client(&self, command_line: &str) {
        spawn(&self.socket_name, command_line);
    }
}

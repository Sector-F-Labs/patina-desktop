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
/// programs (patina-hud, patina-launcher, patina-volume, built into the
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

/// The session's terminal, for the apps that open one (`$TERMINAL`, which
/// patina-apps reads for Files and the launcher): Teleprinter where it's on
/// `path`, unless the user set their own; else none, and they fall back to
/// foot.
fn terminal(path: &OsStr, own: Option<OsString>) -> Option<&'static str> {
    if own.is_some_and(|t| !t.is_empty()) {
        return None;
    }
    std::env::split_paths(path)
        .any(|d| d.join("teleprinter").is_file())
        .then_some("teleprinter")
}

/// `command`, through sh(1), set up as a client of the compositor whose
/// socket is `socket`.
pub fn command(socket: &OsStr, command: &str) -> Command {
    let path = client_path();
    let mut c = Command::new("sh");
    if let Some(terminal) = terminal(&path, std::env::var_os("TERMINAL")) {
        c.env("TERMINAL", terminal);
    }
    c.arg("-c")
        .arg(command)
        .env("PATH", path)
        .env("WAYLAND_DISPLAY", socket)
        .envs(DESKTOP_ENV)
        .env_remove("WAYLAND_SOCKET")
        .env_remove(crate::session::VAR)
        // What the bar tells the pieces it opens, which a compositor
        // started from one of them (a terminal) would pass on to its own.
        .env_remove("PATINA_SKIN")
        .env_remove("PATINA_HUD_EDGE")
        .env_remove("PATINA_OPEN_AT")
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

    /// Starts `app_id`'s app, as its `.desktop` entry says, for a pinned
    /// app's tab with no window (ADR-0080); on a thread, since that reads
    /// the installed entries.
    pub fn start_app(&self, app_id: String) {
        let socket = self.socket_name.clone();
        std::thread::spawn(move || {
            match patina_system::apps::start_command(&patina_system::apps::entries(), &app_id) {
                Some(command) => spawn(&socket, &command),
                None => eprintln!("patina-compositor: no .desktop entry or program for {app_id:?}"),
            }
        });
    }

    /// Starts a piece of the desktop that's started again whenever it
    /// exits: the wallpaper and the bar.
    pub fn keep_client(&self, command_line: &str) {
        crate::processes::spawn(&self.socket_name, command_line, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teleprinter_is_the_terminal_unless_the_user_chose_one() {
        let dir = std::env::temp_dir().join(format!("patina-terminal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::env::join_paths([&dir]).unwrap();
        assert_eq!(terminal(&path, None), None, "not installed");
        std::fs::write(dir.join("teleprinter"), "").unwrap();
        assert_eq!(terminal(&path, None), Some("teleprinter"));
        assert_eq!(terminal(&path, Some("".into())), Some("teleprinter"));
        assert_eq!(terminal(&path, Some("kitty".into())), None, "theirs stays");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

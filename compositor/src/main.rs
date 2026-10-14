//! patina-compositor: a floating Wayland compositor for the Patina desktop,
//! built on Smithay. For now it runs as a window on another desktop and
//! starts patina-bar and a terminal inside it.

mod grabs;
mod maximize;
mod nested;
mod state;
mod toplevels;

use std::process::{Command, ExitCode, Stdio};

use smithay::{
    output::{Output, PhysicalProperties, Subpixel},
    reexports::{calloop::EventLoop, wayland_server::Display},
};

use crate::state::State;

const USAGE: &str = "usage: patina-compositor [--session] [command ...]

Opens a window on the current Wayland desktop and runs a Patina desktop in
it: patina-bar, then each command given, with WAYLAND_DISPLAY set to the
compositor's socket. Apps are started from the bar's launcher.

--session  run as the login session (the Patina entry at the login screen
           starts it so): also tells systemd and D-Bus about the display.
           See docs/running-as-a-session.md.";

/// How clients, and services started for them, know which desktop this is.
const DESKTOP_ENV: [(&str, &str); 3] = [
    ("XDG_CURRENT_DESKTOP", "Patina"),
    ("XDG_SESSION_DESKTOP", "patina"),
    ("XDG_SESSION_TYPE", "wayland"),
];

fn main() -> ExitCode {
    let mut commands: Vec<String> = Vec::new();
    let mut session = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--session" => session = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-compositor {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            a if a.starts_with('-') => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
            _ => commands.push(arg),
        }
    }
    match run(&commands, session) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-compositor: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(commands: &[String], session: bool) -> Result<(), String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(
            "there's no desktop to open a window on (WAYLAND_DISPLAY is unset), and \
             running straight on the hardware isn't built yet; see docs/running-as-a-session.md"
                .into(),
        );
    }
    let mut event_loop: EventLoop<'static, State> =
        EventLoop::try_new().map_err(|e| e.to_string())?;
    let display: Display<State> = Display::new().map_err(|e| e.to_string())?;

    let output = Output::new(
        "PATINA-1".into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Patina".into(),
            model: "Nested".into(),
        },
    );
    let mode = nested::mode(nested::DEFAULT_SIZE);
    output.change_current_state(Some(mode), None, None, Some((0, 0).into()));
    output.set_preferred(mode);

    let nested = nested::Nested::connect(&event_loop, &output)?;
    let mut state = State::new(&mut event_loop, display, output, nested)?;
    eprintln!(
        "patina-compositor: clients connect on WAYLAND_DISPLAY={}",
        state.socket_name.to_string_lossy()
    );

    if session {
        export_to_session(&state);
    }
    spawn(&state, "patina-bar");
    for command in commands {
        spawn(&state, command);
    }

    event_loop
        .run(None, &mut state, |state| {
            toplevels::refresh(state);
            nested::render(state);
            if let Err(e) = state.display_handle.flush_clients() {
                eprintln!("patina-compositor: {e}");
            }
        })
        .map_err(|e| e.to_string())
}

/// PATH for clients, with this binary's folder first, so the desktop's own
/// programs (patina-bar, patina-launcher, built into the same target
/// folder) are found beside it before any installed copies.
fn client_path() -> std::ffi::OsString {
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

/// Tells systemd's user manager and D-Bus activation which display and
/// desktop this is, so services they start later (portals, notification
/// daemons) reach this compositor.
fn export_to_session(state: &State) {
    let mut command = Command::new("dbus-update-activation-environment");
    command
        .arg("--systemd")
        .arg("WAYLAND_DISPLAY")
        .args(DESKTOP_ENV.map(|(k, _)| k))
        .env("WAYLAND_DISPLAY", &state.socket_name)
        .envs(DESKTOP_ENV);
    match command.status() {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("patina-compositor: dbus-update-activation-environment: {s}"),
        Err(e) => eprintln!("patina-compositor: can't run dbus-update-activation-environment: {e}"),
    }
}

/// Runs `command` through sh(1) as a client of this compositor.
fn spawn(state: &State, command: &str) {
    let result = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("PATH", client_path())
        .env("WAYLAND_DISPLAY", &state.socket_name)
        .envs(DESKTOP_ENV)
        .env_remove("WAYLAND_SOCKET")
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .spawn();
    if let Err(e) = result {
        eprintln!("patina-compositor: can't run {command}: {e}");
    }
}

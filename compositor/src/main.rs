//! patina-compositor: a floating Wayland compositor for the Patina desktop,
//! built on Smithay. For now it runs as a window on another desktop and
//! starts patina-bar and a terminal inside it.

mod grabs;
mod nested;
mod state;

use std::process::{Command, ExitCode, Stdio};

use smithay::{
    output::{Output, PhysicalProperties, Subpixel},
    reexports::{calloop::EventLoop, wayland_server::Display},
};

use crate::state::State;

const USAGE: &str = "usage: patina-compositor [command ...]

Opens a window on the current Wayland desktop and runs a Patina desktop in
it: patina-bar, then each command (foot when none is given), with
WAYLAND_DISPLAY set to the compositor's socket.";

fn main() -> ExitCode {
    let mut commands: Vec<String> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
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
    if commands.is_empty() {
        commands.push("foot".into());
    }
    match run(&commands) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-compositor: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(commands: &[String]) -> Result<(), String> {
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

    spawn(&state, &bar_command());
    for command in commands {
        spawn(&state, command);
    }

    event_loop
        .run(None, &mut state, |state| {
            nested::render(state);
            if let Err(e) = state.display_handle.flush_clients() {
                eprintln!("patina-compositor: {e}");
            }
        })
        .map_err(|e| e.to_string())
}

/// patina-bar from beside this binary (both are built into the same
/// target directory), or from PATH.
fn bar_command() -> String {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("patina-bar")));
    match beside {
        Some(path) if path.exists() => path.to_string_lossy().into_owned(),
        _ => "patina-bar".into(),
    }
}

/// Runs `command` through sh(1) as a client of this compositor.
fn spawn(state: &State, command: &str) {
    let result = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("WAYLAND_DISPLAY", &state.socket_name)
        .env_remove("WAYLAND_SOCKET")
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .spawn();
    if let Err(e) = result {
        eprintln!("patina-compositor: can't run {command}: {e}");
    }
}

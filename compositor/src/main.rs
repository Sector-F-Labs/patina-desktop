//! patina-compositor: a floating Wayland compositor for the Patina desktop,
//! built on Smithay. For now it runs as a window on another desktop and
//! starts patina-hud and a terminal inside it.

mod caps;
mod capture;
mod clients;
mod clipboard;
mod cmd;
mod config;
mod constraints;
mod cursor;
mod dmabuf;
mod dnd;
mod drag;
mod focus;
mod frames;
mod fullscreen;
mod grabs;
mod idle;
mod input;
mod keyboard;
mod keys;
mod lock;
mod maximize;
mod media;
mod minimize;
mod nested;
mod night_light;
mod processes;
mod remap;
mod scale;
mod screenshot;
mod session;
mod shadow;
mod split;
mod state;
mod sync;
mod tabs;
mod tiling;
mod toplevels;
mod touch;
mod typing;
mod udev;
mod winit;
mod xwayland;

use std::process::{Command, ExitCode};

use smithay::{
    output::{Output, PhysicalProperties, Subpixel},
    reexports::{calloop::EventLoop, wayland_server::Display},
};

use crate::state::State;

const USAGE: &str = "usage: patina-compositor [--session] [command ...]

Opens a window on the current Wayland desktop and runs a Patina desktop in
it: patina-wallpaper, patina-hud and patina-tiler, then each command given,
with WAYLAND_DISPLAY set to the compositor's socket. Apps are started from the
bar's launcher.

--session  run as the login session (the Patina entry at the login screen
           starts it so). On the hardware, from a login screen or a text
           console, it always tells systemd and D-Bus about the display, so
           portals and other services started later reach it.
           See docs/running-as-a-session.md.";

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
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(session::RESTART),
        Err(e) => {
            eprintln!("patina-compositor: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the desktop until it quits; whether it's to be started again.
fn run(commands: &[String], session: bool) -> Result<bool, String> {
    let mut event_loop: EventLoop<'static, State> =
        EventLoop::try_new().map_err(|e| e.to_string())?;
    let display: Display<State> = Display::new().map_err(|e| e.to_string())?;

    // With a desktop to open a window on, run nested in it; without one
    // (a text console, or the login session), run on the hardware.
    let nested = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let mut state = if nested {
        nested_state(&mut event_loop, display)?
    } else {
        let config = config::load_or_default();
        let (mut udev, output, auto_scale) = udev::init(&event_loop, &display.handle(), &config)?;
        let formats = udev::shm_formats(&mut udev);
        let mut state = State::new(
            &mut event_loop,
            display,
            output,
            None,
            None,
            config,
            auto_scale,
        )?;
        state.udev = Some(udev);
        state.set_shm_formats(formats);
        state.centre_pointer();
        // Lock before the machine sleeps, so waking never shows the desktop.
        if state.config.lock.on_suspend {
            let (lock_now, requests) = smithay::reexports::calloop::channel::channel::<()>();
            event_loop
                .handle()
                .insert_source(requests, |event, _, state| {
                    if let smithay::reexports::calloop::channel::Event::Msg(()) = event {
                        state.lock_screen();
                    }
                })
                .map_err(|e| e.to_string())?;
            lock::watch_sleep(lock_now, state.lock_shown.clone());
        }
        state
    };
    eprintln!(
        "patina-compositor: clients connect on WAYLAND_DISPLAY={}",
        state.socket_name.to_string_lossy()
    );
    // Before anything is drawn: a restart never shows a locked desktop.
    state.resume_lock();

    // On the hardware it's the desktop, however it was started; nested, the
    // parent desktop's services must keep reaching the parent.
    if session || !nested {
        export_to_session(&state);
    }
    state.start_night_light();
    state.start_keyboard();
    state.start_clipboard();
    state.start_idle();
    state.start_tiling(&event_loop.handle());
    state.start_typing();
    // Nested too: the VM and `make run` are where the pieces are worked on.
    let _processes = processes::dbus::serve(&state.socket_name, &event_loop.handle())
        .inspect_err(|e| eprintln!("patina-compositor: the process list isn't on D-Bus: {e}"));
    let _session = session::dbus::serve(&event_loop.handle()).inspect_err(|e| {
        eprintln!("patina-compositor: locking and logging out aren't on D-Bus: {e}")
    });
    let _tabs = tabs::dbus::serve(&event_loop.handle())
        .inspect_err(|e| eprintln!("patina-compositor: the tab order isn't on D-Bus: {e}"));
    // The default apps chosen in defaults.toml, as xdg-open reads them
    // (ADR-0079), off the loop: it reads every desktop entry.
    std::thread::spawn(|| {
        if let Err(e) = patina_system::defaults::sync(&patina_system::apps::entries()) {
            eprintln!("patina-compositor: the default apps aren't written: {e}");
        }
    });
    // Before any client starts, so they all get DISPLAY.
    state.start_xwayland();
    // The wallpaper first, so it's there by the time the bar is.
    state.keep_client("patina-wallpaper");
    state.keep_client("patina-hud");
    // Tiling's decisions (ADR-0053).
    state.keep_client("patina-tiler");
    // Dictation (ADR-0072): it listens for the dictate key on the bus.
    state.keep_client("patina-dictate --daemon");
    for command in commands {
        state.spawn_client(command);
    }

    event_loop
        .run(None, &mut state, |state| {
            toplevels::refresh(state);
            udev::schedule(state);
            winit::schedule(state);
            nested::render(state);
            state.capture_frames();
            if let Err(e) = state.display_handle.flush_clients() {
                eprintln!("patina-compositor: {e}");
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(state.restart)
}

/// The compositor as a window on another desktop: OpenGL where there's
/// EGL, otherwise (or with PATINA_RENDERER=pixman) software (ADR-0015).
fn nested_state(
    event_loop: &mut EventLoop<'static, State>,
    display: Display<State>,
) -> Result<State, String> {
    let config = config::load_or_default();
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

    let software = std::env::var("PATINA_RENDERER").as_deref() == Ok("pixman");
    let gl = if software {
        None
    } else {
        match winit::init(event_loop, &output, &display.handle(), &config) {
            Ok(gl) => {
                eprintln!("patina-compositor: drawing with OpenGL ES");
                Some(gl)
            }
            Err(e) => {
                eprintln!("patina-compositor: no OpenGL ({e}); drawing in software");
                None
            }
        }
    };
    let nested = match gl {
        Some(_) => None,
        None => Some(nested::Nested::connect(event_loop, &output)?),
    };
    let auto_scale = gl.as_ref().map_or(1.0, winit::window_scale);
    let mut state = State::new(event_loop, display, output, nested, gl, config, auto_scale)?;
    if let Some(formats) = state.winit.as_mut().map(winit::shm_formats) {
        state.set_shm_formats(formats);
    }
    Ok(state)
}

/// Tells systemd's user manager and D-Bus activation which display and
/// desktop this is, so services they start later (portals, notification
/// daemons) reach this compositor.
fn export_to_session(state: &State) {
    let mut command = Command::new("dbus-update-activation-environment");
    command
        .arg("--systemd")
        .arg("WAYLAND_DISPLAY")
        .args(clients::DESKTOP_ENV.map(|(k, _)| k))
        .env("WAYLAND_DISPLAY", &state.socket_name)
        .envs(clients::DESKTOP_ENV);
    if let Some(display) = xwayland::display() {
        command.arg("DISPLAY").env("DISPLAY", display);
    }
    match command.status() {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("patina-compositor: dbus-update-activation-environment: {s}"),
        Err(e) => eprintln!("patina-compositor: can't run dbus-update-activation-environment: {e}"),
    }
}

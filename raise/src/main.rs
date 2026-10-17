//! patina-raise: focus an app's window, or start the app if none is open,
//! for a key binding (ADR-0058): `patina-raise firefox` on Alt+B. Run
//! again while the app has the focus, it steps to the app's next window.
//!
//! It sees and focuses windows through wlr-foreign-toplevel-management, as
//! the bar does, so it works on other wlroots compositors too.

mod choose;

use std::process::ExitCode;

use wayland_client::{
    Connection, Dispatch, QueueHandle, event_created_child,
    protocol::{wl_registry, wl_seat::WlSeat},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use crate::choose::{Choice, Window, choose};

const USAGE: &str = "usage: patina-raise APP [--] [COMMAND ...]
       patina-raise --list

Focuses a window of APP, matched by its app id (ignoring case, and the
last part of a reverse-DNS id: nautilus for org.gnome.Nautilus). If APP
has the focus already, it focuses APP's next window. If none is open, it
runs COMMAND, or APP itself without one.

--list prints the open windows' app ids, the focused one marked with *.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (app, command) = match args.split_first() {
        Some((first, _)) if first == "-h" || first == "--help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some((first, _)) if first == "-l" || first == "--list" => {
            return match windows() {
                Ok((_, _, list)) => {
                    for w in list {
                        println!("{}{}", if w.activated { "* " } else { "  " }, w.app_id);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("patina-raise: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Some((first, _)) if first == "-V" || first == "--version" => {
            println!("patina-raise {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some((app, rest)) if !app.starts_with('-') => {
            let rest = rest.strip_prefix(&["--".to_string()]).unwrap_or(rest);
            (app.clone(), rest.to_vec())
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let command = if command.is_empty() {
        app.clone()
    } else {
        shell_line(&command)
    };
    match raise(&app) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => start(&command),
        Err(e) => {
            eprintln!("patina-raise: {e}; starting {app}");
            start(&command)
        }
    }
}

/// `args` as one line for sh(1), each quoted.
fn shell_line(args: &[String]) -> String {
    args.iter()
        .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Starts `command` through the compositor, so it's among the desktop's
/// processes (ADR-0045), or itself where nothing serves that.
fn start(command: &str) -> ExitCode {
    if patina_system::processes::spawn(command, &[]).is_ok() {
        return ExitCode::SUCCESS;
    }
    use std::os::unix::process::CommandExt;
    match std::process::Command::new("sh")
        .args(["-c", command])
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn()
    {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-raise: can't start {command}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One open window, as the compositor describes it.
struct Toplevel {
    handle: ZwlrForeignToplevelHandleV1,
    app_id: String,
    activated: bool,
    closed: bool,
}

#[derive(Default)]
struct State {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    seat: Option<WlSeat>,
    toplevels: Vec<Toplevel>,
}

/// The open windows, with the Wayland queue and state to act on them.
fn windows() -> Result<(wayland_client::EventQueue<State>, State, Vec<Window>), String> {
    let connection = Connection::connect_to_env().map_err(|e| format!("no Wayland: {e}"))?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    connection.display().get_registry(&qh, ());
    let mut state = State::default();
    // The globals, then the windows, then each window's details.
    for _ in 0..3 {
        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("Wayland: {e}"))?;
    }
    if state.manager.is_none() {
        return Err("the compositor doesn't list its windows (wlr-foreign-toplevel)".into());
    }
    state.toplevels.retain(|t| !t.closed);
    let list = state
        .toplevels
        .iter()
        .map(|t| Window {
            app_id: t.app_id.clone(),
            activated: t.activated,
        })
        .collect();
    Ok((queue, state, list))
}

/// Focuses a window of `app`, if one is open: whether it did.
fn raise(app: &str) -> Result<bool, String> {
    let (mut queue, mut state, list) = windows()?;
    let seat = state.seat.clone().ok_or("no seat")?;
    match choose(&list, app) {
        Choice::Start => Ok(false),
        Choice::Focus(i) => {
            state.toplevels[i].handle.activate(&seat);
            queue
                .roundtrip(&mut state)
                .map_err(|e| format!("Wayland: {e}"))?;
            Ok(true)
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "zwlr_foreign_toplevel_manager_v1" => {
                state.manager = Some(registry.bind(name, version.min(3), qh, ()));
            }
            "wl_seat" if state.seat.is_none() => {
                state.seat = Some(registry.bind(name, version.min(1), qh, ()));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wayland_client::protocol::wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push(Toplevel {
                handle: toplevel,
                app_id: String::new(),
                activated: false,
                closed: false,
            });
        }
    }

    event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        let Some(t) = state.toplevels.iter_mut().find(|t| &t.handle == handle) else {
            return;
        };
        match event {
            Event::AppId { app_id } => t.app_id = app_id,
            Event::State { state } => {
                let activated = zwlr_foreign_toplevel_handle_v1::State::Activated as u32;
                t.activated = state
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .any(|s| s == activated);
            }
            Event::Closed => t.closed = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_quoted_for_sh() {
        assert_eq!(
            shell_line(&["nautilus".into(), "--new-window".into()]),
            "'nautilus' '--new-window'"
        );
        assert_eq!(shell_line(&["it's".into()]), r"'it'\''s'");
    }
}

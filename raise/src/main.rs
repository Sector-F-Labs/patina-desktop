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

use patina_system::apps::{self, Entry, shell_line};

use crate::choose::{Choice, Matching, Window, choose};

const USAGE: &str = "usage: patina-raise [-i] [-p] APP [--] [COMMAND ...]
       patina-raise --list

Focuses a window of APP. If APP has the focus already, it focuses APP's
next window. If none is open, it runs COMMAND; without one, the Exec of
APP's .desktop entry, or APP itself.

APP is matched against each window's app id, the last part of a
reverse-DNS app id (Nautilus for org.gnome.Nautilus), and its app's name
in its .desktop entry (\"WhatsApp Web\"): whole, and as written, unless

  -i, --insensitive   case is ignored
  -p, --partial       APP may be any part of them

so `patina-raise -ip telegram` finds Telegram Desktop.

--list prints the open windows' app ids and names, the focused one
marked with *.";

/// What was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    Help,
    Version,
    List,
    Raise {
        app: String,
        how: Matching,
        command: Vec<String>,
    },
}

/// Reads the arguments: options before APP (`-i`, `-p`, or both as `-ip`),
/// then APP, then the command, after an optional `--`.
fn parse(args: &[String]) -> Option<Asked> {
    let mut how = Matching::default();
    let mut rest = args.iter();
    let app = loop {
        let arg = rest.next()?;
        match arg.as_str() {
            "-h" | "--help" => return Some(Asked::Help),
            "-V" | "--version" => return Some(Asked::Version),
            "-l" | "--list" => return Some(Asked::List),
            "--insensitive" => how.insensitive = true,
            "--partial" => how.partial = true,
            "--" => break rest.next()?.clone(),
            flags if flags.starts_with('-') && flags.len() > 1 && !flags.starts_with("--") => {
                for flag in flags[1..].chars() {
                    match flag {
                        'i' => how.insensitive = true,
                        'p' => how.partial = true,
                        _ => return None,
                    }
                }
            }
            flag if flag.starts_with("--") => return None,
            app => break app.to_string(),
        }
    };
    let rest: Vec<String> = rest.cloned().collect();
    let command = match rest.split_first() {
        Some((first, after)) if first == "--" => after.to_vec(),
        _ => rest,
    };
    Some(Asked::Raise { app, how, command })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (app, how, command) = match parse(&args) {
        Some(Asked::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(Asked::Version) => {
            println!("patina-raise {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(Asked::List) => return list(&apps::entries()),
        Some(Asked::Raise { app, how, command }) => (app, how, command),
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let entries = apps::entries();
    match raise(&app, how, &entries) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => start(&starting(&app, how, &command, &entries)),
        Err(e) => {
            eprintln!("patina-raise: {e}; starting {app}");
            start(&starting(&app, how, &command, &entries))
        }
    }
}

/// What starts `app`: `command` if given, else the Exec of its entry,
/// else `app` itself.
fn starting(app: &str, how: Matching, command: &[String], entries: &[Entry]) -> String {
    if !command.is_empty() {
        return shell_line(command);
    }
    entries
        .iter()
        .filter(|e| how.is_app(&e.id, Some(&e.name), app))
        .find_map(|e| e.exec.as_deref())
        .map(shell_line)
        .unwrap_or_else(|| app.to_string())
}

/// Prints the open windows, for `--list`.
fn list(entries: &[Entry]) -> ExitCode {
    match windows(entries) {
        Ok((_, _, list)) => {
            for w in list {
                let mark = if w.activated { "* " } else { "  " };
                match &w.name {
                    Some(name) => println!("{mark}{} ({name})", w.app_id),
                    None => println!("{mark}{}", w.app_id),
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("patina-raise: {e}");
            ExitCode::FAILURE
        }
    }
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
fn windows(
    entries: &[Entry],
) -> Result<(wayland_client::EventQueue<State>, State, Vec<Window>), String> {
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
            name: apps::entry_for(entries, &t.app_id).map(|e| e.name.clone()),
            activated: t.activated,
        })
        .collect();
    Ok((queue, state, list))
}

/// Focuses a window of `app`, if one is open: whether it did.
fn raise(app: &str, how: Matching, entries: &[Entry]) -> Result<bool, String> {
    let (mut queue, mut state, list) = windows(entries)?;
    let seat = state.seat.clone().ok_or("no seat")?;
    match choose(&list, app, how) {
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

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    fn raise(app: &str, insensitive: bool, partial: bool, command: &[&str]) -> Option<Asked> {
        Some(Asked::Raise {
            app: app.into(),
            how: Matching {
                insensitive,
                partial,
            },
            command: command.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn options_come_before_the_app() {
        assert_eq!(parse(&args("firefox")), raise("firefox", false, false, &[]));
        assert_eq!(
            parse(&args("-i teleprinter")),
            raise("teleprinter", true, false, &[])
        );
        assert_eq!(
            parse(&args("-ip telegram")),
            raise("telegram", true, true, &[])
        );
        assert_eq!(
            parse(&args("--partial --insensitive whatsapp")),
            raise("whatsapp", true, true, &[])
        );
        assert_eq!(
            parse(&args("-p nautilus -- nautilus --new-window")),
            raise("nautilus", false, true, &["nautilus", "--new-window"])
        );
        assert_eq!(parse(&args("--list")), Some(Asked::List));
        assert_eq!(parse(&args("-x firefox")), None);
        assert_eq!(parse(&args("-i")), None, "no app");
    }

    fn entry(id: &str, name: &str, exec: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
            wm_class: None,
            icon: None,
            command: None,
            exec: Some(exec.iter().map(|s| s.to_string()).collect()),
        }
    }

    #[test]
    fn with_no_command_it_starts_the_entrys_exec() {
        let entries = [entry(
            "telegram-desktop_telegram-desktop",
            "Telegram",
            &["/snap/bin/telegram-desktop", "--"],
        )];
        let loose = Matching {
            insensitive: true,
            partial: true,
        };
        assert_eq!(
            starting("telegram", loose, &[], &entries),
            "'/snap/bin/telegram-desktop' '--'"
        );
        assert_eq!(starting("firefox", loose, &[], &entries), "firefox");
        assert_eq!(
            starting("telegram", loose, &["tg".into()], &entries),
            "'tg'",
            "a command given wins"
        );
    }
}

//! Words that say what's on the screen, as the process manager has them: a paragraph at
//! the top of each view, a hover on what isn't obvious, a man page to open in Reader.

/// A piece of an explanation: words, or a man page to open in Reader.
#[derive(Clone, Copy)]
pub enum Part {
    Text(&'static str),
    Man(&'static str),
}

/// An explanation at the size of the text around it, each man page a link that opens it
/// in Reader.
pub fn explained(ui: &mut egui::Ui, parts: &[Part]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for part in parts {
            match part {
                Part::Text(text) => {
                    ui.label(*text);
                }
                Part::Man(page) => {
                    if ui
                        .link(*page)
                        .on_hover_text(format!("Open the {page} man page in Reader"))
                        .clicked()
                    {
                        open_man(page);
                    }
                }
            }
        }
    });
}

fn open_man(page: &str) {
    let _ = std::process::Command::new("reader")
        .args(["--single", "--man", page])
        .spawn();
}

pub const WATCHING: &[Part] = &[
    Part::Text(
        "Every message on your session bus, as it passes. Programs on the desktop ask each other \
         to do things (calls), answer (replies, or errors), and announce things to whoever is \
         listening (signals). Dbug is a monitor: the bus hands it a copy of everything and it \
         can't change or answer any of it. A reply is matched to its call, with how long the \
         answer took. Names like :1.42 are what the bus called each program when it joined; the \
         bus itself is org.freedesktop.DBus. More in ",
    ),
    Part::Man("dbus-monitor(1)"),
    Part::Text(" and "),
    Part::Man("busctl(1)"),
    Part::Text("."),
];

pub const SENDING: &[Part] = &[
    Part::Text(
        "Send a message yourself, from a connection of Dbug's own. A call asks the program named \
         as its destination to do something, and its answer comes back below; careful, it does \
         what the program does with it. A signal goes to everyone listening, or to the one you \
         name. Fill in the arguments in the types the method expects. Resend… on any message in \
         the list fills this in from it, to change a value and try again. ",
    ),
    Part::Man("dbus-send(1)"),
    Part::Text(" does the same from a terminal."),
];

/// What a message from the bus itself or a standard interface means, if it's one of those.
pub fn meaning(interface: &str, member: &str) -> Option<&'static str> {
    Some(match (interface, member) {
        ("org.freedesktop.DBus", "Hello") => {
            "A program joining the bus, and asking for its :1.N name."
        }
        ("org.freedesktop.DBus", "AddMatch") => {
            "A program asking to be sent the signals that match a rule."
        }
        ("org.freedesktop.DBus", "RemoveMatch") => {
            "A program no longer wanting the signals a rule matched."
        }
        ("org.freedesktop.DBus", "RequestName") => {
            "A program asking to be known by a name, such as org.example.App."
        }
        ("org.freedesktop.DBus", "ReleaseName") => "A program giving up a name it held.",
        ("org.freedesktop.DBus", "NameHasOwner") => {
            "Asking whether a name is taken: whether that program is running."
        }
        ("org.freedesktop.DBus", "GetNameOwner") => "Asking which connection (:1.N) holds a name.",
        ("org.freedesktop.DBus", "ListNames") => "Asking for every name on the bus.",
        ("org.freedesktop.DBus", "ListActivatableNames") => {
            "Asking for the names the bus could start a program for."
        }
        ("org.freedesktop.DBus", "StartServiceByName") => {
            "Asking the bus to start the program that provides a name."
        }
        ("org.freedesktop.DBus", "GetConnectionUnixProcessID") => {
            "Asking which process a connection belongs to."
        }
        ("org.freedesktop.DBus", "GetConnectionCredentials") => {
            "Asking who a connection is: its process, user and more."
        }
        ("org.freedesktop.DBus", "NameAcquired") => {
            "The bus telling a program it now holds a name."
        }
        ("org.freedesktop.DBus", "NameLost") => {
            "The bus telling a program it no longer holds a name."
        }
        ("org.freedesktop.DBus", "NameOwnerChanged") => {
            "A name changed hands: a program started (it had no owner before), quit (no owner after) or took over. The arguments are the name, the old owner and the new."
        }
        ("org.freedesktop.DBus.Properties", "Get") => "Reading one property of an object.",
        ("org.freedesktop.DBus.Properties", "GetAll") => {
            "Reading every property of one interface of an object."
        }
        ("org.freedesktop.DBus.Properties", "Set") => "Changing a property of an object.",
        ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
            "An object announcing that some of its properties changed. The arguments are the interface, the new values and the properties that are now invalid."
        }
        ("org.freedesktop.DBus.Introspectable", "Introspect") => {
            "Asking an object to describe itself: its interfaces, methods, signals and properties, as XML."
        }
        ("org.freedesktop.DBus.Peer", "Ping") => "Asking whether a program is alive and answering.",
        ("org.freedesktop.DBus.Peer", "GetMachineId") => "Asking which machine a program runs on.",
        ("org.freedesktop.DBus.ObjectManager", "GetManagedObjects") => {
            "Asking for a program's objects and all their properties at once."
        }
        ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded") => {
            "A program announcing a new object."
        }
        ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved") => {
            "A program announcing that an object is gone."
        }
        _ => return None,
    })
}

/// What each kind of message is.
pub fn kind_meaning(kind: crate::monitor::Kind) -> &'static str {
    use crate::monitor::Kind;
    match kind {
        Kind::Call => "A call: one program asking another to do something or tell it something.",
        Kind::Reply => "A reply: the answer to a call, with whatever it returns.",
        Kind::Error => "An error: the answer to a call, when it couldn't be done.",
        Kind::Signal => "A signal: an announcement to whoever asked to hear it. Nobody answers it.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buss_own_messages_are_explained() {
        assert!(
            meaning("org.freedesktop.DBus", "NameOwnerChanged")
                .unwrap()
                .contains("changed hands")
        );
        assert!(meaning("org.example", "Frobnicate").is_none());
    }
}

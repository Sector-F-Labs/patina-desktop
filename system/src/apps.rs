//! Installed apps, from their `.desktop` entries, and a window's app from
//! its app id (a Wayland app id, or an X11 window's class): the bar's tab
//! icons (ADR-0050) and New Window (ADR-0055), and what patina-raise finds
//! and starts by name (ADR-0058), and whether it's a terminal (ADR-0067).

use freedesktop_desktop_entry::{DesktopEntry, Iter, default_paths};

/// What a `.desktop` entry says that matters here.
pub struct Entry {
    pub id: String,
    /// What it's called, as the launcher shows it ("Telegram").
    pub name: String,
    pub wm_class: Option<String>,
    pub icon: Option<String>,
    /// What opens a new window: the entry's `new-window` action if it has
    /// one (a browser or a file manager, which only raise their window when
    /// run again), else its Exec; in a terminal if the entry says so.
    pub command: Option<Vec<String>>,
    /// What starts it: its Exec, in a terminal if the entry says so.
    pub exec: Option<Vec<String>>,
    /// It's a terminal (`TerminalEmulator` in its Categories), so Super
    /// keys reach it with Shift as well as Ctrl (ADR-0067).
    pub terminal_emulator: bool,
    /// The types it opens, from its MimeType: what Settings' Defaults page
    /// offers it for (ADR-0079).
    pub mime_types: Vec<String>,
}

/// `args` run in a terminal, if `terminal`: `$TERMINAL`, or foot.
fn in_terminal(mut args: Vec<String>, terminal: bool) -> Vec<String> {
    if terminal {
        let program = std::env::var("TERMINAL").unwrap_or_else(|_| "foot".into());
        args.splice(0..0, [program, "-e".into()]);
    }
    args
}

fn command(e: &DesktopEntry) -> Option<Vec<String>> {
    let new_window = e
        .actions()
        .is_some_and(|actions| actions.contains(&"new-window"));
    let args = if new_window {
        e.parse_exec_action("new-window").ok()
    } else {
        None
    }
    .or_else(|| e.parse_exec().ok())
    .filter(|args| !args.is_empty())?;
    Some(in_terminal(args, e.terminal()))
}

fn exec(e: &DesktopEntry) -> Option<Vec<String>> {
    let args = e.parse_exec().ok().filter(|args| !args.is_empty())?;
    Some(in_terminal(args, e.terminal()))
}

fn terminal_emulator(e: &DesktopEntry) -> bool {
    e.categories()
        .is_some_and(|c| c.contains(&"TerminalEmulator"))
}

pub fn entries() -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    Iter::new(default_paths())
        .entries::<&str>(None)
        .filter(|e| seen.insert(e.id().to_string()))
        .map(|e| Entry {
            id: e.id().to_string(),
            name: e
                .name::<&str>(&[])
                .map(|n| n.to_string())
                .unwrap_or_default(),
            wm_class: e.startup_wm_class().map(str::to_string),
            icon: e.icon().map(str::to_string),
            command: command(&e),
            exec: exec(&e),
            terminal_emulator: terminal_emulator(&e),
            mime_types: e
                .mime_type()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect(),
        })
        .collect()
}

/// `app_id`'s entry: the one with that id, or whose StartupWMClass is it,
/// or whose id ends in it (`nautilus` for org.gnome.Nautilus), ignoring
/// case.
pub fn entry_for<'a>(entries: &'a [Entry], app_id: &str) -> Option<&'a Entry> {
    let last = |id: &str| id.rsplit('.').next().unwrap_or(id).to_lowercase();
    let wanted = app_id.to_lowercase();
    let by = |matches: &dyn Fn(&Entry) -> bool| entries.iter().find(|e| matches(e));
    by(&|e| e.id.eq_ignore_ascii_case(app_id))
        .or_else(|| {
            by(&|e| {
                e.wm_class
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(app_id))
            })
        })
        .or_else(|| by(&|e| last(&e.id) == wanted))
}

/// `args` as one line for sh(1), each quoted.
pub fn shell_line(args: &[String]) -> String {
    args.iter()
        .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command line, for sh(1), that opens a new window of `app_id`: from
/// its entry, or else the app id itself if a program of that name is
/// installed (`foot`).
pub fn new_window_command(entries: &[Entry], app_id: &str) -> Option<String> {
    if let Some(args) = entry_for(entries, app_id).and_then(|e| e.command.as_ref()) {
        return Some(shell_line(args));
    }
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(app_id).is_file()));
    (on_path && !app_id.contains('/')).then(|| shell_line(&[app_id.to_string()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, wm_class: Option<&str>, icon: &str) -> Entry {
        Entry {
            id: id.into(),
            wm_class: wm_class.map(Into::into),
            name: id.into(),
            icon: Some(icon.into()),
            command: None,
            exec: None,
            terminal_emulator: false,
            mime_types: Vec::new(),
        }
    }

    fn parsed(text: &str) -> DesktopEntry {
        DesktopEntry::from_str(
            std::path::Path::new("/x/app.desktop"),
            text,
            None::<&[&str]>,
        )
        .unwrap()
    }

    #[test]
    fn app_ids_find_their_entry() {
        let entries = [
            entry("org.gnome.Nautilus", None, "org.gnome.Nautilus"),
            entry("firefox", None, "firefox"),
            entry("steam", Some("Steam"), "steam"),
            entry("com.mitchellh.ghostty", None, "ghostty-icon"),
        ];
        let id = |app_id| entry_for(&entries, app_id).map(|e| e.id.as_str());
        assert_eq!(id("firefox"), Some("firefox"));
        assert_eq!(id("org.gnome.nautilus"), Some("org.gnome.Nautilus"));
        assert_eq!(id("Steam"), Some("steam"), "by its X11 class");
        assert_eq!(
            id("ghostty"),
            Some("com.mitchellh.ghostty"),
            "by the id's end"
        );
        assert_eq!(id("foot"), None);
    }

    #[test]
    fn a_new_window_action_wins_over_exec() {
        let firefox = parsed(
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\nActions=new-window;new-private-window;\n\n[Desktop Action new-window]\nName=New Window\nExec=firefox --new-window %u\n\n[Desktop Action new-private-window]\nName=New Private Window\nExec=firefox --private-window %u\n",
        );
        assert_eq!(command(&firefox).unwrap(), ["firefox", "--new-window"]);
        let foot = parsed("[Desktop Entry]\nType=Application\nName=Foot\nExec=foot %F\n");
        assert_eq!(command(&foot).unwrap(), ["foot"]);
    }

    #[test]
    fn terminals_say_so_in_their_categories() {
        let foot = "[Desktop Entry]\nType=Application\nName=Foot\nExec=foot\nCategories=System;TerminalEmulator;\n";
        assert!(terminal_emulator(&parsed(foot)));
        let htop = "[Desktop Entry]\nType=Application\nName=htop\nExec=htop\nTerminal=true\nCategories=System;Monitor;\n";
        assert!(
            !terminal_emulator(&parsed(htop)),
            "runs in a terminal, isn't one"
        );
    }

    #[test]
    fn terminal_apps_open_in_a_terminal() {
        let htop =
            parsed("[Desktop Entry]\nType=Application\nName=htop\nExec=htop\nTerminal=true\n");
        let args = command(&htop).unwrap();
        assert_eq!(&args[1..], ["-e", "htop"]);
    }

    #[test]
    fn the_command_is_quoted_for_sh() {
        let mut nautilus = entry("org.gnome.Nautilus", None, "x");
        nautilus.command = Some(vec!["nautilus".into(), "--new-window".into()]);
        let entries = [nautilus];
        assert_eq!(
            new_window_command(&entries, "org.gnome.Nautilus").unwrap(),
            "'nautilus' '--new-window'"
        );
        assert_eq!(shell_line(&["it's".into()]), r"'it'\''s'");
        assert_eq!(new_window_command(&entries, "no-such-app-anywhere"), None);
    }
}

/// What New Window runs for some app ids on this machine: `cargo test -p
/// patina-bar installed_commands -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore = "reads the installed apps"]
fn installed_commands() {
    let known = entries();
    for app_id in [
        "firefox",
        "org.gnome.Nautilus",
        "foot",
        "teleprinter",
        "steam",
        "code",
    ] {
        println!("{app_id}: {:?}", new_window_command(&known, app_id));
    }
}

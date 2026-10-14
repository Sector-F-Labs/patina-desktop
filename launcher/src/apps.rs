//! Installed apps, from the `.desktop` files in every XDG data directory
//! (`~/.local/share/applications`, `/usr/share/applications`, Flatpak's
//! exports and so on), and searching them (ADR-0010).

use std::path::Path;
use std::process::{Command, Stdio};

use freedesktop_desktop_entry::{DesktopEntry, Iter, default_paths, get_languages_from_env};

/// The name this desktop answers to in `OnlyShowIn` and `NotShowIn`.
const DESKTOP: &str = "Patina";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The entry's id, e.g. `org.codeberg.dnkl.foot`.
    pub id: String,
    pub name: String,
    /// "Web Browser", "Terminal": shown under the name, and searched.
    pub generic: Option<String>,
    pub comment: Option<String>,
    pub keywords: Vec<String>,
    /// The command line, with the spec's `%f`-style field codes removed.
    pub exec: Vec<String>,
    pub terminal: bool,
    /// The working directory to run it in.
    pub path: Option<String>,
    /// An icon name to look up in the theme, or a file path.
    pub icon: Option<String>,
}

/// Every app worth listing, sorted by name. Where two directories have an
/// entry with the same id, the first (the user's own) wins, as the spec
/// says.
pub fn load() -> Vec<App> {
    let locales = get_languages_from_env();
    let mut seen = std::collections::HashSet::new();
    let mut apps: Vec<App> = Iter::new(default_paths())
        .entries(Some(&locales))
        .filter(|e| seen.insert(e.id().to_string()))
        .filter_map(|e| from_entry(&e, &locales))
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

fn from_entry(e: &DesktopEntry, locales: &[String]) -> Option<App> {
    if e.type_() != Some("Application") || e.no_display() || e.hidden() {
        return None;
    }
    if e.only_show_in().is_some_and(|d| !d.contains(&DESKTOP)) {
        return None;
    }
    if e.not_show_in().is_some_and(|d| d.contains(&DESKTOP)) {
        return None;
    }
    if let Some(try_exec) = e.try_exec()
        && !installed(try_exec)
    {
        return None;
    }
    let exec = e.parse_exec().ok().filter(|args| !args.is_empty())?;
    Some(App {
        id: e.id().to_string(),
        name: e.name(locales)?.into_owned(),
        generic: e.generic_name(locales).map(|s| s.into_owned()),
        comment: e.comment(locales).map(|s| s.into_owned()),
        keywords: e
            .keywords(locales)
            .unwrap_or_default()
            .into_iter()
            .filter(|k| !k.is_empty())
            .map(|k| k.into_owned())
            .collect(),
        exec,
        terminal: e.terminal(),
        path: e.path().map(str::to_string),
        icon: e.icon().map(str::to_string),
    })
}

/// Whether `program` (a path, or a name on PATH) exists.
fn installed(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).exists();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(program).exists()))
}

/// How well `app` matches `query`, lower is better, or `None` if it
/// doesn't: the name starting with it, then a word of the name starting with
/// it, then the name containing it, then any of the other fields.
pub fn score(app: &App, query: &str) -> Option<u8> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let name = app.name.to_lowercase();
    if name.starts_with(&query) {
        return Some(0);
    }
    if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&query))
    {
        return Some(1);
    }
    if name.contains(&query) {
        return Some(2);
    }
    let program = app
        .exec
        .first()
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_lowercase());
    let others = app
        .generic
        .iter()
        .chain(&app.keywords)
        .chain(&app.comment)
        .map(|s| s.to_lowercase())
        .chain(program);
    for field in others {
        if field.contains(&query) {
            return Some(3);
        }
    }
    None
}

/// Where in `apps` the ones matching `query` are, best first, then in the
/// order given (by name).
pub fn search(apps: &[App], query: &str) -> Vec<usize> {
    let mut found: Vec<(u8, usize)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, a)| score(a, query).map(|s| (s, i)))
        .collect();
    found.sort_by_key(|(s, _)| *s);
    found.into_iter().map(|(_, i)| i).collect()
}

/// Starts `app`, detached from the launcher. Terminal apps run in the
/// terminal named by `$TERMINAL`, or foot.
pub fn launch(app: &App) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut args = app.exec.clone();
    if app.terminal {
        let terminal = std::env::var("TERMINAL").unwrap_or_else(|_| "foot".into());
        args.splice(0..0, [terminal, "-e".into()]);
    }
    let mut command = Command::new(&args[0]);
    command
        .args(&args[1..])
        .stdin(Stdio::null())
        // Its own process group, so it outlives the launcher.
        .process_group(0);
    if let Some(dir) = &app.path {
        command.current_dir(dir);
    }
    command.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> App {
        App {
            id: name.to_lowercase(),
            name: name.into(),
            generic: None,
            comment: None,
            keywords: Vec::new(),
            exec: vec![name.to_lowercase()],
            terminal: false,
            path: None,
            icon: None,
        }
    }

    #[test]
    fn name_prefixes_beat_words_beat_substrings_beat_the_rest() {
        let mut browser = app("Firefox");
        browser.generic = Some("Web Browser".into());
        let apps = vec![
            app("Foot"),
            app("GNU Image Manipulation Program"),
            app("Tofu"),
            browser,
            app("Image Viewer"),
        ];
        let names = |q| -> Vec<String> {
            search(&apps, q)
                .iter()
                .map(|&i| apps[i].name.clone())
                .collect()
        };
        assert_eq!(
            names("im"),
            ["Image Viewer", "GNU Image Manipulation Program"]
        );
        assert_eq!(names("fo"), ["Foot", "Firefox"]);
        assert_eq!(names("WEB"), ["Firefox"]);
        assert!(names("zzz").is_empty());
        assert_eq!(names("  ").len(), apps.len());
    }

    #[test]
    fn keywords_and_the_program_match() {
        let mut a = app("Foot");
        a.keywords = vec!["shell".into()];
        a.exec = vec!["/usr/bin/footclient".into()];
        assert_eq!(score(&a, "shell"), Some(3));
        assert_eq!(score(&a, "footcl"), Some(3));
        assert_eq!(score(&a, "usr"), None);
    }

    #[test]
    fn reads_entries_and_skips_hidden_ones() {
        let entry = |text: &str| {
            let e =
                DesktopEntry::from_str("/tmp/x.desktop", text, Some(&["en".to_string()])).unwrap();
            from_entry(&e, &["en".to_string()])
        };
        let foot = entry(
            "[Desktop Entry]\nType=Application\nName=Foot\nGenericName=Terminal\nExec=foot %F\nIcon=foot\nKeywords=shell;prompt;\n",
        )
        .expect("listed");
        assert_eq!(foot.name, "Foot");
        assert_eq!(foot.exec, ["foot"]);
        assert_eq!(foot.keywords, ["shell", "prompt"]);
        assert_eq!(foot.icon.as_deref(), Some("foot"));
        assert!(
            entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n").is_none()
        );
        assert!(entry("[Desktop Entry]\nType=Link\nName=X\nURL=https://x\n").is_none());
        assert!(
            entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\nOnlyShowIn=GNOME;\n")
                .is_none()
        );
        assert!(
            entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNotShowIn=Patina;\n")
                .is_none()
        );
        assert!(
            entry(
                "[Desktop Entry]\nType=Application\nName=X\nExec=x\nTryExec=no-such-program-here\n"
            )
            .is_none()
        );
    }
}

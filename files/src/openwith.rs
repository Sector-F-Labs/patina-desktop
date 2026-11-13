//! Open With: the apps that handle a file's type, for the context menu.
//! They come from the `mimeinfo.cache` next to the desktop entries and the
//! mimeapps lists' added and removed associations, the type's defaults first.
//! The type's parents count too (a shell script is text), as GNOME's do.
//! Reading them is done on a thread, once for each type.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::editor;
use crate::mime::data_dirs;

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub id: String,
    /// The one the lists name first for the type.
    pub default: bool,
    pub name: String,
    exec: String,
    terminal: bool,
}

impl App {
    /// The command line that opens `path` with it.
    pub fn command(&self, path: &Path) -> Vec<String> {
        let line = editor::expand(&self.exec, path);
        if self.terminal { editor::in_terminal(line) } else { line }
    }
}

/// The apps found for each type, asked for as the menu needs them.
pub struct Finder {
    send: Sender<(String, Vec<App>)>,
    done: Receiver<(String, Vec<App>)>,
    found: HashMap<String, Vec<App>>,
    asked: HashSet<String>,
}

impl Finder {
    pub fn new() -> Self {
        let (send, done) = channel();
        Finder { send, done, found: HashMap::new(), asked: HashSet::new() }
    }

    /// The apps for `mime`, or `None` while they're being read.
    pub fn get(&mut self, mime: &str, ctx: &egui::Context) -> Option<&[App]> {
        for (mime, apps) in self.done.try_iter() {
            self.found.insert(mime, apps);
        }
        if !self.found.contains_key(mime) && self.asked.insert(mime.to_string()) {
            let (send, ctx, mime) = (self.send.clone(), ctx.clone(), mime.to_string());
            std::thread::spawn(move || {
                let apps = find(&mime);
                let _ = send.send((mime, apps));
                ctx.request_repaint();
            });
        }
        self.found.get(mime).map(Vec::as_slice)
    }

    /// Every app, or `None` while they're being read.
    pub fn all(&mut self, ctx: &egui::Context) -> Option<&[App]> {
        self.get(ALL, ctx)
    }

    /// Reads `mime`'s apps again the next time they're asked for.
    pub fn forget(&mut self, mime: &str) {
        self.found.remove(mime);
        self.asked.remove(mime);
    }
}

/// Makes `id` the app for `mime` in the user's `mimeapps.list`. A
/// desktop-specific list is read first, so a type it sets can't be changed here.
pub fn set_default(mime: &str, id: &str) -> Result<(), String> {
    let dir = dirs::config_dir().ok_or("there's no config folder")?;
    let desktops = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    for desktop in desktops.split(':').filter(|d| !d.is_empty()) {
        let text = std::fs::read_to_string(dir.join(format!("{desktop}-mimeapps.list"))).unwrap_or_default();
        if !editor::defaults(&text, mime).is_empty() {
            return Err(format!("{desktop}-mimeapps.list sets it, and is written from your Patina defaults"));
        }
    }
    let path = dir.join("mimeapps.list");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, with_default(&text, mime, id)).map_err(|e| format!("{}: {e}", path.display()))
}

/// `text`, a mimeapps list, with `id` first among `mime`'s default apps and
/// everything else as it was.
fn with_default(text: &str, mime: &str, id: &str) -> String {
    const SECTION: &str = "[Default Applications]";
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some(header) = lines.iter().position(|l| l.trim() == SECTION) else {
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        return format!("{out}{SECTION}\n{mime}={id};\n");
    };
    let end =
        lines[header + 1..].iter().position(|l| l.trim().starts_with('[')).map_or(lines.len(), |n| header + 1 + n);
    let existing = (header + 1..end).find(|&n| lines[n].split_once('=').is_some_and(|(k, _)| k.trim() == mime));
    match existing {
        Some(n) => {
            let kept: Vec<String> = lines[n]
                .split_once('=')
                .map(|(_, v)| v)
                .unwrap_or_default()
                .split(';')
                .map(str::trim)
                .filter(|o| !o.is_empty() && *o != id)
                .map(str::to_string)
                .collect();
            let rest: String = kept.iter().map(|o| format!("{o};")).collect();
            lines[n] = format!("{mime}={id};{rest}");
        }
        None => lines.insert(header + 1, format!("{mime}={id};")),
    }
    lines.join("\n") + "\n"
}

/// The key that asks for every app there is, for Other Application.
const ALL: &str = "*";

fn find(mime: &str) -> Vec<App> {
    if mime == ALL {
        return all();
    }
    let lists: Vec<String> = editor::lists().iter().filter_map(|l| std::fs::read_to_string(l).ok()).collect();
    let caches: Vec<String> = data_dirs()
        .into_iter()
        .filter_map(|d| std::fs::read_to_string(d.join("applications/mimeinfo.cache")).ok())
        .collect();
    let removed = |ty: &str| -> HashSet<String> {
        lists.iter().flat_map(|t| editor::associations(t, "[Removed Associations]", ty)).collect()
    };
    let mut ids: Vec<String> = Vec::new();
    let add = |ids: &mut Vec<String>, id: String| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    let types = types_of(mime);
    // The defaults of the type, then of what it's a kind of.
    for ty in &types {
        for text in &lists {
            for id in editor::defaults(text, ty) {
                add(&mut ids, id);
            }
        }
    }
    let defaults = ids.len();
    // Then each type's apps by name, the type's own before its parents'.
    for ty in &types {
        let gone = removed(ty);
        let mut group: Vec<String> = Vec::new();
        for text in &lists {
            for id in editor::associations(text, "[Added Associations]", ty) {
                add(&mut group, id);
            }
        }
        for text in &caches {
            for id in editor::associations(text, "[MIME Cache]", ty) {
                add(&mut group, id);
            }
        }
        let mut apps: Vec<App> =
            group.into_iter().filter(|id| !gone.contains(id) && !ids.contains(id)).filter_map(|id| load(&id)).collect();
        apps.sort_by_key(|a| a.name.to_lowercase());
        ids.extend(apps.iter().map(|a| a.id.clone()));
        ids.dedup();
    }
    ids.into_iter()
        .enumerate()
        .filter_map(|(n, id)| {
            let mut app = load(&id)?;
            app.default = n == 0 && defaults > 0;
            Some(app)
        })
        .collect()
}

fn load(id: &str) -> Option<App> {
    read(id, &std::fs::read_to_string(editor::find_entry(id)?).ok()?)
}

/// Every app with a desktop entry that's shown, by name.
fn all() -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps: Vec<App> = Vec::new();
    for dir in data_dirs() {
        let Ok(read_dir) = std::fs::read_dir(dir.join("applications")) else { continue };
        for entry in read_dir.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if id.ends_with(".desktop")
                && seen.insert(id.clone())
                && let Some(app) = load(&id)
            {
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// `mime` and what it's a kind of, nearest first, as the shared MIME database
/// has it: its real name behind an alias, its parent types, and text for any
/// text.
fn types_of(mime: &str) -> Vec<String> {
    let read = |file: &str| -> Vec<(String, String)> {
        data_dirs()
            .into_iter()
            .filter_map(|d| std::fs::read_to_string(d.join("mime").join(file)).ok())
            .flat_map(|t| {
                t.lines()
                    .filter_map(|l| l.split_once(' '))
                    .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    chain(mime, &read("aliases"), &read("subclasses"))
}

fn chain(mime: &str, aliases: &[(String, String)], subclasses: &[(String, String)]) -> Vec<String> {
    let first = aliases.iter().find(|(alias, _)| alias == mime).map_or(mime, |(_, real)| real.as_str());
    let mut types = vec![first.to_string()];
    let mut at = 0;
    while at < types.len() {
        for (child, parent) in subclasses {
            if *child == types[at] && !types.contains(parent) {
                types.push(parent.clone());
            }
        }
        at += 1;
    }
    if types[0].starts_with("text/") && !types.iter().any(|t| t == "text/plain") {
        types.push("text/plain".into());
    }
    types
}

/// The app a desktop entry describes, unless it's hidden or can't open a file.
fn read(id: &str, text: &str) -> Option<App> {
    let mut in_entry = false;
    let (mut name, mut exec, mut terminal, mut hidden) = (None, None, false, false);
    let (mut only, mut not, mut try_exec) = (Vec::new(), Vec::new(), None);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            match key.trim() {
                "Name" => name = Some(value.to_string()),
                "Exec" => exec = Some(value.to_string()),
                "Terminal" => terminal = value == "true",
                "NoDisplay" | "Hidden" => hidden |= value == "true",
                "OnlyShowIn" => only = list(value),
                "NotShowIn" => not = list(value),
                "TryExec" => try_exec = Some(value.to_string()),
                _ => {}
            }
        }
    }
    let desktops: Vec<String> = list(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().replace(':', ";"));
    let here = |names: &[String]| names.iter().any(|n| desktops.iter().any(|d| d.eq_ignore_ascii_case(n)));
    if hidden || (!only.is_empty() && !here(&only)) || here(&not) {
        return None;
    }
    if let Some(program) = try_exec.filter(|p| !p.is_empty()) {
        let found = if program.starts_with('/') {
            Path::new(&program).exists()
        } else {
            std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(&program).exists())
        };
        if !found {
            return None;
        }
    }
    Some(App { id: id.to_string(), default: false, name: name?, exec: exec?, terminal })
}

fn list(value: &str) -> Vec<String> {
    value.split(';').map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_entry_becomes_an_app() {
        let text = "[Desktop Entry]\nName=Viewer\nName[sv]=Visare\nExec=viewer %U\n\n[Desktop Action x]\nName=Other\n";
        let app = read("viewer.desktop", text).unwrap();
        assert_eq!((app.name.as_str(), app.terminal), ("Viewer", false));
        assert_eq!(app.command(Path::new("/a b.png")), ["viewer", "/a b.png"]);
    }

    #[test]
    fn a_default_is_put_first_and_the_rest_kept() {
        let list = "[Default Applications]\ntext/html=a.desktop;b.desktop;\nimage/png=v.desktop\n\n[Added Associations]\nx/y=z.desktop;\n";
        let out = with_default(list, "text/html", "b.desktop");
        assert_eq!(editor::defaults(&out, "text/html"), ["b.desktop", "a.desktop"]);
        assert_eq!(editor::defaults(&out, "image/png"), ["v.desktop"]);
        assert_eq!(editor::associations(&out, "[Added Associations]", "x/y"), ["z.desktop"]);
        let out = with_default(list, "text/csv", "c.desktop");
        assert_eq!(editor::defaults(&out, "text/csv"), ["c.desktop"]);
        assert_eq!(editor::defaults(&out, "text/html"), ["a.desktop", "b.desktop"]);
        let out = with_default("[Added Associations]\nx/y=z.desktop;\n", "text/csv", "c.desktop");
        assert_eq!(editor::defaults(&out, "text/csv"), ["c.desktop"]);
        assert_eq!(editor::defaults(&with_default("", "a/b", "c.desktop"), "a/b"), ["c.desktop"]);
    }

    #[test]
    fn a_type_is_followed_by_what_it_is_a_kind_of() {
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        let aliases = [pair("application/x-sh", "application/x-shellscript")];
        let subclasses = [pair("application/x-shellscript", "text/plain"), pair("text/x-c", "text/plain")];
        assert_eq!(chain("application/x-sh", &aliases, &subclasses), ["application/x-shellscript", "text/plain"]);
        assert_eq!(chain("text/x-log", &aliases, &subclasses), ["text/x-log", "text/plain"]);
        assert_eq!(chain("image/png", &aliases, &subclasses), ["image/png"]);
    }

    #[test]
    fn entries_for_other_desktops_are_left_out() {
        assert!(read("x.desktop", "[Desktop Entry]\nName=X\nExec=x\nOnlyShowIn=NoSuchDesktop;\n").is_none());
        assert!(read("x.desktop", "[Desktop Entry]\nName=X\nExec=x\nTryExec=/no/such/program\n").is_none());
    }

    #[test]
    fn hidden_entries_are_left_out() {
        assert!(read("x.desktop", "[Desktop Entry]\nName=X\nExec=x\nNoDisplay=true\n").is_none());
    }
}

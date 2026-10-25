//! What's open, kept across restarts (ADR-0004): the tabs in
//! `~/.local/share/text-editor/session.toml`, and each tab's unsaved text
//! as a draft in `drafts/`, written a moment after typing stops. Quitting
//! never asks about unsaved changes: they're there next time.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{APP, write_toml};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    /// The tab that was in front.
    pub tab: usize,
}

/// A tab as it's remembered.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTab {
    /// Names its draft.
    pub id: u64,
    /// The file, or none for one never saved.
    pub path: Option<PathBuf>,
    /// Untitled tabs' numbers, so Untitled 2 stays Untitled 2.
    pub untitled: usize,
    /// Its text is in its draft, not (only) on disk.
    pub draft: bool,
    /// A language chosen by hand, rather than found from the file.
    pub language: Option<String>,
    /// Where the cursor was, in characters.
    pub cursor: usize,
}

fn dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(APP))
}

fn drafts() -> Option<PathBuf> {
    Some(dir()?.join("drafts"))
}

fn draft_path(id: u64) -> Option<PathBuf> {
    Some(drafts()?.join(format!("{id:016x}.txt")))
}

impl Session {
    pub fn load() -> Self {
        let Some(text) = dir().and_then(|d| std::fs::read_to_string(d.join("session.toml")).ok()) else {
            return Session::default();
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("text-editor: session.toml: {e}");
            Session::default()
        })
    }

    pub fn save(&self) {
        let Some(dir) = dir() else { return };
        if let Err(e) = write_toml(&dir.join("session.toml"), self) {
            eprintln!("text-editor: can't save the session: {e}");
        }
    }

    /// Removes drafts no tab names: left by a crash between writing a
    /// draft and the session, or by a tab closed elsewhere.
    pub fn sweep_drafts(&self) {
        let Some(Ok(entries)) = drafts().map(std::fs::read_dir) else { return };
        let kept: Vec<PathBuf> = self.tabs.iter().filter(|t| t.draft).filter_map(|t| draft_path(t.id)).collect();
        for entry in entries.flatten() {
            if !kept.contains(&entry.path()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

pub fn read_draft(id: u64) -> Option<String> {
    std::fs::read_to_string(draft_path(id)?).ok()
}

pub fn write_draft(id: u64, text: &str) -> Result<(), String> {
    let path = draft_path(id).ok_or("there's no data folder")?;
    std::fs::create_dir_all(path.parent().unwrap_or(&path)).map_err(|e| e.to_string())?;
    crate::files::write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

pub fn remove_draft(id: u64) {
    if let Some(path) = draft_path(id) {
        let _ = std::fs::remove_file(path);
    }
}

/// A new tab's id: unique enough, from the clock and a counter.
pub fn new_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    nanos.wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let session = Session {
            tabs: vec![
                SavedTab { id: 7, path: Some("/a.rs".into()), draft: true, cursor: 3, ..SavedTab::default() },
                SavedTab { id: 8, untitled: 2, language: Some("Rust".into()), ..SavedTab::default() },
            ],
            tab: 1,
        };
        let text = toml::to_string(&session).unwrap();
        assert_eq!(toml::from_str::<Session>(&text).unwrap(), session);
        assert_ne!(new_id(), new_id());
    }
}

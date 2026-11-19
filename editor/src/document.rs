//! A tab's document: its text, the file it belongs to, whether it's been
//! changed since it was saved, and how it's shown.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::files::{self, Ending, Loaded};
use patina::highlight::{self, Highlighter};
use crate::indent::{self, Indent};

/// The file changed on disk while the tab had changes of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnDisk {
    Changed,
    Deleted,
}

pub struct Document {
    /// Names its draft, and its widgets.
    pub id: u64,
    pub path: Option<PathBuf>,
    /// For an untitled tab: Untitled, Untitled 2, ...
    pub untitled: usize,
    pub text: String,
    /// The fingerprint of the text as last read or saved; none for text
    /// that has never been in a file.
    saved: Option<u64>,
    pub modified: bool,
    /// Counts edits, so what depends on the text knows to look again.
    pub version: u64,
    pub ending: Ending,
    /// The line ends were changed by hand since the last save.
    ending_changed: bool,
    pub bom: bool,
    /// Why it can't be edited, if it can't.
    pub read_only: Option<String>,
    /// Being read from disk.
    pub loading: bool,
    /// A language chosen by hand, kept over what the file's name says.
    pub chosen_language: Option<String>,
    /// The language its name or first line says.
    detected: Option<String>,
    pub indent: Indent,
    pub highlighter: Highlighter,
    pub on_disk: Option<OnDisk>,
    /// Changed since its draft was written, and when.
    pub draft_due: Option<Instant>,
    /// Its text is in a draft file.
    pub has_draft: bool,
    /// Found text, as byte ranges, and the current match.
    pub found: Vec<Range<usize>>,
    pub current: Option<usize>,
    /// The laid-out text, kept until something it shows changes.
    pub galley: Option<(u64, Arc<egui::Galley>)>,
    /// The emoji in that galley, laid out invisibly to be painted in colour.
    pub emoji: Vec<patina::emoji::Placed>,
    /// A cursor to put the editor at, in characters, once it's shown.
    pub place_cursor: Option<Range<usize>>,
}

impl Document {
    pub fn new(untitled: usize, indent: Indent) -> Self {
        Document {
            id: crate::session::new_id(),
            path: None,
            untitled,
            text: String::new(),
            saved: None,
            modified: false,
            version: 0,
            ending: Ending::Lf,
            ending_changed: false,
            bom: false,
            read_only: None,
            loading: false,
            chosen_language: None,
            detected: None,
            indent,
            highlighter: Highlighter::new(None, true),
            on_disk: None,
            draft_due: None,
            has_draft: false,
            found: Vec::new(),
            current: None,
            galley: None,
            emoji: Vec::new(),
            place_cursor: None,
        }
    }

    /// A file, to be read.
    pub fn file(path: PathBuf, indent: Indent) -> Self {
        let mut doc = Document::new(0, indent);
        doc.path = Some(path);
        doc.loading = true;
        doc.place_cursor = Some(0..0);
        doc
    }

    pub fn name(&self) -> String {
        match &self.path {
            Some(path) => {
                path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
            }
            None if self.untitled > 1 => format!("Untitled {}", self.untitled),
            None => "Untitled".into(),
        }
    }

    /// The tab's title: its name, marked while it has unsaved changes.
    pub fn title(&self) -> String {
        if self.modified { format!("• {}", self.name()) } else { self.name() }
    }

    /// An untitled tab with nothing in it, which opening a file replaces.
    pub fn is_blank(&self) -> bool {
        self.path.is_none() && self.text.is_empty() && !self.loading
    }

    pub fn language(&self) -> Option<String> {
        self.chosen_language.clone().or_else(|| self.detected.clone())
    }

    fn detect(&mut self) {
        let first = &self.text[..self.text.find('\n').unwrap_or(self.text.len())];
        self.detected = highlight::detect(self.path.as_deref().and_then(Path::to_str), first);
    }

    /// The text was edited.
    pub fn edited(&mut self) {
        if self.path.is_none() {
            // A #! line typed into a new file.
            self.detect();
        }
        self.modified = self.ending_changed
            || match self.saved {
                Some(saved) => files::digest(&self.text) != saved,
                None => !self.text.is_empty(),
            };
        self.version += 1;
        self.draft_due = Some(Instant::now());
    }

    /// The file as read from disk. A tab restored from a draft keeps its
    /// text; the file only says what was saved.
    pub fn loaded(&mut self, loaded: Loaded, default_indent: Indent) {
        let draft = self.has_draft && self.loading;
        self.loading = false;
        self.saved = Some(files::digest(&loaded.text));
        (self.ending, self.bom) = (loaded.ending, loaded.bom);
        self.read_only = loaded.lossy.then(|| "It isn't UTF-8 text, so it's open read-only, to keep it whole.".into());
        if !draft {
            self.text = loaded.text;
            self.indent = indent::detect(&self.text).unwrap_or(default_indent);
        }
        self.detect();
        self.edited();
        if !self.modified {
            self.draft_due = None;
        }
    }

    /// The file couldn't be read: a new file, if it isn't there.
    pub fn load_failed(&mut self) {
        self.loading = false;
        self.saved = None;
        self.edited();
        if !self.modified {
            self.draft_due = None;
        }
    }

    /// Text restored from a draft.
    pub fn restored(&mut self, text: String, default_indent: Indent) {
        self.indent = indent::detect(&text).unwrap_or(default_indent);
        self.text = text;
        self.has_draft = true;
        self.detect();
    }

    /// The text with fingerprint `digest` was written to `path`.
    pub fn saved_as(&mut self, path: PathBuf, digest: u64) {
        self.path = Some(path);
        self.detect();
        self.saved = Some(digest);
        self.on_disk = None;
        self.ending_changed = false;
        self.modified = files::digest(&self.text) != digest;
    }

    /// Saves with other line ends from now on.
    pub fn set_ending(&mut self, ending: Ending) {
        if ending != self.ending {
            self.ending = ending;
            self.ending_changed = true;
            self.edited();
        }
    }

    /// The file went from disk: what's here is all there is.
    pub fn deleted(&mut self) {
        self.on_disk = Some(OnDisk::Deleted);
        self.saved = None;
        self.edited();
    }

    /// Whether `text` read from disk is what this tab last saved or read.
    pub fn is_saved(&self, text: &str) -> bool {
        self.saved == Some(files::digest(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(text: &str) -> Loaded {
        files::decode(text.as_bytes().to_vec())
    }

    #[test]
    fn modified_until_it_matches_what_was_saved() {
        let mut doc = Document::file("/a.txt".into(), Indent::default());
        doc.loaded(loaded("one\n"), Indent::default());
        assert!(!doc.modified && doc.title() == "a.txt");
        doc.text.push('x');
        doc.edited();
        assert!(doc.modified && doc.title() == "• a.txt");
        doc.text.pop();
        doc.edited();
        assert!(!doc.modified);
    }

    #[test]
    fn a_draft_keeps_its_text_over_the_file() {
        let mut doc = Document::file("/a.txt".into(), Indent::default());
        doc.restored("draft\n".into(), Indent::default());
        doc.loaded(loaded("on disk\n"), Indent::default());
        assert_eq!(doc.text, "draft\n");
        assert!(doc.modified);
        assert!(doc.is_saved("on disk\n"));
    }

    #[test]
    fn untitled_tabs_are_numbered_and_modified_once_typed_in() {
        let mut doc = Document::new(2, Indent::default());
        assert!(doc.is_blank() && doc.name() == "Untitled 2");
        doc.text.push('a');
        doc.edited();
        assert!(doc.modified && !doc.is_blank());
        doc.saved_as("/b.txt".into(), files::digest("a"));
        assert!(!doc.modified && doc.name() == "b.txt");
    }

    #[test]
    fn text_that_isnt_utf8_is_read_only() {
        let mut doc = Document::file("/a.bin".into(), Indent::default());
        doc.loaded(files::decode(b"\xff\xfe".to_vec()), Indent::default());
        assert!(doc.read_only.is_some());
    }
}

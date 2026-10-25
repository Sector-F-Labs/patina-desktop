//! Watching the open files, so a file changed by something else is read
//! again (ADR-0006). Their folders are watched, not the files, which
//! catches programs that save by replacing the file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How long things stay quiet before changes are taken in, so a save in
/// several writes is read once, whole.
const SETTLE: Duration = Duration::from_millis(150);

pub struct Watch {
    watcher: RecommendedWatcher,
    events: Receiver<notify::Result<notify::Event>>,
    watched: HashSet<PathBuf>,
    pending: HashSet<PathBuf>,
    /// The watcher lost track: look at every file.
    everything: bool,
    last: Option<Instant>,
}

/// What changed since the last look.
pub enum Changes {
    Paths(HashSet<PathBuf>),
    Everything,
}

impl Changes {
    pub fn touches(&self, path: &Path) -> bool {
        match self {
            Changes::Paths(paths) => paths.contains(path),
            Changes::Everything => true,
        }
    }
}

impl Watch {
    /// A watcher that wakes the window when something changes.
    pub fn new(ctx: &egui::Context) -> notify::Result<Self> {
        let (send, events) = channel();
        let ctx = ctx.clone();
        let watcher = notify::recommended_watcher(move |event| {
            if send.send(event).is_ok() {
                ctx.request_repaint_after(SETTLE);
            }
        })?;
        Ok(Watch { watcher, events, watched: HashSet::new(), pending: HashSet::new(), everything: false, last: None })
    }

    /// Watches the folders of exactly these files.
    pub fn set<'a>(&mut self, files: impl IntoIterator<Item = &'a Path>) {
        let wanted: HashSet<&Path> = files.into_iter().filter_map(Path::parent).collect();
        self.watched.retain(|dir| {
            let keep = wanted.contains(dir.as_path());
            if !keep {
                let _ = self.watcher.unwatch(dir);
            }
            keep
        });
        for dir in wanted {
            if !self.watched.contains(dir) && self.watcher.watch(dir, RecursiveMode::NonRecursive).is_ok() {
                self.watched.insert(dir.to_path_buf());
            }
        }
    }

    /// What changed, once things have been quiet for a moment; until then
    /// nothing, with the window asked to look again.
    pub fn changes(&mut self, ctx: &egui::Context) -> Option<Changes> {
        for event in self.events.try_iter() {
            self.last = Some(Instant::now());
            let Ok(event) = event else {
                self.everything = true;
                continue;
            };
            self.everything |= event.need_rescan();
            // Reading a file is no change to it.
            if let EventKind::Access(kind) = event.kind
                && kind != AccessKind::Close(AccessMode::Write)
            {
                continue;
            }
            self.pending.extend(event.paths);
        }
        let quiet = self.last?.elapsed();
        if quiet < SETTLE {
            ctx.request_repaint_after(SETTLE - quiet);
            return None;
        }
        self.last = None;
        let paths = std::mem::take(&mut self.pending);
        if std::mem::take(&mut self.everything) {
            return Some(Changes::Everything);
        }
        (!paths.is_empty()).then_some(Changes::Paths(paths))
    }
}

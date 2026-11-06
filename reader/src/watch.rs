//! Watching the folders the tabs show, so a file saved, added or removed
//! elsewhere shows at once. Each folder is watched on its own, not
//! recursively, so build output and version control, which the tree
//! leaves out, cost no watches.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, ModifyKind};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How long the folders stay quiet before changes are taken in, so a
/// save in several writes is read once, whole.
const SETTLE: Duration = Duration::from_millis(120);

/// What changed since the last look.
#[derive(Default)]
pub struct Changes {
    /// Files written, created, removed or renamed.
    pub paths: HashSet<PathBuf>,
    /// Folders whose entries changed, so their trees need scanning again.
    pub folders: HashSet<PathBuf>,
    /// The watcher lost track (its queue overflowed): scan everything.
    pub everything: bool,
}

impl Changes {
    fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.folders.is_empty() && !self.everything
    }
}

pub struct Watch {
    watcher: RecommendedWatcher,
    events: Receiver<notify::Result<notify::Event>>,
    watched: HashSet<PathBuf>,
    pending: Changes,
    last: Option<Instant>,
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
        Ok(Watch { watcher, events, watched: HashSet::new(), pending: Changes::default(), last: None })
    }

    /// Watches exactly `folders`.
    pub fn set<'a>(&mut self, folders: impl IntoIterator<Item = &'a Path>) {
        let wanted: HashSet<&Path> = folders.into_iter().collect();
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
                self.pending.everything = true;
                continue;
            };
            if event.need_rescan() {
                self.pending.everything = true;
            }
            match event.kind {
                // Reader reading a file is no change to it.
                EventKind::Access(kind) if kind != AccessKind::Close(AccessMode::Write) => continue,
                EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) | EventKind::Access(_) => {}
                _ => self.pending.folders.extend(event.paths.iter().filter_map(|p| p.parent()).map(Path::to_path_buf)),
            }
            self.pending.paths.extend(event.paths);
        }
        let last = self.last?;
        let quiet = last.elapsed();
        if quiet < SETTLE {
            ctx.request_repaint_after(SETTLE - quiet);
            return None;
        }
        self.last = None;
        let changes = std::mem::take(&mut self.pending);
        (!changes.is_empty()).then_some(changes)
    }
}

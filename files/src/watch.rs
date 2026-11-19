//! Noticing changes to the folders on screen, so they re-read themselves:
//! inotify on Linux, kqueue on FreeBSD, through notify.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime};

use notify::{RecursiveMode, Watcher as _};

pub struct Watcher {
    watcher: Option<notify::RecommendedWatcher>,
    watching: HashSet<PathBuf>,
    changed: Receiver<PathBuf>,
    /// When the folders' own times were last looked at, and what they were: a way to notice what
    /// the watch didn't tell of.
    checked: Instant,
    seen: HashMap<PathBuf, Option<SystemTime>>,
    every: Duration,
}

/// How often the folders' times are looked at.
pub const CHECK_EVERY: Duration = Duration::from_secs(3);

impl Watcher {
    pub fn new(ctx: &egui::Context) -> Self {
        let (send, changed) = channel();
        let ctx = ctx.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            if matches!(event.kind, notify::EventKind::Access(_)) {
                return;
            }
            for path in event.paths {
                // The folder that changed: the path's parent, or the path
                // itself when it's the folder being watched.
                let _ = send.send(path.clone());
                if let Some(parent) = path.parent() {
                    let _ = send.send(parent.to_path_buf());
                }
            }
            ctx.request_repaint();
        })
        .map_err(|e| eprintln!("files: can't watch folders for changes: {e}"))
        .ok();
        Watcher { watcher, watching: HashSet::new(), changed, checked: Instant::now(), seen: HashMap::new(), every: CHECK_EVERY }
    }

    /// Watches exactly `folders`, adding and dropping watches to match.
    pub fn set(&mut self, folders: HashSet<PathBuf>) {
        let Some(watcher) = &mut self.watcher else { return };
        for gone in self.watching.difference(&folders) {
            let _ = watcher.unwatch(gone);
        }
        for new in folders.difference(&self.watching) {
            let _ = watcher.watch(new, RecursiveMode::NonRecursive);
        }
        self.watching = folders;
    }

    /// The watched folders to read again though the watch said nothing of them: all of them when
    /// `regained` (the window came back, and may have slept through changes it was never woken
    /// for), else those whose own modified time is not what it was, looked at every few seconds.
    /// A folder's time moves when something is added, removed or renamed in it.
    pub fn missed(&mut self, regained: bool) -> Vec<PathBuf> {
        if !regained && self.checked.elapsed() < self.every {
            return Vec::new();
        }
        self.checked = Instant::now();
        self.seen.retain(|folder, _| self.watching.contains(folder));
        let mut folders = Vec::new();
        for folder in &self.watching {
            let time = std::fs::metadata(folder).and_then(|m| m.modified()).ok();
            let before = self.seen.insert(folder.clone(), time);
            if regained || before.is_some_and(|before| before != time) {
                folders.push(folder.clone());
            }
        }
        folders
    }

    /// The watched folders that changed since last asked.
    pub fn changed(&self) -> HashSet<PathBuf> {
        self.changed.try_iter().filter(|p| self.watching.contains(p)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("files-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Whether `dir` is reported changed within a second of `change`.
    fn reported(watcher: &Watcher, dir: &PathBuf, change: impl FnOnce()) -> bool {
        let _ = watcher.changed();
        change();
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            if watcher.changed().contains(dir) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_change_the_watch_missed_is_found_by_the_folders_time_and_regaining_the_window_finds_all() {
        let dir = folder("missed");
        let mut watcher = Watcher::new(&egui::Context::default());
        // No watch to tell of it: the folder is only in the set.
        watcher.watching = [dir.clone()].into();
        watcher.every = Duration::ZERO;
        assert!(watcher.missed(false).is_empty(), "the first look only notes the time");
        assert!(watcher.missed(false).is_empty(), "nothing changed");
        // A folder's time has a granularity, so let it pass before the change.
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(dir.join("later.png"), "x").unwrap();
        assert_eq!(watcher.missed(false), vec![dir.clone()], "something was added");
        assert!(watcher.missed(false).is_empty(), "and it's said once");
        assert_eq!(watcher.missed(true), [dir], "the window coming back reads everything");
    }

    #[test]
    fn the_times_are_not_looked_at_more_often_than_asked() {
        let dir = folder("rare");
        let mut watcher = Watcher::new(&egui::Context::default());
        watcher.watching = [dir.clone()].into();
        watcher.every = Duration::from_secs(3600);
        assert!(watcher.missed(false).is_empty());
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(dir.join("a"), "x").unwrap();
        assert!(watcher.missed(false).is_empty(), "too soon to look");
    }

    #[test]
    fn a_file_made_in_a_watched_folder_is_noticed() {
        let dir = folder("made");
        let mut watcher = Watcher::new(&egui::Context::default());
        watcher.set([dir.clone()].into());
        assert!(reported(&watcher, &dir, || std::fs::write(dir.join("new.txt"), "hi").unwrap()));
    }

    #[test]
    fn a_file_saved_by_writing_beside_it_and_renaming_is_noticed() {
        let dir = folder("renamed");
        let mut watcher = Watcher::new(&egui::Context::default());
        watcher.set([dir.clone()].into());
        assert!(reported(&watcher, &dir, || {
            std::fs::write(dir.join(".new.png.tmp"), "pixels").unwrap();
            std::fs::rename(dir.join(".new.png.tmp"), dir.join("new.png")).unwrap();
        }));
    }

    #[test]
    fn a_folder_reached_through_a_link_is_noticed() {
        let real = folder("real");
        let link = std::env::temp_dir().join(format!("files-watch-{}-link", std::process::id()));
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut watcher = Watcher::new(&egui::Context::default());
        watcher.set([link.clone()].into());
        assert!(reported(&watcher, &link, || std::fs::write(real.join("via.txt"), "x").unwrap()));
    }
}

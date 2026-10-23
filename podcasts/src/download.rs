//! Episode downloads (ADR-0003). Every episode played is downloaded to
//! `~/.local/share/podcasts/episodes/`, and playback starts while the file
//! is still arriving: the player reads it through a `GrowingFile`, which
//! waits for bytes that haven't come yet.
//!
//! A download writes to `<name>.part` and renames it when complete, so a
//! file without `.part` is always whole.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::feed::Episode;

/// One download's progress, shared between its thread, the player and the UI.
pub struct Download {
    /// The file being written; readers use positional reads on clones of it.
    file: File,
    written: AtomicU64,
    /// From Content-Length; 0 while unknown.
    total: AtomicU64,
    done: AtomicBool,
    cancelled: AtomicBool,
    error: Mutex<Option<String>>,
}

impl Download {
    fn new(file: File) -> Self {
        Self {
            file,
            written: AtomicU64::new(0),
            total: AtomicU64::new(0),
            done: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }

    /// A file already on disk, for the player.
    pub fn complete(file: File) -> io::Result<Arc<Self>> {
        let len = file.metadata()?.len();
        let d = Self::new(file);
        d.written.store(len, Ordering::SeqCst);
        d.total.store(len, Ordering::SeqCst);
        d.done.store(true, Ordering::SeqCst);
        Ok(Arc::new(d))
    }

    pub fn total(&self) -> Option<u64> {
        Some(self.total.load(Ordering::SeqCst)).filter(|t| *t > 0)
    }

    /// From 0 to 1, when the size is known.
    pub fn fraction(&self) -> Option<f32> {
        self.total().map(|t| (self.written.load(Ordering::SeqCst) as f64 / t as f64).min(1.0) as f32)
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// A reader over the file that waits for bytes still to come. It gives
    /// up when `stop` is set, so a stalled download can't hang its reader.
    pub fn reader(self: &Arc<Self>, stop: Arc<AtomicBool>) -> io::Result<GrowingFile> {
        Ok(GrowingFile { file: self.file.try_clone()?, pos: 0, download: Arc::clone(self), stop })
    }

    fn fail(&self, e: String) {
        if let Ok(mut error) = self.error.lock() {
            *error = Some(e);
        }
    }
}

pub struct GrowingFile {
    file: File,
    pos: u64,
    download: Arc<Download>,
    stop: Arc<AtomicBool>,
}

impl GrowingFile {
    /// Waits until `until` returns true, or fails if the download or the reader stopped.
    fn wait(&self, until: impl Fn(&Download) -> bool) -> io::Result<()> {
        loop {
            if until(&self.download) {
                return Ok(());
            }
            if self.stop.load(Ordering::SeqCst) || self.download.cancelled.load(Ordering::SeqCst) {
                return Err(io::Error::other("stopped"));
            }
            if let Some(e) = self.download.error() {
                return Err(io::Error::other(e));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Read for GrowingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let pos = self.pos;
        self.wait(|d| pos < d.written.load(Ordering::SeqCst) || d.is_done())?;
        let available = self.download.written.load(Ordering::SeqCst).saturating_sub(pos);
        let n = (buf.len() as u64).min(available) as usize;
        if n == 0 {
            return Ok(0);
        }
        let n = self.file.read_at(&mut buf[..n], pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for GrowingFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => {
                // The end is known from Content-Length, or once the download is done.
                self.wait(|d| d.total().is_some() || d.is_done())?;
                let end = self.download.total().unwrap_or_else(|| self.download.written.load(Ordering::SeqCst));
                end as i64 + d
            }
        };
        if pos < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

// ---------------------------------------------------------------------------
// The downloads
// ---------------------------------------------------------------------------

pub fn dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("podcasts").join("episodes"))
}

/// Where an episode's audio is kept once downloaded.
pub fn path(episode: &Episode) -> Option<PathBuf> {
    Some(dir()?.join(format!("{:016x}.{}", fnv(&episode.id), episode.extension())))
}

fn part(path: &std::path::Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// A stable hash for file names (std's hasher may change between releases).
pub fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

pub struct Downloads {
    ctx: egui::Context,
    agent: ureq::Agent,
    /// By episode id.
    active: HashMap<String, Arc<Download>>,
}

impl Downloads {
    pub fn new(ctx: &egui::Context) -> Self {
        // No overall timeout: an hour of audio takes as long as it takes.
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .user_agent(concat!("podcasts/", env!("CARGO_PKG_VERSION"), " (Patina)"))
            .build()
            .into();
        Self { ctx: ctx.clone(), agent, active: HashMap::new() }
    }

    pub fn get(&self, id: &str) -> Option<&Arc<Download>> {
        self.active.get(id)
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// Whether the episode is on disk in full.
    pub fn is_downloaded(&self, episode: &Episode) -> bool {
        !self.active.contains_key(&episode.id) && path(episode).is_some_and(|p| p.exists())
    }

    /// The episode's audio for the player: what's on disk, or what's arriving.
    pub fn open(&mut self, episode: &Episode) -> Result<Arc<Download>, String> {
        if let Some(d) = self.active.get(&episode.id) {
            return Ok(Arc::clone(d));
        }
        let path = path(episode).ok_or("No data folder")?;
        if let Ok(file) = File::open(&path) {
            return Download::complete(file).map_err(|e| e.to_string());
        }
        self.start(episode)
    }

    /// Starts downloading, or returns the download already under way.
    pub fn start(&mut self, episode: &Episode) -> Result<Arc<Download>, String> {
        if let Some(d) = self.active.get(&episode.id) {
            return Ok(Arc::clone(d));
        }
        let path = path(episode).ok_or("No data folder")?;
        let part = part(&path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
        }
        // Read and write, so readers' clones of it can read.
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&part)
            .map_err(|e| format!("Couldn't write {}: {e}", part.display()))?;
        let mut writer = file.try_clone().map_err(|e| e.to_string())?;
        let download = Arc::new(Download::new(file));
        self.active.insert(episode.id.clone(), Arc::clone(&download));

        let (agent, ctx, url, shared) =
            (self.agent.clone(), self.ctx.clone(), episode.audio.clone(), Arc::clone(&download));
        std::thread::spawn(move || {
            let result = fetch(&agent, &url, &mut writer, &shared, &ctx);
            match result {
                Ok(()) => match std::fs::rename(&part, &path) {
                    Ok(()) => shared.done.store(true, Ordering::SeqCst),
                    Err(e) => shared.fail(format!("Couldn't keep the download: {e}")),
                },
                Err(e) => {
                    shared.fail(e);
                    let _ = std::fs::remove_file(&part);
                }
            }
            ctx.request_repaint();
        });
        Ok(download)
    }

    /// Finished and failed downloads, as (episode id, error), removed from the active ones.
    pub fn poll(&mut self) -> Vec<(String, Option<String>)> {
        let ended: Vec<String> =
            self.active.iter().filter(|(_, d)| d.is_done() || d.error().is_some()).map(|(id, _)| id.clone()).collect();
        ended.into_iter().filter_map(|id| self.active.remove(&id).map(|d| (id, d.error()))).collect()
    }

    pub fn cancel(&mut self, id: &str) {
        if let Some(d) = self.active.get(id) {
            d.cancel();
        }
    }

    pub fn delete(&mut self, episode: &Episode) {
        self.cancel(&episode.id);
        if let Some(path) = path(episode) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn fetch(
    agent: &ureq::Agent,
    url: &str,
    out: &mut File,
    download: &Download,
    ctx: &egui::Context,
) -> Result<(), String> {
    let mut resp = agent.get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(code) => format!("The server answered {code}"),
        e => format!("Couldn't download: {e}"),
    })?;
    if let Some(len) = resp.body().content_length() {
        download.total.store(len, Ordering::SeqCst);
    }
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut last_repaint = std::time::Instant::now();
    loop {
        if download.cancelled.load(Ordering::SeqCst) {
            return Err("Cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("Couldn't write the download: {e}"))?;
        download.written.fetch_add(n as u64, Ordering::SeqCst);
        // Keep the progress moving without a repaint for every packet.
        if last_repaint.elapsed() > Duration::from_millis(250) {
            last_repaint = std::time::Instant::now();
            ctx.request_repaint();
        }
    }
    let written = download.written.load(Ordering::SeqCst);
    download.total.store(written, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("podcasts-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn a_reader_waits_for_bytes_still_to_come() {
        let path = scratch("growing");
        let file = File::options().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        let mut writer = file.try_clone().unwrap();
        let download = Arc::new(Download::new(file));
        download.total.store(8, Ordering::SeqCst);
        let mut reader = download.reader(Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(reader.seek(SeekFrom::End(0)).unwrap(), 8);
        reader.seek(SeekFrom::Start(0)).unwrap();

        let shared = Arc::clone(&download);
        let t = std::thread::spawn(move || {
            for chunk in [b"abcd", b"efgh"] {
                std::thread::sleep(Duration::from_millis(30));
                writer.write_all(chunk).unwrap();
                shared.written.fetch_add(4, Ordering::SeqCst);
            }
            shared.done.store(true, Ordering::SeqCst);
        });
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        t.join().unwrap();
        assert_eq!(all, b"abcdefgh");
    }

    #[test]
    fn a_stopped_reader_gives_up() {
        let path = scratch("stalled");
        let file = File::options().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        let download = Arc::new(Download::new(file));
        let stop = Arc::new(AtomicBool::new(true));
        let mut reader = download.reader(stop).unwrap();
        assert!(reader.read(&mut [0u8; 4]).is_err());
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(fnv("a"), 0xaf63dc4c8601ec8c);
    }
}

//! Recording a voiceover from the microphone: ffmpeg reads the default input
//! (PulseAudio or PipeWire on Linux, OSS on FreeBSD) into a FLAC file while
//! the movie plays silently, so the voice can be timed to the picture.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub struct Recording {
    pub path: PathBuf,
    /// Where in the movie it began.
    pub at: f64,
    pub started: Instant,
    child: Child,
}

/// The microphone, as ffmpeg's input arguments.
fn microphone() -> [&'static str; 4] {
    if cfg!(target_os = "freebsd") { ["-f", "oss", "-i", "/dev/dsp"] } else { ["-f", "pulse", "-i", "default"] }
}

/// Where voiceovers go: `~/Videos/Montage Voiceovers/`, or under the data
/// folder when there's no Videos folder.
pub fn folder() -> Option<PathBuf> {
    dirs::video_dir()
        .map(|d| d.join("Montage Voiceovers"))
        .or_else(|| dirs::data_dir().map(|d| d.join("montage").join("voiceovers")))
}

/// The first `Voiceover N.flac` not taken in `dir`.
fn free_name(dir: &std::path::Path) -> PathBuf {
    (1..).map(|n| dir.join(format!("Voiceover {n}.flac"))).find(|p| !p.exists()).expect("a free name")
}

impl Recording {
    pub fn start(at: f64) -> Result<Self, String> {
        let dir = folder().ok_or("There's no folder to put the voiceover in")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
        let path = free_name(&dir);
        // Not render::ffmpeg: this one reads `q` on stdin to stop cleanly.
        let child = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error"])
            .args(microphone())
            .args(["-ac", "1", "-c:a", "flac"])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start recording: {e}"))?;
        Ok(Recording { path, at, started: Instant::now(), child })
    }

    /// Whether ffmpeg is still recording; it stops by itself when there's no microphone.
    pub fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Stops, and finishes the file in the background. What arrives is its
    /// path, if anything was recorded.
    pub fn stop(mut self, ctx: &egui::Context) -> Receiver<Option<PathBuf>> {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(stdin) = self.child.stdin.as_mut() {
                let _ = stdin.write_all(b"q");
            }
            drop(self.child.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(3);
            while matches!(self.child.try_wait(), Ok(None)) {
                if Instant::now() > deadline {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let recorded = std::fs::metadata(&self.path).is_ok_and(|m| m.len() > 0);
            if !recorded {
                let _ = std::fs::remove_file(&self.path);
            }
            let _ = tx.send(recorded.then(|| self.path.clone()));
            ctx.request_repaint();
        });
        rx
    }
}

impl Drop for Recording {
    /// Quitting mid-recording doesn't leave ffmpeg recording.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_count_up_past_taken_ones() {
        let dir = std::env::temp_dir().join(format!("montage-voice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_name(&dir), dir.join("Voiceover 1.flac"));
        std::fs::write(dir.join("Voiceover 1.flac"), b"x").unwrap();
        assert_eq!(free_name(&dir), dir.join("Voiceover 2.flac"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

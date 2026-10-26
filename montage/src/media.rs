//! Reading media files, off the UI thread: ffprobe for what's in a file,
//! ffmpeg for a thumbnail and for the sound's loudness over time, which the
//! timeline draws as a waveform (ADR-0002).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};

use serde::Deserialize;

use crate::project::{Info, MediaId};
use crate::render::{self, Size};

/// A thumbnail's size: 16:9, like the preview.
pub const THUMB: Size = Size { width: 160, height: 90 };

/// How many loudness values a second of sound gets.
pub const PEAKS_PER_SECOND: usize = 25;

/// The rate sound is read at for its loudness: enough to see speech and beats.
const PEAK_RATE: usize = 4000;

pub enum Update {
    /// What's in the file, and a thumbnail of its picture, if it has one.
    Probed { id: MediaId, info: Result<Info, String>, thumb: Option<egui::ColorImage> },
    /// Its sound's loudness, `PEAKS_PER_SECOND` values from 0 to 1 for every second.
    Peaks { id: MediaId, peaks: Vec<f32> },
    /// ffmpeg isn't installed, or doesn't run.
    Missing(String),
}

/// The worker that reads files, one at a time, in the order asked.
pub struct Reader {
    tx: Sender<(MediaId, PathBuf)>,
    rx: Receiver<Update>,
}

impl Reader {
    pub fn start(ctx: &egui::Context) -> Self {
        let (tx, jobs) = mpsc::channel::<(MediaId, PathBuf)>();
        let (send, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for tool in ["ffmpeg", "ffprobe"] {
                if Command::new(tool).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_err() {
                    let _ = send.send(Update::Missing(format!(
                        "Montage needs {tool}, which isn't installed: install ffmpeg from your packages"
                    )));
                    ctx.request_repaint();
                }
            }
            for (id, path) in jobs {
                let info = probe(&path);
                let thumb = match &info {
                    Ok(info) if info.size.is_some() => thumbnail(&path, info.duration),
                    _ => None,
                };
                let audio = info.as_ref().is_ok_and(|i| i.audio);
                if send.send(Update::Probed { id, info, thumb }).is_err() {
                    return;
                }
                ctx.request_repaint();
                if audio {
                    let _ = send.send(Update::Peaks { id, peaks: peaks(&path).unwrap_or_default() });
                    ctx.request_repaint();
                }
            }
        });
        Reader { tx, rx }
    }

    pub fn read(&self, id: MediaId, path: PathBuf) {
        let _ = self.tx.send((id, path));
    }

    pub fn poll(&self) -> Vec<Update> {
        self.rx.try_iter().collect()
    }
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<String>,
    #[serde(default)]
    disposition: Disposition,
}

#[derive(Deserialize, Default)]
struct Disposition {
    /// A music file's cover art, which isn't a picture to play.
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

/// What ffprobe's JSON says is in a file.
fn parse_probe(json: &[u8]) -> Result<Info, String> {
    let probe: Probe = serde_json::from_slice(json).map_err(|e| format!("ffprobe said something unexpected: {e}"))?;
    let video =
        probe.streams.iter().find(|s| s.codec_type.as_deref() == Some("video") && s.disposition.attached_pic == 0);
    let audio = probe.streams.iter().any(|s| s.codec_type.as_deref() == Some("audio"));
    let duration = probe
        .format
        .and_then(|f| f.duration)
        .or_else(|| probe.streams.iter().find_map(|s| s.duration.clone()))
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);
    if video.is_none() && !audio {
        return Err("has no picture or sound Montage can use".into());
    }
    // A photo reads as a video of one frame.
    if duration < 0.1 {
        return Err("is a still picture, and Montage can't use those yet".into());
    }
    Ok(Info { duration, size: video.and_then(|v| Some([v.width?, v.height?])), audio })
}

fn probe(path: &Path) -> Result<Info, String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
        .map_err(|e| format!("Couldn't run ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(render::reason(&out.stderr));
    }
    parse_probe(&out.stdout)
}

/// A frame from early in the file (a second in, or a tenth of the way for a
/// short one), letterboxed into `THUMB`.
fn thumbnail(path: &Path, duration: f64) -> Option<egui::ColorImage> {
    let at = (duration * 0.1).min(1.0);
    let Size { width: w, height: h } = THUMB;
    let out = render::ffmpeg()
        .args(["-ss", &format!("{at:.3}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf"])
        .arg(format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:-1:-1,setsar=1,format=rgba"))
        .args(["-f", "rawvideo", "-"])
        .output()
        .ok()?;
    (out.stdout.len() == (w * h * 4) as usize)
        .then(|| egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &out.stdout))
}

fn peaks(path: &Path) -> Option<Vec<f32>> {
    let out = render::ffmpeg()
        .arg("-i")
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", &PEAK_RATE.to_string(), "-f", "s16le", "-"])
        .output()
        .ok()?;
    let samples: Vec<i16> = out.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    Some(loudness(&samples, PEAK_RATE / PEAKS_PER_SECOND))
}

/// The loudest sample in each stretch of `per` samples, from 0 to 1.
fn loudness(samples: &[i16], per: usize) -> Vec<f32> {
    samples
        .chunks(per)
        .map(|c| c.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32 / i16::MAX as f32)
        .map(|v| v.min(1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_film_with_sound() {
        let json = br#"{"streams": [
            {"codec_type": "video", "width": 1920, "height": 1080, "disposition": {"attached_pic": 0}},
            {"codec_type": "audio"}
        ], "format": {"duration": "12.480000"}}"#;
        assert_eq!(parse_probe(json).unwrap(), Info { duration: 12.48, size: Some([1920, 1080]), audio: true });
    }

    #[test]
    fn a_song_with_cover_art_is_only_sound() {
        let json = br#"{"streams": [
            {"codec_type": "audio", "duration": "200.1"},
            {"codec_type": "video", "width": 600, "height": 600, "disposition": {"attached_pic": 1}}
        ], "format": {}}"#;
        assert_eq!(parse_probe(json).unwrap(), Info { duration: 200.1, size: None, audio: true });
    }

    #[test]
    fn photos_and_other_files_are_refused() {
        let photo = br#"{"streams": [{"codec_type": "video", "width": 4000, "height": 3000}], "format": {"duration": "0.040000"}}"#;
        assert!(parse_probe(photo).unwrap_err().contains("still"));
        assert!(parse_probe(br#"{"streams": [{"codec_type": "data"}], "format": {"duration": "3"}}"#).is_err());
        assert!(parse_probe(b"not json").is_err());
    }

    #[test]
    fn loudness_is_the_peak_of_each_stretch() {
        assert_eq!(loudness(&[0, 100, -32767, 5, 0], 2), vec![100.0 / 32767.0, 1.0, 0.0]);
    }
}

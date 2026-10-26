//! The movie as ffmpeg sees it: one filter graph for the whole timeline from
//! a given time on (ADR-0002). Exporting writes that graph to a file, and the
//! preview pipes the same graph's picture and sound back at a smaller size,
//! so what plays is what exports.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use crate::project::Project;

/// The sound's sample rate everywhere, in and out.
pub const RATE: u32 = 48_000;
/// The movie's frame rate.
pub const FPS: u32 = 30;

/// Every sound, from a file or from silence, in one format before it's joined or mixed.
const AFORMAT: &str = "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// The sizes a movie can be exported at, all 16:9 like the preview.
pub const SIZES: [(&str, Size); 3] = [
    ("480p", Size { width: 854, height: 480 }),
    ("720p", Size { width: 1280, height: 720 }),
    ("1080p", Size { width: 1920, height: 1080 }),
];

/// How the picture comes out of the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pixels {
    /// For an encoder.
    Yuv420,
    /// For the preview's textures.
    Rgba,
}

fn seconds(t: f64) -> String {
    format!("{:.3}", t.max(0.0))
}

/// Reads `length` seconds of a file from `from`, seeking before decoding.
fn input(args: &mut Vec<OsString>, path: &Path, from: f64, length: f64) {
    args.extend(["-ss".into(), seconds(from).into(), "-t".into(), seconds(length).into(), "-i".into()]);
    args.push(path.into());
}

/// ffmpeg's inputs and `-filter_complex` for the movie from `from` seconds
/// on, with the picture (if asked for) at `[v]` and the sound (if asked for)
/// at `[a]`. `None` when nothing on the video track is left to play.
///
/// The video track's clips are scaled into `size`, letterboxed, and joined
/// with their own sound (or silence, for a clip without any). Each clip's
/// picture is padded with its last frame and then cut, and its sound padded
/// with silence and cut, to exactly the clip's length, so over many clips the
/// two never drift apart. The music and
/// voiceover clips are delayed to their places and mixed over that, and
/// end with the video.
pub fn movie(p: &Project, from: f64, size: Size, picture: Option<Pixels>, sound: bool) -> Option<Vec<OsString>> {
    let Size { width: w, height: h } = size;
    let mut args = Vec::new();
    let mut chains = Vec::new();
    let mut joined = String::new();
    let mut inputs = 0;
    let mut segments = 0;
    let mut at = 0.0;
    for clip in &p.video {
        let start = at;
        at += clip.length;
        let Some(m) = p.media(clip.media) else { continue };
        if at <= from + 1e-3 {
            continue;
        }
        let skip = (from - start).max(0.0);
        let length = seconds(clip.length - skip);
        input(&mut args, &m.path, clip.from + skip, clip.length - skip);
        let (i, n) = (inputs, segments);
        inputs += 1;
        segments += 1;
        if let Some(pixels) = picture {
            let format = match pixels {
                Pixels::Yuv420 => "yuv420p",
                Pixels::Rgba => "rgba",
            };
            chains.push(format!(
                "[{i}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:-1:-1,setsar=1,\
                 fps={FPS},tpad=stop_mode=clone:stop_duration=1,trim=duration={length},setpts=PTS-STARTPTS,format={format}[v{n}]"
            ));
            joined += &format!("[v{n}]");
        }
        if sound {
            if m.has_audio() {
                chains.push(format!(
                    "[{i}:a:0]{AFORMAT},volume={:.3},apad,atrim=duration={length},asetpts=PTS-STARTPTS[a{n}]",
                    clip.volume
                ));
            } else {
                chains.push(format!("anullsrc=r={RATE}:cl=stereo,atrim=duration={length}[a{n}]"));
            }
            joined += &format!("[a{n}]");
        }
    }
    if segments == 0 || (picture.is_none() && !sound) {
        return None;
    }
    chains.push(format!(
        "{joined}concat=n={segments}:v={}:a={}{}{}",
        picture.is_some() as u8,
        sound as u8,
        if picture.is_some() { "[v]" } else { "" },
        if sound { "[base]" } else { "" },
    ));

    if sound {
        let end = p.duration();
        let mut mixed = vec!["[base]".to_string()];
        for placed in p.music.iter().chain(&p.voice) {
            let Some(m) = p.media(placed.clip.media) else { continue };
            if placed.end() <= from || placed.at >= end || !m.has_audio() {
                continue;
            }
            let skip = (from - placed.at).max(0.0);
            // No more than plays before the movie ends.
            let length = (placed.clip.length - skip).min(end - placed.at.max(from));
            input(&mut args, &m.path, placed.clip.from + skip, length);
            let delay = ((placed.at - from).max(0.0) * 1000.0).round() as u64;
            chains.push(format!(
                "[{inputs}:a:0]{AFORMAT},volume={:.3},adelay={delay}:all=1[m{inputs}]",
                placed.clip.volume
            ));
            mixed.push(format!("[m{inputs}]"));
            inputs += 1;
        }
        if mixed.len() == 1 {
            chains.push("[base]anull[a]".into());
        } else {
            // Not normalised: amix would otherwise quieten the video's own sound
            // by the number of tracks, even where nothing plays over it.
            chains.push(format!("{}amix=inputs={}:duration=first:normalize=0[a]", mixed.concat(), mixed.len()));
        }
    }
    args.push("-filter_complex".into());
    args.push(chains.join(";").into());
    Some(args)
}

/// One RGBA frame, `size` big, of the movie at `t`: only the clip under it is read.
pub fn still(p: &Project, t: f64, size: Size) -> Option<Vec<OsString>> {
    let (i, offset) = p.video_at(t)?;
    let clip = &p.video[i];
    let m = p.media(clip.media)?;
    let Size { width: w, height: h } = size;
    let mut args: Vec<OsString> =
        vec!["-ss".into(), seconds(clip.from + offset).into(), "-i".into(), m.path.clone().into()];
    args.extend(
        [
            "-frames:v",
            "1",
            "-vf",
            &format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:-1:-1,setsar=1,format=rgba"),
            "-f",
            "rawvideo",
            "-",
        ]
        .map(OsString::from),
    );
    Some(args)
}

/// An ffmpeg that doesn't read the keyboard and says only what went wrong.
pub fn ffmpeg() -> Command {
    let mut c = Command::new("ffmpeg");
    c.args(["-hide_banner", "-nostdin", "-loglevel", "error"]);
    c
}

/// The last line ffmpeg wrote to stderr, as the reason it failed.
pub fn reason(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ffmpeg failed").trim().to_string()
}

/// A movie being written to a file by ffmpeg, in the background.
pub struct Export {
    pub path: PathBuf,
    pub duration: f64,
    child: Arc<Mutex<Child>>,
    /// How far it has got, in microseconds of the movie.
    done_us: Arc<AtomicU64>,
    finished: Receiver<Result<(), String>>,
    cancelled: bool,
}

impl Export {
    pub fn start(p: &Project, size: Size, path: PathBuf, ctx: &egui::Context) -> Result<Self, String> {
        let graph =
            movie(p, 0.0, size, Some(Pixels::Yuv420), true).ok_or("There's nothing on the video track to export")?;
        let mut cmd = ffmpeg();
        cmd.args(["-y", "-progress", "pipe:1", "-nostats"]).args(graph);
        cmd.args(["-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-preset", "medium", "-crf", "20"]);
        cmd.args(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]).arg(&path);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Couldn't start ffmpeg: {e}"))?;
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let child = Arc::new(Mutex::new(child));
        let done_us = Arc::new(AtomicU64::new(0));
        let (tx, finished) = mpsc::channel();
        let errors = std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stderr.read_to_end(&mut text);
            text
        });
        let (waiter, progress, ctx) = (child.clone(), done_us.clone(), ctx.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // `out_time_ms` is in microseconds too, despite its name.
                if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse().ok()) {
                    progress.store(us, Ordering::Relaxed);
                    ctx.request_repaint();
                }
            }
            let status = waiter.lock().unwrap().wait();
            let stderr = errors.join().unwrap_or_default();
            let _ = tx.send(match status {
                Ok(s) if s.success() => Ok(()),
                Ok(_) => Err(reason(&stderr)),
                Err(e) => Err(e.to_string()),
            });
            ctx.request_repaint();
        });
        Ok(Export { path, duration: p.duration(), child, done_us, finished, cancelled: false })
    }

    /// How much is written, from 0 to 1.
    pub fn progress(&self) -> f32 {
        let done = self.done_us.load(Ordering::Relaxed) as f64 / 1e6;
        (done / self.duration.max(0.001)).clamp(0.0, 1.0) as f32
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        let _ = self.child.lock().unwrap().kill();
    }

    /// Once it has finished: whether it worked. A cancelled export's half-written file is removed.
    pub fn poll(&self) -> Option<Result<(), String>> {
        let result = self.finished.try_recv().ok()?;
        if self.cancelled {
            let _ = std::fs::remove_file(&self.path);
            return Some(Err("Export cancelled".into()));
        }
        Some(result)
    }
}

impl Drop for Export {
    /// Quitting mid-export stops ffmpeg too.
    fn drop(&mut self) {
        let _ = self.child.lock().map(|mut c| c.kill());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Info, Track};

    const SMALL: Size = Size { width: 640, height: 360 };

    fn project() -> Project {
        let mut p = Project::default();
        for (name, info) in [
            ("/a.mp4", Info { duration: 10.0, size: Some([1920, 1080]), audio: true }),
            ("/silent.mp4", Info { duration: 4.0, size: Some([640, 480]), audio: false }),
            ("/song.mp3", Info { duration: 60.0, size: None, audio: true }),
        ] {
            let (id, _) = p.add_media(name.into());
            p.media_mut(id).unwrap().info = Some(info);
        }
        p.drop_media(Track::Video, 1, 0.0).unwrap();
        p.drop_media(Track::Video, 2, 99.0).unwrap();
        p
    }

    fn text(args: &[OsString]) -> Vec<String> {
        args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    fn graph(args: &[OsString]) -> String {
        text(args).last().unwrap().clone()
    }

    #[test]
    fn the_video_track_is_joined_with_its_sound() {
        let args = movie(&project(), 0.0, SMALL, Some(Pixels::Yuv420), true).unwrap();
        let t = text(&args);
        assert_eq!(&t[..10], ["-ss", "0.000", "-t", "10.000", "-i", "/a.mp4", "-ss", "0.000", "-t", "4.000"]);
        let g = graph(&args);
        assert!(g.contains("[0:v:0]scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:-1:-1"), "{g}");
        assert!(g.contains("[0:a:0]aformat"), "{g}");
        // The silent clip gets silence of its own length, so the sound stays in step.
        assert!(g.contains("anullsrc=r=48000:cl=stereo,atrim=duration=4.000[a1]"), "{g}");
        assert!(g.contains("[v0][a0][v1][a1]concat=n=2:v=1:a=1[v][base]"), "{g}");
        assert!(g.ends_with("[base]anull[a]"), "{g}");
    }

    #[test]
    fn starting_part_way_seeks_into_the_clip_under_it() {
        let args = movie(&project(), 12.5, SMALL, Some(Pixels::Rgba), false).unwrap();
        let t = text(&args);
        assert_eq!(&t[..6], ["-ss", "2.500", "-t", "1.500", "-i", "/silent.mp4"]);
        let g = graph(&args);
        assert!(g.contains("format=rgba[v0]") && g.ends_with("[v0]concat=n=1:v=1:a=0[v]"), "{g}");
        assert!(movie(&project(), 14.0, SMALL, Some(Pixels::Rgba), false).is_none(), "past the end");
        assert!(movie(&Project::default(), 0.0, SMALL, None, true).is_none(), "nothing to play");
    }

    #[test]
    fn music_and_voiceover_are_mixed_in_at_their_places() {
        let mut p = project();
        p.drop_media(Track::Music, 3, 2.0).unwrap();
        p.drop_media(Track::Voice, 1, 20.0).unwrap(); // after the movie ends: left out
        p.voice.push(crate::project::Placed { at: 0.5, clip: p.video[0].clone() });
        p.music[0].clip.volume = 0.5;
        let args = movie(&p, 1.0, SMALL, None, true).unwrap();
        let t = text(&args);
        // The song from its start, a second later; only the 13 s the movie lasts after it.
        assert!(t.windows(6).any(|w| w == ["-ss", "0.000", "-t", "12.000", "-i", "/song.mp3"]), "{t:?}");
        let g = graph(&args);
        assert!(g.contains("[2:a:0]") && g.contains("volume=0.500,adelay=1000:all=1[m2]"), "{g}");
        // The voiceover began before the preview did: it's read from half a second in, at once.
        assert!(t.windows(4).any(|w| w == ["-ss", "0.500", "-t", "9.500"]), "{t:?}");
        assert!(g.contains("adelay=0:all=1[m3]"), "{g}");
        assert!(g.ends_with("[base][m2][m3]amix=inputs=3:duration=first:normalize=0[a]"), "{g}");
    }

    #[test]
    fn a_still_reads_only_the_clip_under_it() {
        let t = text(&still(&project(), 11.0, SMALL).unwrap());
        assert_eq!(&t[..4], ["-ss", "1.000", "-i", "/silent.mp4"]);
        assert!(still(&project(), 30.0, SMALL).is_none());
    }

    /// The graph through a real ffmpeg: tiny files made from its test
    /// sources, a movie with a silent clip, music and a voiceover, written to
    /// a file and piped as the preview does. Skipped where ffmpeg isn't installed.
    #[test]
    fn ffmpeg_runs_the_graph() {
        let Some((dir, p)) = crate::testing::tiny_project("render") else { return };
        let out = dir.join("movie.mp4");
        let graph = movie(&p, 0.0, SIZES[0].1, Some(Pixels::Yuv420), true).unwrap();
        let done = ffmpeg()
            .args(graph)
            .args(["-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-y"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(done.status.success(), "{}", reason(&done.stderr));
        let probed = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
            .arg(&out)
            .output()
            .unwrap();
        let duration: f64 = String::from_utf8_lossy(&probed.stdout).trim().parse().unwrap();
        assert!((duration - 3.0).abs() < 0.15, "the movie is {duration} s, not 3");

        // The preview's picture from 1.5 s: half a second of the first clip, then the second.
        let preview = Size { width: 64, height: 36 };
        let frames = ffmpeg()
            .args(movie(&p, 1.5, preview, Some(Pixels::Rgba), false).unwrap())
            .args(["-map", "[v]", "-f", "rawvideo", "-"])
            .output()
            .unwrap();
        assert!(frames.status.success(), "{}", reason(&frames.stderr));
        assert_eq!(frames.stdout.len(), 45 * 64 * 36 * 4, "1.5 s at {FPS} fps");
        let sound = ffmpeg()
            .args(movie(&p, 1.5, preview, None, true).unwrap())
            .args(["-map", "[a]", "-f", "f32le", "-ac", "2", "-ar", &RATE.to_string(), "-"])
            .output()
            .unwrap();
        assert!(sound.status.success(), "{}", reason(&sound.stderr));
        let seconds = sound.stdout.len() as f64 / 8.0 / RATE as f64;
        assert!((seconds - 1.5).abs() < 0.05, "{seconds} s of sound");
        let still = ffmpeg().args(super::still(&p, 2.5, preview).unwrap()).output().unwrap();
        assert_eq!(still.stdout.len(), 64 * 36 * 4);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failures_are_ffmpeg_s_last_word() {
        assert_eq!(reason(b"first\nNo such file\n\n"), "No such file");
        assert_eq!(reason(b""), "ffmpeg failed");
    }
}

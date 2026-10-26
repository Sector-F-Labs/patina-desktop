//! The preview: the movie played from the playhead, its picture and its
//! sound each piped from an ffmpeg running `render::movie`'s graph
//! (ADR-0002), and, while it's stopped, a still of the frame under the
//! playhead. The sound is the clock: a frame shows once the sound has
//! played up to it.

use std::ffi::OsString;
use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::time::Instant;

use rodio::{ChannelCount, MixerDeviceSink, Player, SampleRate, Source};

use crate::project::Project;
use crate::render::{self, FPS, Pixels, RATE, Size};

/// The preview's picture: small enough to decode in real time anywhere.
pub const SIZE: Size = Size { width: 640, height: 360 };

const FRAME_BYTES: usize = (SIZE.width * SIZE.height * 4) as usize;

/// Stereo frames of sound in each chunk the reader passes on: about 85 ms.
const CHUNK_FRAMES: usize = 4096;

pub struct Preview {
    /// Dropping it stops all sound, so it lives as long as the preview.
    device: Option<MixerDeviceSink>,
    /// Why there's no sound, if there isn't.
    pub no_sound: Option<String>,
    playing: Option<Playing>,
    stills: Stills,
    texture: Option<egui::TextureHandle>,
}

struct Playing {
    from: f64,
    end: f64,
    started: Instant,
    sound: Option<Sound>,
    frames: Receiver<Vec<u8>>,
    shown: u64,
    children: Vec<Child>,
}

struct Sound {
    player: Player,
    /// Samples played (both channels).
    played: Arc<AtomicU64>,
    /// The sound has run out, or never came.
    done: Arc<AtomicBool>,
}

impl Drop for Playing {
    fn drop(&mut self) {
        if let Some(sound) = &self.sound {
            sound.player.stop();
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Playing {
    /// Where in the movie it has got to.
    fn position(&self) -> f64 {
        match &self.sound {
            // A sound that failed at once can't be the clock; the wall clock is.
            Some(s) if !(s.done.load(Ordering::Relaxed) && s.played.load(Ordering::Relaxed) == 0) => {
                self.from + s.played.load(Ordering::Relaxed) as f64 / 2.0 / RATE as f64
            }
            _ => self.from + self.started.elapsed().as_secs_f64(),
        }
    }
}

/// Reads until `buf` is full or the stream ends; how much it read.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    n
}

impl Preview {
    pub fn new(ctx: &egui::Context) -> Self {
        let (device, no_sound) = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut device) => {
                device.log_on_drop(false);
                (Some(device), None)
            }
            Err(e) => (None, Some(format!("No sound output: {e}"))),
        };
        Preview { device, no_sound, playing: None, stills: Stills::start(ctx), texture: None }
    }

    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        self.texture.as_ref()
    }

    pub fn is_playing(&self) -> bool {
        self.playing.is_some()
    }

    /// Plays the movie from `from`, silently if `muted` (while a voiceover is recorded).
    pub fn play(&mut self, p: &Project, from: f64, muted: bool, ctx: &egui::Context) -> Result<(), String> {
        self.stop();
        let picture = render::movie(p, from, SIZE, Some(Pixels::Rgba), false).ok_or("There's nothing to play")?;
        let mut children = Vec::new();
        let mut video = render::ffmpeg()
            .args(picture)
            .args(["-map", "[v]", "-f", "rawvideo", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start ffmpeg: {e}"))?;
        let mut out = video.stdout.take().expect("piped");
        children.push(video);
        let (tx, frames) = mpsc::sync_channel(8);
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            loop {
                let mut frame = vec![0; FRAME_BYTES];
                if fill(&mut out, &mut frame) < FRAME_BYTES || tx.send(frame).is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        });

        let sound = match (&self.device, render::movie(p, from, SIZE, None, true)) {
            (Some(device), Some(graph)) => {
                let started = render::ffmpeg()
                    .args(graph)
                    .args(["-map", "[a]", "-f", "f32le", "-ac", "2", "-ar", &RATE.to_string(), "-"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();
                match started {
                    Ok(mut audio) => {
                        let out = audio.stdout.take().expect("piped");
                        children.push(audio);
                        Some(Sound::start(device, out, muted))
                    }
                    Err(_) => None,
                }
            }
            _ => None,
        };
        self.playing =
            Some(Playing { from, end: p.duration(), started: Instant::now(), sound, frames, shown: 0, children });
        Ok(())
    }

    pub fn stop(&mut self) {
        self.playing = None;
    }

    /// While playing, shows the frames that are due, and says where it has
    /// got to: `None` once it has stopped at the end.
    pub fn advance(&mut self, ctx: &egui::Context) -> Option<f64> {
        let playing = self.playing.as_mut()?;
        let position = playing.position();
        let sound_done = playing.sound.as_ref().is_some_and(|s| s.done.load(Ordering::Relaxed));
        if position >= playing.end - 0.5 / FPS as f64 || (sound_done && position >= playing.end - 0.25) {
            self.playing = None;
            return None;
        }
        let due = ((position - playing.from) * FPS as f64).floor() as u64 + 1;
        let mut latest = None;
        while playing.shown < due {
            match playing.frames.try_recv() {
                Ok(frame) => {
                    playing.shown += 1;
                    latest = Some(frame);
                }
                Err(_) => break,
            }
        }
        if let Some(frame) = latest {
            self.show(ctx, &frame);
        }
        ctx.request_repaint();
        Some(position)
    }

    /// Asks for the frame at `t` while stopped. The newest ask wins.
    pub fn still(&mut self, p: &Project, t: f64) {
        match render::still(p, t, SIZE) {
            Some(args) => self.stills.ask(args),
            None => {
                self.stills.ask(Vec::new());
                self.texture = None;
            }
        }
    }

    /// Shows a still that has arrived.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(frame) = self.stills.take()
            && !self.is_playing()
        {
            match frame {
                Some(frame) => self.show(ctx, &frame),
                None => self.texture = None,
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context, rgba: &[u8]) {
        let image = egui::ColorImage::from_rgba_unmultiplied([SIZE.width as usize, SIZE.height as usize], rgba);
        match &mut self.texture {
            Some(t) => t.set(image, egui::TextureOptions::LINEAR),
            None => self.texture = Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR)),
        }
    }
}

#[cfg(test)]
impl Preview {
    /// A preview with no sound output, which keeps time by the wall clock.
    fn silent(ctx: &egui::Context) -> Self {
        Preview { device: None, no_sound: None, playing: None, stills: Stills::start(ctx), texture: None }
    }
}

impl Sound {
    fn start(device: &MixerDeviceSink, mut out: impl Read + Send + 'static, muted: bool) -> Self {
        let (tx, chunks): (SyncSender<Vec<f32>>, _) = mpsc::sync_channel(16);
        std::thread::spawn(move || {
            let mut bytes = vec![0; CHUNK_FRAMES * 2 * 4];
            loop {
                // Whole stereo frames only, so the channels never swap.
                let n = fill(&mut out, &mut bytes) / 8 * 8;
                if n == 0 {
                    break;
                }
                let samples =
                    bytes[..n].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                if tx.send(samples).is_err() || n < bytes.len() {
                    break;
                }
            }
        });
        let played = Arc::new(AtomicU64::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let player = Player::connect_new(device.mixer());
        player.set_volume(if muted { 0.0 } else { 1.0 });
        player.append(Feed {
            chunks,
            samples: Vec::new(),
            next: 0,
            silence: 0,
            played: played.clone(),
            done: done.clone(),
        });
        Sound { player, played, done }
    }
}

/// The sound from ffmpeg, for rodio. When ffmpeg falls behind it plays
/// silence, which doesn't move the clock, so the picture waits too.
struct Feed {
    chunks: Receiver<Vec<f32>>,
    samples: Vec<f32>,
    next: usize,
    silence: u8,
    played: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
}

impl Iterator for Feed {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.silence > 0 {
            self.silence -= 1;
            return Some(0.0);
        }
        if self.next >= self.samples.len() {
            match self.chunks.try_recv() {
                Ok(samples) => {
                    self.samples = samples;
                    self.next = 0;
                }
                Err(TryRecvError::Empty) => {
                    // A whole frame of silence: this one and its other channel.
                    self.silence = 1;
                    return Some(0.0);
                }
                Err(TryRecvError::Disconnected) => {
                    self.done.store(true, Ordering::Relaxed);
                    return None;
                }
            }
        }
        let sample = self.samples[self.next];
        self.next += 1;
        self.played.fetch_add(1, Ordering::Relaxed);
        Some(sample)
    }
}

impl Source for Feed {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).expect("two")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(RATE).expect("not zero")
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// A worker that decodes one frame at a time for the stopped preview, always
/// the newest asked for, so dragging the playhead never queues up frames.
struct Stills {
    tx: Sender<Vec<OsString>>,
    rx: Receiver<Option<Vec<u8>>>,
}

impl Stills {
    fn start(ctx: &egui::Context) -> Self {
        let (tx, asks) = mpsc::channel::<Vec<OsString>>();
        let (send, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(mut args) = asks.recv() {
                while let Ok(newer) = asks.try_recv() {
                    args = newer;
                }
                // Nothing to show: a cleared preview.
                let frame = if args.is_empty() {
                    None
                } else {
                    render::ffmpeg()
                        .args(args)
                        .stderr(Stdio::null())
                        .output()
                        .ok()
                        .and_then(|o| (o.stdout.len() == FRAME_BYTES).then_some(o.stdout))
                };
                if send.send(frame).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        Stills { tx, rx }
    }

    fn ask(&self, args: Vec<OsString>) {
        let _ = self.tx.send(args);
    }

    /// The newest still that has arrived.
    fn take(&self) -> Option<Option<Vec<u8>>> {
        self.rx.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn it_plays_from_the_playhead_to_the_end_and_stops() {
        let Some((dir, p)) = crate::testing::tiny_project("preview") else { return };
        let ctx = egui::Context::default();
        let mut preview = Preview::silent(&ctx);
        let started = Instant::now();
        preview.play(&p, 1.5, false, &ctx).unwrap();
        assert!(preview.is_playing());
        let mut last = 1.5;
        let mut shown = 0;
        while let Some(t) = preview.advance(&ctx) {
            assert!(t >= last, "time runs forwards");
            last = t;
            if preview.texture().is_some() {
                shown += 1;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "it ends");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!preview.is_playing());
        assert!(shown > 0, "frames were shown");
        let took = started.elapsed().as_secs_f64();
        assert!((1.3..3.0).contains(&took), "1.5 s of movie took {took} s");

        preview.still(&p, 2.5);
        let asked = Instant::now();
        preview.texture = None;
        while preview.texture().is_none() && asked.elapsed() < Duration::from_secs(5) {
            preview.poll(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(preview.texture().is_some(), "a still arrived");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

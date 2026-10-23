//! Playback (ADR-0003): one rodio `Player` on the default output device.
//!
//! An episode is decoded on a thread of its own, from a `GrowingFile` that
//! may still be downloading, into chunks sent to a `Feed` that rodio plays.
//! Reads that wait for the network block only that thread; the audio
//! callback never waits, and plays silence ("buffering") when it runs dry.
//! Seeking goes to the decoding thread, and chunks decoded before a seek are
//! dropped by their generation.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Decoder, MixerDeviceSink, Player, SampleRate, Source};

use crate::download::{Download, GrowingFile};

/// Samples per chunk, interleaved; a multiple of any channel count we'll meet.
const CHUNK: usize = 4096 * 6;
/// Chunks decoded ahead: a few seconds.
const AHEAD: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Loading,
    Playing,
    Paused,
}

/// What the decoding thread and the `Feed` share.
#[derive(Default)]
struct Shared {
    /// Bumped by every seek; older chunks are dropped.
    generation: AtomicU64,
    /// Where the `Feed` is, in milliseconds.
    position_ms: AtomicU64,
    /// The `Feed` ran out of decoded audio and is playing silence.
    starving: AtomicBool,
    /// The decoder reached the end.
    ended: AtomicBool,
    /// The track was replaced or stopped.
    stop: Arc<AtomicBool>,
}

enum Cmd {
    Seek(Duration),
}

struct Opened {
    id: String,
    result: Result<Option<Duration>, String>,
}

struct Track {
    id: String,
    shared: Arc<Shared>,
    cmd: Sender<Cmd>,
    state: PlayState,
    duration: Option<Duration>,
}

pub struct Engine {
    // Dropping the device sink stops all audio, so it lives as long as the engine.
    _device: Option<MixerDeviceSink>,
    player: Arc<Player>,
    /// Held while one track is swapped for another, so a slow opener can't
    /// append its track after a newer one.
    swap: Arc<Mutex<()>>,
    track: Option<Track>,
    tx: Sender<Opened>,
    rx: Receiver<Opened>,
    pub error: Option<String>,
    /// The id of a track that played to its end, for the app to take.
    finished: Option<String>,
}

impl Engine {
    pub fn new(volume: f32) -> Self {
        let (device, error) = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut device) => {
                device.log_on_drop(false);
                (Some(device), None)
            }
            Err(e) => (None, Some(format!("No audio output: {e}"))),
        };
        let player = match &device {
            Some(device) => Player::connect_new(device.mixer()),
            // No device: an unconnected player still accepts sources, it just never plays them.
            None => Player::new().0,
        };
        player.set_volume(volume);
        let (tx, rx) = mpsc::channel();
        Self {
            _device: device,
            player: Arc::new(player),
            swap: Arc::new(Mutex::new(())),
            track: None,
            tx,
            rx,
            error,
            finished: None,
        }
    }

    /// An engine with no output device, for tests.
    #[cfg(test)]
    pub fn silent() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            _device: None,
            player: Arc::new(Player::new().0),
            swap: Arc::new(Mutex::new(())),
            track: None,
            tx,
            rx,
            error: None,
            finished: None,
        }
    }

    pub fn current(&self) -> Option<&str> {
        self.track.as_ref().map(|t| t.id.as_str())
    }

    pub fn state(&self) -> PlayState {
        self.track.as_ref().map_or(PlayState::Stopped, |t| t.state)
    }

    pub fn duration(&self) -> Option<Duration> {
        self.track.as_ref().and_then(|t| t.duration)
    }

    pub fn position(&self) -> Duration {
        self.track
            .as_ref()
            .map_or(Duration::ZERO, |t| Duration::from_millis(t.shared.position_ms.load(Ordering::Relaxed)))
    }

    /// Playing, but waiting for the download.
    pub fn buffering(&self) -> bool {
        self.track.as_ref().is_some_and(|t| match t.state {
            PlayState::Loading => true,
            PlayState::Playing => t.shared.starving.load(Ordering::Relaxed),
            _ => false,
        })
    }

    /// Plays `id` from `audio`, starting at `start`, replacing whatever is
    /// playing. `duration` (from the feed) stands until the decoder knows better.
    pub fn play(
        &mut self,
        id: &str,
        audio: Arc<Download>,
        hint: &str,
        start: Duration,
        duration: Option<Duration>,
        ctx: &egui::Context,
    ) {
        self.stop();
        let shared = Arc::new(Shared::default());
        shared.position_ms.store(start.as_millis() as u64, Ordering::SeqCst);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        self.track = Some(Track {
            id: id.to_string(),
            shared: Arc::clone(&shared),
            cmd: cmd_tx,
            state: PlayState::Loading,
            duration,
        });
        self.error = None;

        let (player, swap, tx, ctx) = (Arc::clone(&self.player), Arc::clone(&self.swap), self.tx.clone(), ctx.clone());
        let (id, hint) = (id.to_string(), hint.to_string());
        std::thread::spawn(move || {
            let opened = open(&audio, &hint, &shared).map(|mut decoder| {
                if start > Duration::ZERO {
                    let _ = decoder.try_seek(start);
                }
                decoder
            });
            let decoder = match opened {
                Ok(d) => d,
                Err(e) => {
                    let _ = tx.send(Opened { id, result: Err(e) });
                    ctx.request_repaint();
                    return;
                }
            };
            let total = decoder.total_duration();
            let (chunks_tx, chunks_rx) = mpsc::sync_channel(AHEAD);
            let feed = Feed::new(&decoder, chunks_rx, Arc::clone(&shared), total);
            {
                let _guard = swap.lock().unwrap_or_else(|e| e.into_inner());
                if shared.stop.load(Ordering::SeqCst) {
                    return;
                }
                player.append(feed);
                player.play();
            }
            let _ = tx.send(Opened { id, result: Ok(total) });
            ctx.request_repaint();
            decode(decoder, start, chunks_tx, cmd_rx, &shared, &ctx);
        });
    }

    pub fn toggle_pause(&mut self) {
        let Some(track) = &mut self.track else { return };
        match track.state {
            PlayState::Playing => {
                self.player.pause();
                track.state = PlayState::Paused;
            }
            PlayState::Paused => {
                self.player.play();
                track.state = PlayState::Playing;
            }
            PlayState::Stopped | PlayState::Loading => {}
        }
    }

    pub fn stop(&mut self) {
        if let Some(track) = self.track.take() {
            let _guard = self.swap.lock().unwrap_or_else(|e| e.into_inner());
            track.shared.stop.store(true, Ordering::SeqCst);
            self.player.clear();
        }
    }

    pub fn seek(&mut self, pos: Duration) {
        let Some(track) = &self.track else { return };
        let pos = match track.duration {
            Some(d) => pos.min(d.saturating_sub(Duration::from_secs(1))),
            None => pos,
        };
        track.shared.position_ms.store(pos.as_millis() as u64, Ordering::SeqCst);
        let _ = track.cmd.send(Cmd::Seek(pos));
    }

    /// Seeks by `by` seconds from where it is.
    pub fn skip(&mut self, by: i64) {
        let now = self.position().as_secs_f64();
        self.seek(Duration::from_secs_f64((now + by as f64).max(0.0)));
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }

    /// Takes opened tracks and notices the end of one. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok(Opened { id, result }) = self.rx.try_recv() {
            let Some(track) = self.track.as_mut().filter(|t| t.id == id) else { continue };
            match result {
                Ok(total) => {
                    if total.is_some() {
                        track.duration = total;
                    }
                    track.state = PlayState::Playing;
                }
                Err(e) => {
                    self.error = Some(e);
                    self.track = None;
                }
            }
        }
        if let Some(track) = &self.track
            && track.state == PlayState::Playing
            && self.player.empty()
        {
            self.finished = Some(track.id.clone());
            self.track = None;
        }
    }

    /// The id of the episode that played to its end since the last call.
    pub fn take_finished(&mut self) -> Option<String> {
        self.finished.take()
    }
}

fn open(audio: &Arc<Download>, hint: &str, shared: &Shared) -> Result<Decoder<GrowingFile>, String> {
    let reader = audio.reader(Arc::clone(&shared.stop)).map_err(|e| e.to_string())?;
    let builder = Decoder::builder().with_data(reader).with_hint(hint).with_gapless(false);
    // With the length known, MP3 can seek by estimate rather than reading
    // every frame from the start, which would wait for the whole download.
    let builder = match audio.total() {
        Some(len) => builder.with_byte_len(len).with_coarse_seek(true),
        None => builder.with_seekable(true),
    };
    builder.build().map_err(|e| match audio.error() {
        Some(e) => e,
        None => format!("Can't play this episode: {e}"),
    })
}

/// The decoding thread's loop: decode ahead into chunks, and seek on request.
fn decode(
    mut decoder: Decoder<GrowingFile>,
    start: Duration,
    chunks: SyncSender<Chunk>,
    cmds: Receiver<Cmd>,
    shared: &Shared,
    ctx: &egui::Context,
) {
    let rate = decoder.sample_rate().get() as u64;
    let channels = decoder.channels().get() as usize;
    let mut base_ms = start.as_millis() as u64;
    let mut frames = 0u64;
    let mut pending: Option<Cmd> = None;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let cmd = pending.take().or_else(|| cmds.try_recv().ok());
        if let Some(Cmd::Seek(pos)) = cmd {
            // Silence while the seek waits, rather than the old place.
            shared.generation.fetch_add(1, Ordering::SeqCst);
            shared.ended.store(false, Ordering::SeqCst);
            if decoder.try_seek(pos).is_ok() {
                base_ms = pos.as_millis() as u64;
                frames = 0;
            }
            shared.position_ms.store(base_ms, Ordering::SeqCst);
            continue;
        }
        if shared.ended.load(Ordering::SeqCst) {
            // At the end; wait for a seek back, or for the feed to finish.
            match cmds.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => pending = Some(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            continue;
        }
        let samples: Vec<f32> = decoder.by_ref().take(CHUNK).collect();
        let whole = samples.len() - samples.len() % channels.max(1);
        let mut chunk = Chunk {
            generation: shared.generation.load(Ordering::SeqCst),
            start_ms: base_ms + frames * 1000 / rate.max(1),
            samples: samples[..whole].to_vec(),
        };
        frames += (whole / channels.max(1)) as u64;
        if chunk.samples.is_empty() {
            shared.ended.store(true, Ordering::SeqCst);
            ctx.request_repaint();
            continue;
        }
        // Wait for room, but keep answering seeks.
        loop {
            match chunks.try_send(chunk) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return,
                Err(TrySendError::Full(c)) => {
                    if shared.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Ok(cmd) = cmds.try_recv() {
                        pending = Some(cmd);
                        break;
                    }
                    chunk = c;
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
}

struct Chunk {
    generation: u64,
    start_ms: u64,
    samples: Vec<f32>,
}

/// The source rodio plays: decoded chunks, or silence while there are none.
struct Feed {
    chunks: Receiver<Chunk>,
    shared: Arc<Shared>,
    channels: ChannelCount,
    rate: SampleRate,
    total: Option<Duration>,
    samples: Vec<f32>,
    next: usize,
    /// The generation `samples` came from; a seek leaves what's left of them unplayed.
    generation: u64,
    /// Samples of silence still to give, whole frames at a time.
    silence: usize,
}

impl Feed {
    fn new(
        decoder: &Decoder<GrowingFile>,
        chunks: Receiver<Chunk>,
        shared: Arc<Shared>,
        total: Option<Duration>,
    ) -> Self {
        Self {
            chunks,
            shared,
            channels: decoder.channels(),
            rate: decoder.sample_rate(),
            total,
            samples: Vec::new(),
            next: 0,
            generation: 0,
            silence: 0,
        }
    }

    /// The next chunk of the current generation, if one is decoded.
    fn take_chunk(&mut self) -> Result<bool, TryRecvError> {
        loop {
            let chunk = self.chunks.try_recv()?;
            if chunk.generation == self.shared.generation.load(Ordering::SeqCst) {
                self.shared.position_ms.store(chunk.start_ms, Ordering::Relaxed);
                self.samples = chunk.samples;
                self.next = 0;
                self.generation = chunk.generation;
                return Ok(true);
            }
        }
    }
}

impl Iterator for Feed {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.silence > 0 {
            self.silence -= 1;
            return Some(0.0);
        }
        if self.generation != self.shared.generation.load(Ordering::Relaxed) {
            self.next = self.samples.len();
        }
        if self.next >= self.samples.len() {
            match self.take_chunk() {
                Ok(_) => self.shared.starving.store(false, Ordering::Relaxed),
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => {
                    // The decoder sends its last chunk before it says it ended, so look once more.
                    if self.shared.ended.load(Ordering::SeqCst) && !matches!(self.take_chunk(), Ok(true)) {
                        return None;
                    }
                    if self.next >= self.samples.len() {
                        self.shared.starving.store(true, Ordering::Relaxed);
                        // A whole frame, so the channels stay in step.
                        self.silence = self.channels.get() as usize - 1;
                        return Some(0.0);
                    }
                }
            }
        }
        let sample = self.samples[self.next];
        self.next += 1;
        Some(sample)
    }
}

impl Source for Feed {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total
    }

    fn try_seek(&mut self, _: Duration) -> Result<(), SeekError> {
        Err(SeekError::NotSupported { underlying_source: "the podcast feed seeks on its decoding thread" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A mono 16-bit WAV of `seconds` at 8 kHz, each second at its own level.
    fn wav(seconds: u32) -> std::fs::File {
        let rate = 8000u32;
        let samples: Vec<i16> = (0..seconds * rate).map(|i| ((i / rate + 1) * 1000) as i16).collect();
        let data = samples.len() as u32 * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let dir = std::env::temp_dir().join(format!("podcasts-player-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{seconds}.wav"));
        std::fs::File::create(&path).unwrap().write_all(&bytes).unwrap();
        std::fs::File::open(&path).unwrap()
    }

    /// Runs the decoding thread and returns the `Feed` it fills, and its seek channel.
    fn feed(seconds: u32, start: Duration) -> (Feed, Sender<Cmd>, Arc<Shared>) {
        let audio = Download::complete(wav(seconds)).unwrap();
        let shared = Arc::new(Shared::default());
        let mut decoder = open(&audio, "wav", &shared).unwrap();
        if start > Duration::ZERO {
            decoder.try_seek(start).unwrap();
        }
        let (chunks_tx, chunks_rx) = mpsc::sync_channel(AHEAD);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let feed = Feed::new(&decoder, chunks_rx, Arc::clone(&shared), decoder.total_duration());
        let thread_shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            decode(decoder, start, chunks_tx, cmd_rx, &thread_shared, &egui::Context::default())
        });
        (feed, cmd_tx, shared)
    }

    /// The next sample that isn't buffering silence.
    fn next_sound(feed: &mut Feed) -> Option<f32> {
        feed.find(|s| *s != 0.0)
    }

    /// Whether the feed reaches the given second soon. A coarse seek, as
    /// MP3s get, keeps up to a packet (1024 samples here) from before it.
    fn reaches_second(feed: &mut Feed, second: u32) -> bool {
        let level = (second + 1) as f32 * 1000.0 / 32768.0;
        feed.filter(|s| *s != 0.0).take(2048).any(|s| (s - level).abs() < 1e-3)
    }

    #[test]
    fn plays_to_the_end() {
        let (feed, _cmd, _) = feed(3, Duration::ZERO);
        assert_eq!(feed.total_duration(), Some(Duration::from_secs(3)));
        let sound = feed.filter(|s| *s != 0.0).count();
        assert_eq!(sound, 3 * 8000);
    }

    #[test]
    fn starts_where_it_was_left() {
        let (mut feed, _cmd, shared) = feed(3, Duration::from_secs(2));
        assert!(reaches_second(&mut feed, 2));
        assert!(shared.position_ms.load(Ordering::SeqCst) >= 2000);
    }

    #[test]
    fn a_seek_drops_what_was_decoded_before_it() {
        let (mut feed, cmd, shared) = feed(4, Duration::ZERO);
        let first = next_sound(&mut feed).unwrap();
        assert!((first - 1000.0 / 32768.0).abs() < 1e-3);
        cmd.send(Cmd::Seek(Duration::from_secs(3))).unwrap();
        // Give the decoding thread time to answer, then everything heard is from the fourth second.
        while shared.generation.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(reaches_second(&mut feed, 3));
    }
}

//! `--daemon`: dictation behind the dictate key (ADR-0072). The compositor
//! signals `DictationStarted` when the key goes down and `DictationStopped`
//! when it comes up, on `com.sectorflabs.Patina.Input`; this records
//! between them, transcribes on a thread of its own, so the next press
//! records at once, and calls `Commit` with the text, which the compositor
//! types into the focused window (ADR-0071).
//!
//! It ends with the compositor: when `com.sectorflabs.Patina.Input` leaves
//! the bus. Nothing else would end it, since it has no Wayland connection to
//! lose, and a restarted desktop would have two.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::audio::Audio;
use crate::capture::{Meter, Recording};
use crate::config::Config;
use crate::transcribe::{Transcriber, Tuning, model_path};

const NAME: &str = "com.sectorflabs.Patina.Input";
const PATH: &str = "/com/sectorflabs/Patina/Input";

/// A press shorter than this was a slip, not dictation.
const SHORTEST: Duration = Duration::from_millis(300);
/// How often the overlay is told the loudness: its waveform's bars.
const METERING: Duration = Duration::from_millis(33);

/// The overlay (`--overlay`, ADR-0073), shown from a press until the text is
/// committed: levels to its input while recording, then "transcribing",
/// then its input closed, which fades it out.
struct Shown {
    child: Child,
    input: Arc<Mutex<Option<ChildStdin>>>,
    metering: Arc<AtomicBool>,
}

impl Shown {
    /// Starts it at the configured edge, metering `meter`. `None`, with a
    /// line in the log, if it can't start: dictation goes on without it.
    fn start(meter: Meter) -> Option<Self> {
        let edge = crate::config::load().overlay;
        // By name, from PATH, where the compositor puts its own folder
        // first: the running binary may have been replaced since.
        let mut child = Command::new("patina-dictate")
            .args(["--overlay", edge.name()])
            .stdin(Stdio::piped())
            .spawn()
            .inspect_err(|e| eprintln!("patina-dictate: the overlay: {e}"))
            .ok()?;
        let input = Arc::new(Mutex::new(child.stdin.take()));
        let metering = Arc::new(AtomicBool::new(true));
        let (to, on) = (input.clone(), metering.clone());
        std::thread::spawn(move || {
            while on.load(Ordering::Relaxed) {
                if !say(&to, &format!("{:.4}", meter.level())) {
                    return;
                }
                std::thread::sleep(METERING);
            }
        });
        Some(Self {
            child,
            input,
            metering,
        })
    }

    fn transcribing(&self) {
        self.metering.store(false, Ordering::Relaxed);
        say(&self.input, "transcribing");
    }

    /// Fades it out, and reaps it when it's gone.
    fn close(mut self) {
        self.metering.store(false, Ordering::Relaxed);
        if let Ok(mut input) = self.input.lock() {
            input.take();
        }
        std::thread::spawn(move || {
            let _ = self.child.wait();
        });
    }
}

/// A line to the overlay; false once it's gone.
fn say(input: &Mutex<Option<ChildStdin>>, line: &str) -> bool {
    let Ok(mut input) = input.lock() else {
        return false;
    };
    input
        .as_mut()
        .is_some_and(|i| writeln!(i, "{line}").and_then(|()| i.flush()).is_ok())
}

/// Listens until the session bus goes away.
/// `model` and `language` are the command line's, which win over the config's.
pub fn run(model: Option<&str>, language: Option<&str>, verbose: bool) -> Result<(), String> {
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    end_with_the_compositor(&bus)?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(NAME)
        .and_then(|b| b.path(PATH))
        .map_err(|e| e.to_string())?
        .build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &bus, None)
        .map_err(|e| e.to_string())?;

    let (work, todo) = mpsc::channel::<Work>();
    let recorded = work.clone();
    let committing = bus.clone();
    let model = model.map(String::from);
    let language = language.map(String::from);
    std::thread::Builder::new()
        .name("transcribe".into())
        .spawn(move || {
            transcribe(
                &todo,
                model.as_deref(),
                language.as_deref(),
                verbose,
                &committing,
            )
        })
        .map_err(|e| e.to_string())?;

    // A fullscreen window may have the keyboard already.
    if fullscreen_now(&bus) {
        let _ = work.send(Work::Fullscreen(true));
    }

    let mut recording: Option<(Recording, Instant, Option<Shown>)> = None;
    for message in signals {
        let Ok(message) = message else { continue };
        let header = message.header();
        match header.member().map(|m| m.as_str()) {
            Some("DictationStarted") if recording.is_none() => match Recording::start() {
                Ok(r) => {
                    let shown = Shown::start(r.meter());
                    recording = Some((r, Instant::now(), shown));
                }
                Err(e) => eprintln!("patina-dictate: {e}"),
            },
            Some("DictationStopped") => {
                if let Some((r, started, shown)) = recording.take() {
                    let audio = r.stop();
                    if started.elapsed() >= SHORTEST {
                        if let Some(shown) = &shown {
                            shown.transcribing();
                        }
                        let _ = recorded.send(Work::Audio(audio, shown));
                    } else if let Some(shown) = shown {
                        shown.close();
                    }
                }
            }
            Some("FullscreenChanged") => {
                if let Ok(on) = message.body().deserialize::<bool>() {
                    let _ = work.send(Work::Fullscreen(on));
                }
            }
            _ => {}
        }
    }
    Err("the session bus went away".into())
}

/// Exits when the compositor's name leaves the bus; an error if it isn't
/// there to begin with.
fn end_with_the_compositor(bus: &zbus::blocking::Connection) -> Result<(), String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus).map_err(|e| e.to_string())?;
    let name = zbus::names::BusName::try_from(NAME).map_err(|e| e.to_string())?;
    let changes = dbus
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .map_err(|e| e.to_string())?;
    if !dbus.name_has_owner(name).map_err(|e| e.to_string())? {
        return Err(format!(
            "{NAME} isn't on the bus: the Patina compositor serves it"
        ));
    }
    std::thread::Builder::new()
        .name("compositor".into())
        .spawn(move || {
            for change in changes {
                if change.args().is_ok_and(|a| a.new_owner().is_none()) {
                    eprintln!("patina-dictate: the compositor left the bus; ending");
                    std::process::exit(0);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// What the thread that holds the model is given to do.
enum Work {
    /// A recording to turn into text.
    Audio(Audio, Option<Shown>),
    /// A fullscreen window, a game or a video, gained the keyboard (true) or
    /// lost it (ADR-0129).
    Fullscreen(bool),
}

/// Whether a fullscreen window has the keyboard now, as the compositor says;
/// false if it doesn't say (an older compositor).
fn fullscreen_now(bus: &zbus::blocking::Connection) -> bool {
    bus.call_method(
        Some(NAME),
        PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(NAME, "Fullscreen"),
    )
    .ok()
    .and_then(|reply| {
        reply
            .body()
            .deserialize::<zbus::zvariant::Value>()
            .ok()
            .and_then(|v| bool::try_from(v).ok())
    })
    .unwrap_or(false)
}

/// When the model, if it's loaded, is to be unloaded next: after `idle_mins`
/// unused since `used`, or `fullscreen_secs` into the fullscreen window that
/// began at `fullscreen`, whichever comes first. A 0 never unloads.
fn unload_at(
    used: Instant,
    fullscreen: Option<Instant>,
    fullscreen_secs: u32,
    idle_mins: u32,
) -> Option<Instant> {
    let idle = (idle_mins > 0).then(|| used + Duration::from_secs(u64::from(idle_mins) * 60));
    let away = fullscreen
        .filter(|_| fullscreen_secs > 0)
        .map(|since| since + Duration::from_secs(u64::from(fullscreen_secs)));
    match (idle, away) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The model, loaded and unloaded as the settings and the desktop say
/// (ADR-0129), and the text it makes.
struct Engine {
    /// What's loaded, and what asked for it.
    transcriber: Option<Transcriber>,
    loaded: Option<(PathBuf, Option<String>)>,
    /// The command line's model and language, which win over the config's.
    model: Option<String>,
    language: Option<String>,
    verbose: bool,
    /// When the model was last loaded or used.
    used: Instant,
    /// Since when a fullscreen window has had the keyboard, if one does.
    fullscreen: Option<Instant>,
}

impl Engine {
    /// Loads what the config and command line ask for, unless it's loaded; a
    /// model that wouldn't load is tried again, so one downloaded later is
    /// used without a restart.
    fn load(&mut self, config: &Config) {
        let path = model_path(
            self.model
                .as_deref()
                .or(config.model.as_deref())
                .unwrap_or(crate::DEFAULT_MODEL),
        );
        let language = self.language.clone().or_else(|| config.language.clone());
        let wanted = (path, language);
        if self.transcriber.is_none() || self.loaded.as_ref() != Some(&wanted) {
            let started = Instant::now();
            self.transcriber = Transcriber::new(&wanted.0, wanted.1.as_deref(), !self.verbose)
                .inspect_err(|e| eprintln!("patina-dictate: {e}"))
                .ok();
            if self.verbose && self.transcriber.is_some() {
                eprintln!(
                    "patina-dictate: loaded {} in {:.2?}",
                    wanted.0.display(),
                    started.elapsed()
                );
            }
            self.loaded = Some(wanted);
        }
        self.used = Instant::now();
    }

    fn unload(&mut self, why: &str) {
        if self.transcriber.take().is_some() {
            self.loaded = None;
            if self.verbose {
                eprintln!("patina-dictate: unloaded the model: {why}");
            }
        }
    }
}

/// Turns each recording into text and commits it, and keeps the model
/// loaded only as long as it's wanted. The settings are read at each
/// recording: a changed model or language is loaded then, the tuning used as
/// it is. The model loads at the first press, or when the daemon starts if
/// `preload` says so, and is unloaded after a stretch in a fullscreen window
/// or of not being used (ADR-0129).
fn transcribe(
    todo: &mpsc::Receiver<Work>,
    model: Option<&str>,
    language: Option<&str>,
    verbose: bool,
    bus: &zbus::blocking::Connection,
) {
    let mut engine = Engine {
        transcriber: None,
        loaded: None,
        model: model.map(String::from),
        language: language.map(String::from),
        verbose,
        used: Instant::now(),
        fullscreen: None,
    };
    let config = crate::config::load();
    if config.preload {
        engine.load(&config);
    }
    loop {
        let config = crate::config::load();
        let deadline = engine
            .transcriber
            .is_some()
            .then(|| {
                unload_at(
                    engine.used,
                    engine.fullscreen,
                    config.unload_fullscreen_secs,
                    config.unload_idle_mins,
                )
            })
            .flatten();
        let work = match deadline {
            Some(deadline) => {
                match todo.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(work) => work,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let fullscreen = engine
                            .fullscreen
                            .is_some_and(|_| config.unload_fullscreen_secs > 0);
                        engine.unload(if fullscreen {
                            "a fullscreen window has had the keyboard a while"
                        } else {
                            "it's been idle"
                        });
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            None => match todo.recv() {
                Ok(work) => work,
                Err(_) => return,
            },
        };
        let (audio, shown) = match work {
            Work::Fullscreen(true) => {
                engine.fullscreen = Some(Instant::now());
                continue;
            }
            Work::Fullscreen(false) => {
                engine.fullscreen = None;
                // Back from the game: ready again, if that's what's asked for.
                if config.preload && engine.transcriber.is_none() {
                    engine.load(&config);
                }
                continue;
            }
            Work::Audio(audio, shown) => (audio, shown),
        };
        engine.load(&config);
        if let Some(t) = &mut engine.transcriber {
            t.tuning = Tuning::from(&config);
        }
        // Gone once the text is in, or there's none.
        let _closes = Closes(shown);
        let Some(t) = &engine.transcriber else {
            continue;
        };
        let seconds = audio.seconds();
        let started = Instant::now();
        let text = match t.text(&audio.at_whisper_rate()) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("patina-dictate: {e}");
                continue;
            }
        };
        engine.used = Instant::now();
        if verbose {
            eprintln!(
                "patina-dictate: {seconds:.1} s transcribed in {:.2?}",
                started.elapsed()
            );
        }
        if text.is_empty() {
            continue;
        }
        if let Err(e) = bus.call_method(Some(NAME), PATH, Some(NAME), "Commit", &(text,)) {
            eprintln!("patina-dictate: Commit: {e}");
        }
    }
}

/// Closes the overlay when dropped, however transcribing ends.
struct Closes(Option<Shown>);

impl Drop for Closes {
    fn drop(&mut self) {
        if let Some(shown) = self.0.take() {
            shown.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_goes_at_the_earlier_of_idle_and_fullscreen() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        // Idle only: ten minutes after it was used.
        assert_eq!(unload_at(t0, None, 10, 10), Some(t0 + s(600)));
        // In a fullscreen window: ten seconds after that began.
        let fs = t0 + s(100);
        assert_eq!(unload_at(t0, Some(fs), 10, 10), Some(fs + s(10)));
        // But the idle time may come first.
        assert_eq!(unload_at(t0, Some(t0 + s(595)), 30, 10), Some(t0 + s(600)));
        // 0 turns each off.
        assert_eq!(unload_at(t0, Some(fs), 0, 10), Some(t0 + s(600)));
        assert_eq!(unload_at(t0, Some(fs), 10, 0), Some(fs + s(10)));
        assert_eq!(unload_at(t0, Some(fs), 0, 0), None);
        assert_eq!(unload_at(t0, None, 10, 0), None);
    }
}

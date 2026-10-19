//! `--daemon`: dictation behind the dictate key (ADR-0072). The compositor
//! signals `DictationStarted` when the key goes down and `DictationStopped`
//! when it comes up, on `com.sectorflabs.Patina.Input`; this records
//! between them, transcribes on a thread of its own, so the next press
//! records at once, and calls `Commit` with the text, which the compositor
//! types into the focused window (ADR-0071).

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::audio::Audio;
use crate::capture::Recording;
use crate::transcribe::Transcriber;

const NAME: &str = "com.sectorflabs.Patina.Input";
const PATH: &str = "/com/sectorflabs/Patina/Input";

/// A press shorter than this was a slip, not dictation.
const SHORTEST: Duration = Duration::from_millis(300);

/// Listens until the session bus goes away.
pub fn run(model: &Path, language: Option<&str>, verbose: bool) -> Result<(), String> {
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(NAME)
        .and_then(|b| b.path(PATH))
        .map_err(|e| e.to_string())?
        .build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &bus, None)
        .map_err(|e| e.to_string())?;

    let (recorded, todo) = mpsc::channel::<Audio>();
    let committing = bus.clone();
    let model = model.to_path_buf();
    let language = language.map(String::from);
    std::thread::Builder::new()
        .name("transcribe".into())
        .spawn(move || transcribe(&todo, &model, language.as_deref(), verbose, &committing))
        .map_err(|e| e.to_string())?;

    let mut recording: Option<(Recording, Instant)> = None;
    for message in signals {
        let Ok(message) = message else { continue };
        let header = message.header();
        match header.member().map(|m| m.as_str()) {
            Some("DictationStarted") if recording.is_none() => match Recording::start() {
                Ok(r) => recording = Some((r, Instant::now())),
                Err(e) => eprintln!("patina-dictate: {e}"),
            },
            Some("DictationStopped") => {
                if let Some((r, started)) = recording.take() {
                    let audio = r.stop();
                    if started.elapsed() >= SHORTEST {
                        let _ = recorded.send(audio);
                    }
                }
            }
            _ => {}
        }
    }
    Err("the session bus went away".into())
}

/// Turns each recording into text and commits it. The model is loaded at
/// the start, and again at each recording until it loads, so one
/// downloaded later is used without a restart.
fn transcribe(
    todo: &mpsc::Receiver<Audio>,
    model: &Path,
    language: Option<&str>,
    verbose: bool,
    bus: &zbus::blocking::Connection,
) {
    let load = || {
        Transcriber::new(model, language, !verbose)
            .inspect_err(|e| eprintln!("patina-dictate: {e}"))
            .ok()
    };
    let mut transcriber = load();
    for audio in todo {
        if transcriber.is_none() {
            transcriber = load();
        }
        let Some(t) = &transcriber else { continue };
        let seconds = audio.seconds();
        let started = Instant::now();
        let text = match t.text(&audio.at_whisper_rate()) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("patina-dictate: {e}");
                continue;
            }
        };
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

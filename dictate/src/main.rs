//! patina-dictate: speech to text on the machine, with Whisper. As a
//! command it records until Enter (or the end of its input) and prints what
//! was said, or transcribes a WAV file. With `--daemon` it's dictation
//! behind a held key (Ctrl+Super+Alt), which the compositor starts and keeps
//! running, and the text is typed where the focus is.

mod audio;
mod capture;
mod daemon;
mod transcribe;

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use crate::transcribe::{Transcriber, model_path};

const USAGE: &str = "usage: patina-dictate [-v] [-m MODEL] [-l LANGUAGE] [FILE.wav]
       patina-dictate --daemon [-v] [-m MODEL] [-l LANGUAGE]

Records from the microphone until Enter, or the end of its input, and
prints what was said. Given a WAV file, it prints what's said in that.

With --daemon, it records while the compositor's dictate key
(Ctrl+Super+Alt) is held, and has what was said typed into the focused window.

  -m, --model MODEL       whisper.cpp's name for it (base.en, small,
                          large-v3-turbo), kept in the models folder as
                          ggml-MODEL.bin, or a path; base.en if not given
  -l, --language CODE     what's spoken (en, sv), or auto; English for an
                          English-only model, else auto
  -d, --daemon            dictation behind the dictate key
  -v, --verbose           whisper.cpp's log, and how long each step took

Models are kept in $XDG_DATA_HOME/patina-dictate/models.";

const DEFAULT_MODEL: &str = "base.en";

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    model: Option<String>,
    language: Option<String>,
    verbose: bool,
    daemon: bool,
    file: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
enum Asked {
    Help,
    Version,
    Dictate(Options),
}

fn parse(args: &[String]) -> Option<Asked> {
    let mut options = Options::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-h" | "--help" => return Some(Asked::Help),
            "-V" | "--version" => return Some(Asked::Version),
            "-v" | "--verbose" => options.verbose = true,
            "-d" | "--daemon" => options.daemon = true,
            "-m" | "--model" => options.model = Some(rest.next()?.clone()),
            "-l" | "--language" => options.language = Some(rest.next()?.clone()),
            flag if flag.starts_with('-') && flag != "-" => return None,
            file if options.file.is_none() => options.file = Some(PathBuf::from(file)),
            _ => return None,
        }
    }
    if options.daemon && options.file.is_some() {
        return None;
    }
    Some(Asked::Dictate(options))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Some(Asked::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(Asked::Version) => {
            println!("patina-dictate {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(Asked::Dictate(options)) => options,
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if options.daemon {
        let model = model_path(options.model.as_deref().unwrap_or(DEFAULT_MODEL));
        let ended = daemon::run(&model, options.language.as_deref(), options.verbose);
        if let Err(e) = ended {
            eprintln!("patina-dictate: {e}");
        }
        return ExitCode::FAILURE;
    }
    match dictate(&options) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("patina-dictate: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dictate(options: &Options) -> Result<String, String> {
    let verbose = options.verbose;
    let started = Instant::now();
    let model = model_path(options.model.as_deref().unwrap_or(DEFAULT_MODEL));
    let transcriber = Transcriber::new(&model, options.language.as_deref(), !verbose)?;
    if verbose {
        eprintln!("loaded {} in {:.2?}", model.display(), started.elapsed());
    }

    let audio = match &options.file {
        Some(file) => audio::read_wav(file).map_err(|e| format!("{}: {e}", file.display()))?,
        None => record()?,
    };
    if verbose {
        eprintln!(
            "{:.1} s of sound at {} Hz, loudest {:.4}",
            audio.seconds(),
            audio.rate,
            audio.loudest()
        );
    }

    let started = Instant::now();
    let text = transcriber.text(&audio.at_whisper_rate())?;
    if verbose {
        eprintln!("transcribed in {:.2?}", started.elapsed());
    }
    Ok(text)
}

/// Records until a line, or the end, of standard input.
fn record() -> Result<audio::Audio, String> {
    let recording = capture::Recording::start()?;
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("Listening. Press Enter to stop.");
    }
    let _ = stdin.lock().read_line(&mut String::new());
    Ok(recording.stop())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn reads_options_and_a_file() {
        assert_eq!(
            parse(&args("-v -m small -l sv talk.wav")),
            Some(Asked::Dictate(Options {
                model: Some("small".into()),
                language: Some("sv".into()),
                verbose: true,
                daemon: false,
                file: Some("talk.wav".into()),
            }))
        );
        assert_eq!(parse(&[]), Some(Asked::Dictate(Options::default())));
    }

    #[test]
    fn refuses_what_it_doesnt_know() {
        assert_eq!(parse(&args("-x")), None);
        assert_eq!(parse(&args("-m")), None);
        assert_eq!(parse(&args("a.wav b.wav")), None);
        assert_eq!(parse(&args("--daemon a.wav")), None, "it records from the mic");
    }
}

//! The speech models Dictation can use: the few worth choosing between, what
//! is on disk, and downloading one (with `curl`, from whisper.cpp's page on
//! Hugging Face, the URL patina-dictate's own error message gives).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A model Settings offers. `name` is whisper.cpp's, as patina-dictate's
/// `model` setting takes it: the file is `ggml-NAME.bin`.
pub struct Model {
    pub name: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    /// Its size on disk, in MB, give or take; shown, and the download's progress.
    pub megabytes: u64,
}

/// Roughly slowest and best last. Models with `.en` know English only, which
/// they do better than a multilingual one of their size; the large ones are
/// the most forgiving of accents and mumbling, and `-q5_0` is the same model
/// squeezed to a third of the size for a small loss.
pub const MODELS: [Model; 7] = [
    Model {
        name: "tiny.en",
        title: "Tiny (English)",
        about: "Very fast, makes the most mistakes. For trying things.",
        megabytes: 75,
    },
    Model {
        name: "base.en",
        title: "Base (English)",
        about: "Fast; the default. Fine for a clear voice.",
        megabytes: 142,
    },
    Model {
        name: "small.en",
        title: "Small (English)",
        about: "Better with accents and punctuation, still quick on a GPU.",
        megabytes: 466,
    },
    Model {
        name: "medium.en-q5_0",
        title: "Medium (English), compact",
        about: "A good step up for accents, at half the size of the full one.",
        megabytes: 515,
    },
    Model {
        name: "medium.en",
        title: "Medium (English)",
        about: "Accurate; slow without a GPU.",
        megabytes: 1533,
    },
    Model {
        name: "large-v3-turbo-q5_0",
        title: "Large v3 Turbo, compact",
        about: "The most forgiving of accents and mumbling that's still fast. Knows many languages.",
        megabytes: 574,
    },
    Model {
        name: "large-v3-turbo",
        title: "Large v3 Turbo",
        about: "As above, full precision.",
        megabytes: 1624,
    },
];

/// Where patina-dictate looks for models.
pub fn dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("patina-dictate/models"))
}

fn file(name: &str) -> Option<PathBuf> {
    Some(dir()?.join(format!("ggml-{name}.bin")))
}

pub fn installed(name: &str) -> bool {
    file(name).is_some_and(|f| f.is_file())
}

/// Removes a downloaded model.
pub fn delete(name: &str) -> Result<(), String> {
    let path = file(name).ok_or("there's no data folder")?;
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn url(name: &str) -> String {
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin")
}

/// A download in progress: curl writing `ggml-NAME.bin.part`, renamed to the
/// real name when it succeeds, so a half-downloaded file is never taken for a
/// model.
pub struct Download {
    pub name: &'static str,
    child: Child,
    part: PathBuf,
    done: PathBuf,
    expected: u64,
}

pub enum Progress {
    /// Fraction done, 0 to 1.
    Going(f32),
    Finished,
    Failed(String),
}

impl Download {
    pub fn start(model: &Model) -> Result<Self, String> {
        let done = file(model.name).ok_or("there's no data folder")?;
        std::fs::create_dir_all(done.parent().unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?;
        let part = done.with_extension("bin.part");
        let child = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(&part)
            .arg(url(model.name))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("couldn't run curl: {e}"))?;
        Ok(Self {
            name: model.name,
            child,
            part,
            done,
            expected: model.megabytes * 1_000_000,
        })
    }

    pub fn poll(&mut self) -> Progress {
        match self.child.try_wait() {
            Ok(None) => {
                let have = std::fs::metadata(&self.part).map_or(0, |m| m.len());
                // The sizes shown are approximate, so never claim to be done.
                Progress::Going((have as f32 / self.expected as f32).min(0.99))
            }
            Ok(Some(status)) if status.success() => match std::fs::rename(&self.part, &self.done) {
                Ok(()) => Progress::Finished,
                Err(e) => Progress::Failed(e.to_string()),
            },
            Ok(Some(_)) => {
                let mut said = String::new();
                if let Some(mut err) = self.child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut err, &mut said);
                }
                let _ = std::fs::remove_file(&self.part);
                Progress::Failed(said.trim().to_string())
            }
            Err(e) => Progress::Failed(e.to_string()),
        }
    }

    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.part);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_whisper_cpps() {
        assert_eq!(
            url("small.en"),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin"
        );
    }

    #[test]
    fn the_default_model_is_offered() {
        assert!(MODELS.iter().any(|m| m.name == "base.en"));
    }
}

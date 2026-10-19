//! Whisper, through whisper.cpp (whisper-rs): a model loaded once, then
//! sound in and text out.

use std::path::{Path, PathBuf};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Where models are kept: `$XDG_DATA_HOME/patina-dictate/models`.
pub fn models_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("patina-dictate/models")
}

/// A model named as whisper.cpp names them (`base.en`, `small`,
/// `large-v3-turbo`), found in [`models_dir`] as `ggml-NAME.bin`; or a path
/// to the file itself.
pub fn model_path(model: &str) -> PathBuf {
    if model.contains('/') || model.ends_with(".bin") {
        PathBuf::from(model)
    } else {
        models_dir().join(format!("ggml-{model}.bin"))
    }
}

/// Where whisper.cpp's models are downloaded from.
pub fn model_url(path: &Path) -> String {
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{file}")
}

pub struct Transcriber {
    context: WhisperContext,
    /// The spoken language as a code (`en`, `sv`), or `auto` to detect it.
    language: String,
}

impl Transcriber {
    /// Loads the model. `quiet` keeps whisper.cpp's own logging off stderr.
    pub fn new(model: &Path, language: Option<&str>, quiet: bool) -> Result<Self, String> {
        if quiet {
            whisper_rs::install_logging_hooks();
        }
        if !model.is_file() {
            return Err(format!(
                "no model at {}; download it from {}",
                model.display(),
                model_url(model)
            ));
        }
        let path = model.to_str().ok_or("the model's path isn't UTF-8")?;
        let context = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|e| format!("loading {}: {e}", model.display()))?;
        // English-only models (*.en) know one language; the rest detect it.
        let language = match language {
            Some(language) => language.to_string(),
            None if context.is_multilingual() => "auto".to_string(),
            None => "en".to_string(),
        };
        Ok(Self { context, language })
    }

    /// The text spoken in `samples`, 16 kHz mono.
    pub fn text(&self, samples: &[f32]) -> Result<String, String> {
        // Under a second, whisper.cpp says nothing useful (and warns), so pad
        // a short press with silence.
        let mut padded;
        let samples = if samples.len() < crate::audio::RATE as usize {
            padded = samples.to_vec();
            padded.resize(crate::audio::RATE as usize + 1, 0.0);
            &padded[..]
        } else {
            samples
        };
        let mut state = self.context.create_state().map_err(|e| e.to_string())?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_n_threads(threads());
        // Each dictation stands alone.
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, samples).map_err(|e| e.to_string())?;
        // On silence or noise Whisper makes something up ("you", "Thank
        // you."), but it knows: those segments are likely no speech.
        let text: String = state
            .as_iter()
            .filter(|segment| segment.no_speech_probability() < NO_SPEECH)
            .filter_map(|segment| segment.to_str_lossy().ok().map(|s| s.into_owned()))
            .collect();
        Ok(text.trim().to_string())
    }
}

/// Segments likelier than this to be no speech are dropped.
const NO_SPEECH: f32 = 0.6;

/// whisper.cpp gains little past eight threads.
fn threads() -> i32 {
    std::thread::available_parallelism().map_or(4, |n| n.get().min(8) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_looked_for_in_the_models_folder() {
        let path = model_path("base.en");
        assert!(path.ends_with("patina-dictate/models/ggml-base.en.bin"));
        assert_eq!(model_path("./mine.bin"), PathBuf::from("./mine.bin"));
        assert_eq!(model_path("/m/ggml-small"), PathBuf::from("/m/ggml-small"));
    }

    #[test]
    fn the_url_is_whisper_cpps() {
        assert_eq!(
            model_url(Path::new("/x/ggml-small.bin")),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
        );
    }
}

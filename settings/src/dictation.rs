//! The Dictation page (ADR-0073, ADR-0119): the key held to dictate, from the
//! compositor's `[keys]`, and patina-dictate's own config, which it reads at
//! each press: the speech model (downloaded here), the language, how Whisper
//! decodes, and where the overlay shows while dictating.

use std::path::PathBuf;

use egui::{RichText, Ui};
use toml_edit::{DocumentMut, value};

use crate::app::Message;
use crate::dictation_models::{self as models, Download, MODELS, Progress};

/// The overlay's edges: the config's value and what Settings calls it.
const EDGES: [(&str, &str); 2] = [
    ("bottom", "Bottom of the screen"),
    ("top", "Top of the screen"),
];

/// The compositor's default for `dictate` (`compositor/src/keys.rs`).
const DEFAULT_KEY: &str = "Ctrl+Super+Alt";

/// patina-dictate's config.
fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("patina-dictate/config.toml"))
}

/// The overlay's edge as `text` says it, or the default.
fn parse_edge(text: &str) -> &'static str {
    let doc: Option<DocumentMut> = text.parse().ok();
    let set = doc
        .as_ref()
        .and_then(|d| d.get("overlay")?.as_str().map(String::from));
    EDGES
        .iter()
        .map(|(v, _)| *v)
        .find(|v| set.as_deref() == Some(*v))
        .unwrap_or("bottom")
}

/// `text` with `overlay` set to `edge`, the rest as it was.
fn with_edge(text: &str, edge: &str) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("{e}"))?;
    doc["overlay"] = value(edge);
    Ok(doc.to_string())
}

/// The language choices: the config's value (empty for the default) and its name.
const LANGUAGES: [(&str, &str); 12] = [
    ("", "Default: English for English models, else detect"),
    ("en", "English"),
    ("auto", "Detect the language"),
    ("af", "Afrikaans"),
    ("sv", "Swedish"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("nl", "Dutch"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("ja", "Japanese"),
];

/// What patina-dictate uses with no `model` set.
const DEFAULT_MODEL: &str = "base.en";

/// The settings that are plain values in patina-dictate's config.
#[derive(Debug, Clone, PartialEq)]
struct Tuning {
    model: String,
    language: String,
    beam_size: i64,
    no_speech: f64,
    prompt: String,
    threads: i64,
    fallback: bool,
}

impl Tuning {
    fn parse(text: &str) -> Self {
        let doc: Option<DocumentMut> = text.parse().ok();
        let get = |key: &str| doc.as_ref().and_then(|d| d.get(key)).cloned();
        let string = |key: &str| {
            get(key)
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        };
        let model = string("model");
        Self {
            model: if model.trim().is_empty() {
                DEFAULT_MODEL.into()
            } else {
                model
            },
            language: string("language"),
            beam_size: get("beam_size")
                .and_then(|v| v.as_integer())
                .unwrap_or(1)
                .clamp(1, 8),
            no_speech: get("no_speech")
                .and_then(|v| v.as_float().or(v.as_integer().map(|i| i as f64)))
                .unwrap_or(0.6)
                .clamp(0.0, 1.0),
            prompt: string("prompt"),
            threads: get("threads")
                .and_then(|v| v.as_integer())
                .unwrap_or(0)
                .clamp(0, 64),
            fallback: get("fallback").and_then(|v| v.as_bool()).unwrap_or(true),
        }
    }
}

/// `text` with `key` set to `v`, the rest as it was.
fn with_value(text: &str, key: &str, v: impl Into<toml_edit::Value>) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("{e}"))?;
    doc[key] = value(v);
    Ok(doc.to_string())
}

/// The keys `dictate` has in the compositor's config `text`, as words.
fn dictate_key(text: &str) -> String {
    let doc: Option<DocumentMut> = text.parse().ok();
    let set = doc
        .as_ref()
        .and_then(|d| d.get("keys")?.get("dictate").cloned());
    let keys: Vec<String> = match &set {
        None => return DEFAULT_KEY.to_string(),
        Some(item) => match item.as_array() {
            Some(list) => list
                .iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect(),
            None => item.as_str().map(String::from).into_iter().collect(),
        },
    };
    let keys: Vec<String> = keys.into_iter().filter(|k| !k.trim().is_empty()).collect();
    if keys.is_empty() {
        String::new()
    } else {
        keys.join(" or ")
    }
}

pub struct DictationPage {
    edge: &'static str,
    /// The key held to dictate; empty if `[keys]` takes it away.
    key: String,
    tuning: Tuning,
    /// The model being downloaded, if one is.
    download: Option<(Download, f32)>,
}

impl DictationPage {
    pub fn new() -> Self {
        let read = |path: Option<PathBuf>| {
            path.and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default()
        };
        let config = read(path());
        Self {
            edge: parse_edge(&config),
            key: dictate_key(&read(crate::compositor_config::path())),
            tuning: Tuning::parse(&config),
            download: None,
        }
    }

    /// Writes `key` = `v` to the config and updates what's shown.
    fn save(&mut self, key: &str, v: impl Into<toml_edit::Value>) -> Result<(), String> {
        let path = path().ok_or("there's no config folder")?;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let text = with_value(&text, key, v)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, &text).map_err(|e| e.to_string())?;
        self.tuning = Tuning::parse(&text);
        Ok(())
    }

    fn set_edge(&mut self, edge: &'static str) -> Result<(), String> {
        let path = path().ok_or("there's no config folder")?;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let text = with_edge(&text, edge)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        self.edge = edge;
        Ok(())
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("Dictation");
        ui.add_space(8.0);
        let how = if self.key.is_empty() {
            "Dictation has no key: [keys] in the compositor's config takes it away.".to_string()
        } else {
            format!(
                "Hold {} and speak. When you let go, what you said is typed where you're \
                 typing. It's transcribed on this computer; nothing is sent anywhere.",
                self.key
            )
        };
        ui.label(how);
        ui.label(
            RichText::new("The key is dictate in [keys], in the compositor's config.")
                .small()
                .weak(),
        );
        ui.add_space(16.0);

        self.model_ui(ui, message);
        ui.add_space(16.0);
        self.language_ui(ui, message);
        ui.add_space(16.0);
        self.decoding_ui(ui, message);
        ui.add_space(16.0);

        ui.label(RichText::new("Overlay").strong());
        ui.label(
            RichText::new(
                "Shows your voice while you speak, then that the words are being transcribed.",
            )
            .small()
            .weak(),
        );
        for (edge, name) in EDGES {
            if ui.radio(self.edge == edge, name).clicked()
                && self.edge != edge
                && let Err(e) = self.set_edge(edge)
            {
                *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
            }
        }
    }
}

impl DictationPage {
    fn model_ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Speech model").strong());
        ui.label(
            RichText::new(
                "All of these are OpenAI's Whisper, run by whisper.cpp on this computer. Bigger \
                 ones cope better with accents and mumbling, and are slower and larger. Download \
                 one, then choose it; the next time you dictate it's used.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);

        // The download's progress, and what it ended in.
        if let Some((download, shown)) = &mut self.download {
            match download.poll() {
                Progress::Going(fraction) => {
                    *shown = fraction;
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(250));
                }
                Progress::Finished => {
                    let name = download.name;
                    self.download = None;
                    *message = Some(Message::note(format!("Downloaded {name}")));
                }
                Progress::Failed(why) => {
                    let name = download.name;
                    self.download = None;
                    *message = Some(Message::error(format!("Couldn't download {name}: {why}")));
                }
            }
        }

        let mut choose = None;
        let mut start = None;
        let mut remove = None;
        let mut cancel = false;
        for model in &MODELS {
            let have = models::installed(model.name);
            let downloading = self
                .download
                .as_ref()
                .filter(|(d, _)| d.name == model.name)
                .map(|(_, fraction)| *fraction);
            ui.horizontal(|ui| {
                let picked = self.tuning.model == model.name;
                let title = format!("Whisper {} ({} MB)", model.title, model.megabytes);
                if ui
                    .add_enabled(have, egui::RadioButton::new(picked, title))
                    .clicked()
                    && !picked
                {
                    choose = Some(model.name);
                }
                if let Some(fraction) = downloading {
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(120.0)
                            .show_percentage(),
                    );
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                } else if !have {
                    if ui
                        .add_enabled(self.download.is_none(), egui::Button::new("Download"))
                        .clicked()
                    {
                        start = Some(model);
                    }
                } else if !picked && ui.small_button("Delete").clicked() {
                    remove = Some(model.name);
                }
            });
            ui.label(RichText::new(model.about).small().weak());
        }
        let listed = MODELS.iter().find(|m| m.name == self.tuning.model);
        ui.label(
            RichText::new(match listed {
                Some(m) => format!(
                    "In use: Whisper {} (ggml-{}.bin)",
                    m.title, self.tuning.model
                ),
                None => format!("In use: {} (set in the config file)", self.tuning.model),
            })
            .small(),
        );
        if !models::installed(&self.tuning.model)
            && MODELS.iter().any(|m| m.name == self.tuning.model)
        {
            ui.label(
                RichText::new(format!(
                    "{} isn't downloaded, so dictation does nothing until it is.",
                    self.tuning.model
                ))
                .small()
                .color(ui.visuals().warn_fg_color),
            );
        }

        if let Some(name) = choose
            && let Err(e) = self.save("model", name)
        {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
        if let Some(model) = start {
            match Download::start(model) {
                Ok(d) => self.download = Some((d, 0.0)),
                Err(e) => *message = Some(Message::error(format!("Couldn't download: {e}"))),
            }
        }
        if cancel && let Some((d, _)) = self.download.take() {
            d.cancel();
        }
        if let Some(name) = remove
            && let Err(e) = models::delete(name)
        {
            *message = Some(Message::error(format!("Couldn't delete {name}: {e}")));
        }
    }

    fn language_ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Language").strong());
        ui.label(
            RichText::new(
                "What you speak. Naming it stops Whisper guessing; an English model \
                 only knows English.",
            )
            .small()
            .weak(),
        );
        let current = LANGUAGES
            .iter()
            .find(|(code, _)| *code == self.tuning.language)
            .map_or(self.tuning.language.as_str(), |(_, name)| *name)
            .to_string();
        let mut chosen = None;
        egui::ComboBox::from_id_salt("dictation-language")
            .selected_text(current)
            .width(320.0)
            .show_ui(ui, |ui| {
                for (code, name) in LANGUAGES {
                    if ui
                        .selectable_label(self.tuning.language == code, name)
                        .clicked()
                    {
                        chosen = Some(code);
                    }
                }
            });
        if let Some(code) = chosen
            && code != self.tuning.language
            && let Err(e) = self.save("language", code)
        {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }

    fn decoding_ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Accuracy").strong());
        ui.label(
            RichText::new("Try these when words come out wrong. Each applies at the next press.")
                .small()
                .weak(),
        );
        let mut t = self.tuning.clone();
        let mut failed = None;
        let mut save = |page: &mut Self, key: &str, v: toml_edit::Value| {
            if let Err(e) = page.save(key, v) {
                failed = Some(e);
            }
        };

        ui.horizontal(|ui| {
            ui.label("Search width");
            let r = ui.add(egui::Slider::new(&mut t.beam_size, 1..=8));
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                save(self, "beam_size", t.beam_size.into());
            }
        });
        ui.label(
            RichText::new(
                "1 takes Whisper's first idea. Higher weighs several and picks the best: \
                 steadier on mumbling and accents, slower. 5 is the usual step up.",
            )
            .small()
            .weak(),
        );

        ui.horizontal(|ui| {
            ui.label("Silence threshold");
            let r = ui.add(egui::Slider::new(&mut t.no_speech, 0.0..=1.0));
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                save(
                    self,
                    "no_speech",
                    ((t.no_speech * 100.0).round() / 100.0).into(),
                );
            }
        });
        ui.label(
            RichText::new(
                "Lower throws away more of what Whisper suspects isn't speech: fewer made-up \
                 \"Thank you.\"s, but quiet speech can go too. Higher keeps more.",
            )
            .small()
            .weak(),
        );

        if ui
            .checkbox(&mut t.fallback, "Retry unsure passages")
            .changed()
        {
            save(self, "fallback", t.fallback.into());
        }
        ui.label(
            RichText::new(
                "When a passage looks like babble, try it again with some randomness. Helps \
                 with mumbling; turn it off if the text changes between identical tries.",
            )
            .small()
            .weak(),
        );

        ui.horizontal(|ui| {
            ui.label("Threads");
            let r = ui.add(egui::DragValue::new(&mut t.threads).range(0..=64));
            if r.changed() {
                save(self, "threads", t.threads.into());
            }
            ui.label(RichText::new("0 is automatic").small().weak());
        });

        ui.add_space(4.0);
        ui.label("Words to expect");
        let r = ui.add(
            egui::TextEdit::multiline(&mut t.prompt)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Names and jargon, spelled the way you want them: Patina, Sector F Labs, Teleprinter"),
        );
        if r.changed() {
            save(self, "prompt", t.prompt.clone().into());
        }
        ui.label(
            RichText::new(
                "Whisper treats this as what was said just before, so it leans towards these \
                 words and this style. Written as a sentence, with punctuation, it also gets \
                 you punctuation.",
            )
            .small()
            .weak(),
        );

        // The text box owns its edits while it's being typed in.
        if r.has_focus() {
            self.tuning.prompt = t.prompt;
        }
        if let Some(e) = failed {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_is_at_the_bottom_unless_set() {
        assert_eq!(parse_edge(""), "bottom");
        assert_eq!(parse_edge("overlay = \"top\""), "top");
        assert_eq!(parse_edge("overlay = \"left\""), "bottom");
    }

    #[test]
    fn setting_the_edge_keeps_the_rest() {
        let text = "# mine\nmodel = \"small\"\n";
        let written = with_edge(text, "top").unwrap();
        assert!(
            written.starts_with("# mine\nmodel = \"small\"\n"),
            "{written}"
        );
        assert_eq!(parse_edge(&written), "top");
        assert_eq!(
            parse_edge(&with_edge(&written, "bottom").unwrap()),
            "bottom"
        );
    }

    #[test]
    fn the_tuning_reads_the_config_and_keeps_the_rest() {
        let t = Tuning::parse("");
        assert_eq!(t.model, DEFAULT_MODEL);
        assert_eq!((t.beam_size, t.threads, t.fallback), (1, 0, true));
        assert_eq!(t.no_speech, 0.6);
        let text = "overlay = \"top\"\nmodel = \"small.en\"\nbeam_size = 50\nno_speech = 1\n";
        let t = Tuning::parse(text);
        assert_eq!(
            (t.model.as_str(), t.beam_size, t.no_speech),
            ("small.en", 8, 1.0)
        );
        let written = with_value(text, "prompt", "Patina, Sector F").unwrap();
        assert!(written.starts_with("overlay = \"top\""), "{written}");
        assert_eq!(Tuning::parse(&written).prompt, "Patina, Sector F");
        let written = with_value(&written, "fallback", false).unwrap();
        assert!(!Tuning::parse(&written).fallback);
    }

    #[test]
    fn the_key_is_the_compositors() {
        assert_eq!(dictate_key(""), DEFAULT_KEY);
        assert_eq!(
            dictate_key("[keys]\nlauncher = \"Alt+Space\"\n"),
            DEFAULT_KEY
        );
        assert_eq!(dictate_key("[keys]\ndictate = \"Super+D\"\n"), "Super+D");
        assert_eq!(
            dictate_key("[keys]\ndictate = [\"Super+D\", \"F9\"]\n"),
            "Super+D or F9"
        );
        assert_eq!(dictate_key("[keys]\ndictate = \"\"\n"), "");
    }
}

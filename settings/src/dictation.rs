//! The Dictation page (ADR-0073): the key held to dictate, from the
//! compositor's `[keys]`, and where the overlay shows while dictating,
//! `overlay` in patina-dictate's config, which it reads at each press.

use std::path::PathBuf;

use egui::{RichText, Ui};
use toml_edit::{DocumentMut, value};

use crate::app::Message;

/// The overlay's edges: the config's value and what Settings calls it.
const EDGES: [(&str, &str); 2] = [("bottom", "Bottom of the screen"), ("top", "Top of the screen")];

/// The compositor's default for `dictate` (`compositor/src/keys.rs`).
const DEFAULT_KEY: &str = "Ctrl+Super+Alt";

/// patina-dictate's config.
fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("patina-dictate/config.toml"))
}

/// The overlay's edge as `text` says it, or the default.
fn parse_edge(text: &str) -> &'static str {
    let doc: Option<DocumentMut> = text.parse().ok();
    let set = doc.as_ref().and_then(|d| d.get("overlay")?.as_str().map(String::from));
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

/// The keys `dictate` has in the compositor's config `text`, as words.
fn dictate_key(text: &str) -> String {
    let doc: Option<DocumentMut> = text.parse().ok();
    let set = doc.as_ref().and_then(|d| d.get("keys")?.get("dictate").cloned());
    let keys: Vec<String> = match &set {
        None => return DEFAULT_KEY.to_string(),
        Some(item) => match item.as_array() {
            Some(list) => list.iter().filter_map(|k| k.as_str().map(String::from)).collect(),
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
}

impl DictationPage {
    pub fn new() -> Self {
        let read = |path: Option<PathBuf>| {
            path.and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default()
        };
        Self {
            edge: parse_edge(&read(path())),
            key: dictate_key(&read(crate::compositor_config::path())),
        }
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
        assert!(written.starts_with("# mine\nmodel = \"small\"\n"), "{written}");
        assert_eq!(parse_edge(&written), "top");
        assert_eq!(parse_edge(&with_edge(&written, "bottom").unwrap()), "bottom");
    }

    #[test]
    fn the_key_is_the_compositors() {
        assert_eq!(dictate_key(""), DEFAULT_KEY);
        assert_eq!(dictate_key("[keys]\nlauncher = \"Alt+Space\"\n"), DEFAULT_KEY);
        assert_eq!(dictate_key("[keys]\ndictate = \"Super+D\"\n"), "Super+D");
        assert_eq!(
            dictate_key("[keys]\ndictate = [\"Super+D\", \"F9\"]\n"),
            "Super+D or F9"
        );
        assert_eq!(dictate_key("[keys]\ndictate = \"\"\n"), "");
    }
}

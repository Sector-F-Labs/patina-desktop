//! Style, on the Appearance page: light or dark (ADR-0087). It's GNOME's
//! setting (`org.gnome.desktop.interface color-scheme`, through gsettings),
//! which xdg-desktop-portal hands on as `org.freedesktop.appearance
//! color-scheme`: Patina apps on the Default skin follow it
//! (`patina::appearance`), as do GTK 4, Firefox and others, live.

use egui::{RichText, Ui};

use crate::app::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// No preference, which apps take as light.
    Default,
    Light,
    Dark,
}

impl Scheme {
    const ALL: [Scheme; 3] = [Scheme::Default, Scheme::Light, Scheme::Dark];

    /// The value gsettings holds.
    fn value(self) -> &'static str {
        match self {
            Scheme::Default => "default",
            Scheme::Light => "prefer-light",
            Scheme::Dark => "prefer-dark",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Scheme::Default => "Default",
            Scheme::Light => "Light",
            Scheme::Dark => "Dark",
        }
    }

    fn parse(value: &str) -> Option<Scheme> {
        Scheme::ALL.into_iter().find(|s| s.value() == value)
    }
}

pub struct ColorSchemeSection {
    /// `None` where gsettings or the key is missing, so there's nothing to set.
    chosen: Option<Scheme>,
}

impl ColorSchemeSection {
    pub fn new() -> Self {
        Self { chosen: read() }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Style").strong());
        let Some(chosen) = &mut self.chosen else {
            ui.label(RichText::new("GNOME's color-scheme setting isn't installed.").weak());
            return;
        };
        let before = *chosen;
        ui.horizontal(|ui| {
            for scheme in Scheme::ALL {
                ui.selectable_value(chosen, scheme, scheme.name());
            }
        });
        ui.label(
            RichText::new("Light or dark, for apps that follow it; the skin above isn't changed.")
                .small()
                .weak(),
        );
        if *chosen != before
            && let Err(e) = write(*chosen)
        {
            *message = Some(Message::error(format!("Couldn't set the style: {e}")));
        }
    }
}

const SCHEMA: &str = "org.gnome.desktop.interface";

fn read() -> Option<Scheme> {
    let out = std::process::Command::new("gsettings")
        .args(["get", SCHEMA, "color-scheme"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    // An unknown value still means the key is there; show it as Default.
    let value = String::from_utf8_lossy(&out.stdout);
    Some(Scheme::parse(value.trim().trim_matches('\'')).unwrap_or(Scheme::Default))
}

fn write(scheme: Scheme) -> Result<(), String> {
    let status = std::process::Command::new("gsettings")
        .args(["set", SCHEMA, "color-scheme", scheme.value()])
        .status()
        .map_err(|e| format!("gsettings: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("gsettings: {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_it_writes() {
        for scheme in Scheme::ALL {
            assert_eq!(Scheme::parse(scheme.value()), Some(scheme));
        }
        assert_eq!(Scheme::parse("dark"), None);
    }
}

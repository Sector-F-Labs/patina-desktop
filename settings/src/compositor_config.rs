//! `[night_light]`, and the screen turning off (`[screen] off_after` and
//! `[lock] on_screen_off`), in the compositor's config (ADR-0019, ADR-0039,
//! ADR-0047), read and written in place: the rest of the file, its comments
//! included, stays as it is. The compositor notices the change and applies
//! it.

use std::path::PathBuf;

use toml_edit::{DocumentMut, value};

/// The defaults, as the compositor has them.
pub const FROM: (u32, u32) = (20, 0);
pub const TO: (u32, u32) = (6, 0);
pub const TEMPERATURE: u32 = 4000;
pub const NEUTRAL: u32 = 6500;
pub const WARMEST: u32 = 1700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Always,
    Schedule,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Always => "always",
            Mode::Schedule => "schedule",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NightLight {
    pub mode: Mode,
    /// Hours and minutes.
    pub from: (u32, u32),
    pub to: (u32, u32),
    pub temperature: u32,
}

impl Default for NightLight {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            from: FROM,
            to: TO,
            temperature: TEMPERATURE,
        }
    }
}

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patina-compositor").join("config.toml"))
}

/// "HH:MM".
pub fn time(text: &str) -> Option<(u32, u32)> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

pub fn format_time((h, m): (u32, u32)) -> String {
    format!("{h:02}:{m:02}")
}

/// `[night_light]` from the config's text, the defaults for what's missing
/// or wrong, as the compositor reads it.
pub fn parse(text: &str) -> NightLight {
    let mut night = NightLight::default();
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return night;
    };
    let Some(table) = doc.get("night_light") else {
        return night;
    };
    let text_of = |key: &str| table.get(key).and_then(|v| v.as_str());
    night.mode = match text_of("mode") {
        Some("always") => Mode::Always,
        Some("schedule") => Mode::Schedule,
        _ => Mode::Off,
    };
    if let Some(t) = text_of("from").and_then(time) {
        night.from = t;
    }
    if let Some(t) = text_of("to").and_then(time) {
        night.to = t;
    }
    if let Some(k) = table.get("temperature").and_then(|v| v.as_integer())
        && (WARMEST as i64..=NEUTRAL as i64).contains(&k)
    {
        night.temperature = k as u32;
    }
    night
}

/// `text` with `[night_light]` set to `night`, everything else kept.
pub fn with_night_light(text: &str, night: &NightLight) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let table = doc
        .entry("night_light")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("night_light in the compositor's config isn't a section")?;
    table["mode"] = value(night.mode.name());
    table["from"] = value(format_time(night.from));
    table["to"] = value(format_time(night.to));
    table["temperature"] = value(night.temperature as i64);
    Ok(doc.to_string())
}

/// The screen turning off without input, as the compositor has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    /// Seconds; 0 for never.
    pub off_after: u64,
    /// Lock when it turns off.
    pub lock: bool,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            off_after: 300,
            lock: true,
        }
    }
}

/// `[screen] off_after` and `[lock] on_screen_off` from the config's text,
/// the defaults for what's missing or wrong.
pub fn parse_screen(text: &str) -> Screen {
    let mut screen = Screen::default();
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return screen;
    };
    if let Some(s) = doc
        .get("screen")
        .and_then(|t| t.get("off_after"))
        .and_then(|v| v.as_integer())
        && s >= 0
    {
        screen.off_after = s as u64;
    }
    if let Some(lock) = doc
        .get("lock")
        .and_then(|t| t.get("on_screen_off"))
        .and_then(|v| v.as_bool())
    {
        screen.lock = lock;
    }
    screen
}

/// `text` with the screen's settings set to `screen`, everything else kept.
pub fn with_screen(text: &str, screen: &Screen) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    for (section, key, v) in [
        ("screen", "off_after", value(screen.off_after as i64)),
        ("lock", "on_screen_off", value(screen.lock)),
    ] {
        let table = doc
            .entry(section)
            .or_insert_with(toml_edit::table)
            .as_table_mut()
            .ok_or(format!(
                "{section} in the compositor's config isn't a section"
            ))?;
        table[key] = v;
    }
    Ok(doc.to_string())
}

fn read_text() -> Option<String> {
    path().and_then(|p| std::fs::read_to_string(p).ok())
}

/// Night Light's settings from the compositor's config.
pub fn read() -> NightLight {
    read_text().map(|t| parse(&t)).unwrap_or_default()
}

/// The screen's settings from the compositor's config.
pub fn read_screen() -> Screen {
    read_text().map(|t| parse_screen(&t)).unwrap_or_default()
}

/// Writes Night Light's settings into the compositor's config, making it if
/// need be.
pub fn write(night: &NightLight) -> Result<(), String> {
    update(|text| with_night_light(text, night))
}

/// Writes the screen's settings into the compositor's config.
pub fn write_screen(screen: &Screen) -> Result<(), String> {
    update(|text| with_screen(text, screen))
}

/// Changes the compositor's config with `edit`, making it if need be.
fn update(edit: impl FnOnce(&str) -> Result<String, String>) -> Result<(), String> {
    let path = path().ok_or("no config folder")?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let text = edit(&text)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Written beside it and renamed over it, so the compositor never reads
    // half a file.
    let partial = path.with_extension("toml.partial");
    std::fs::write(&partial, text)
        .and_then(|()| std::fs::rename(&partial, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTER: &str = "# Patina compositor settings.\n\nscale = 1.5\n\n[night_light]\n# How warm.\n# temperature = 4000\n\n[lock]\n# command = \"patina-lock\"\n";

    #[test]
    fn writing_keeps_the_rest_and_reads_back() {
        let night = NightLight {
            mode: Mode::Schedule,
            from: (21, 30),
            to: (7, 0),
            temperature: 3200,
        };
        let text = with_night_light(STARTER, &night).unwrap();
        assert!(text.starts_with("# Patina compositor settings.\n\nscale = 1.5\n"));
        assert!(text.contains("# How warm."));
        assert!(text.contains("[lock]\n# command = \"patina-lock\""));
        assert!(text.contains("from = \"21:30\""));
        assert_eq!(parse(&text), night);
        // Again, changing it, doesn't add a second section.
        let text = with_night_light(&text, &NightLight::default()).unwrap();
        assert_eq!(text.matches("[night_light]").count(), 1);
        assert_eq!(parse(&text), NightLight::default());
    }

    #[test]
    fn the_screen_is_written_into_its_sections_keeping_the_rest() {
        assert_eq!(parse_screen(STARTER), Screen::default());
        let screen = Screen {
            off_after: 600,
            lock: true,
        };
        let text = with_screen(STARTER, &screen).unwrap();
        assert!(text.contains("[lock]\non_screen_off = true"), "{text}");
        assert!(text.contains("[screen]\noff_after = 600"), "{text}");
        assert!(text.contains("[night_light]\n# How warm."));
        assert!(text.contains("# command = \"patina-lock\""));
        assert_eq!(parse_screen(&text), screen);
        let never = Screen {
            off_after: 0,
            lock: false,
        };
        let text = with_screen(&text, &never).unwrap();
        assert_eq!(text.matches("[screen]").count(), 1);
        assert_eq!(parse_screen(&text), never);
        assert_eq!(
            parse_screen("[screen]\noff_after = -5\n"),
            Screen::default()
        );
    }

    #[test]
    fn an_empty_file_gets_the_section() {
        let text = with_night_light("", &NightLight::default()).unwrap();
        assert!(text.contains("[night_light]\nmode = \"off\""), "{text}");
    }

    #[test]
    fn bad_values_read_as_the_defaults() {
        let n = parse("[night_light]\nmode = \"sometimes\"\nfrom = \"25:00\"\ntemperature = 100\n");
        assert_eq!(n, NightLight::default());
        assert_eq!(time("7:05"), Some((7, 5)));
        assert_eq!(time("07:60"), None);
        assert_eq!(format_time((7, 5)), "07:05");
    }
}

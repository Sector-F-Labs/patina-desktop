//! `[night_light]`, the screen turning off (`[screen] off_after` and
//! `[lock] on_screen_off`), gaps, and the keyboard's layouts and key repeat,
//! in the compositor's config (ADR-0019, ADR-0039, ADR-0047, ADR-0051,
//! ADR-0068), read and written in place: the rest of the file, its comments
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
    /// Off while a fullscreen window (a game, a video) has the keyboard.
    pub pause_in_fullscreen: bool,
}

impl Default for NightLight {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            from: FROM,
            to: TO,
            temperature: TEMPERATURE,
            pause_in_fullscreen: false,
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
    if let Some(pause) = table.get("pause_in_fullscreen").and_then(|v| v.as_bool()) {
        night.pause_in_fullscreen = pause;
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
    table["pause_in_fullscreen"] = value(night.pause_in_fullscreen);
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

/// Gaps (ADR-0051), as the compositor has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gaps {
    pub enabled: bool,
    pub size: u32,
    /// The floating bar's corners.
    pub radius: u32,
    /// With gaps on, an app's maximise fills inside them (ADR-0093); false lets it
    /// maximise edge to edge (ADR-0130).
    pub maximise_inside: bool,
}

/// The largest gap and radius the compositor takes.
pub const MOST_GAP: u32 = 64;

impl Default for Gaps {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 8,
            radius: 8,
            maximise_inside: true,
        }
    }
}

/// `[gaps]` from the config's text, the defaults for what's missing or
/// wrong.
pub fn parse_gaps(text: &str) -> Gaps {
    let mut gaps = Gaps::default();
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return gaps;
    };
    let Some(table) = doc.get("gaps") else {
        return gaps;
    };
    if let Some(on) = table.get("enabled").and_then(|v| v.as_bool()) {
        gaps.enabled = on;
    }
    if let Some(inside) = table.get("maximise_inside").and_then(|v| v.as_bool()) {
        gaps.maximise_inside = inside;
    }
    for (key, field) in [("size", &mut gaps.size), ("radius", &mut gaps.radius)] {
        if let Some(n) = table.get(key).and_then(|v| v.as_integer())
            && (0..=MOST_GAP as i64).contains(&n)
        {
            *field = n as u32;
        }
    }
    gaps
}

/// `text` with `[gaps]` set to `gaps`, everything else kept.
pub fn with_gaps(text: &str, gaps: &Gaps) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let table = doc
        .entry("gaps")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("gaps in the compositor's config isn't a section")?;
    table["enabled"] = value(gaps.enabled);
    table["size"] = value(gaps.size as i64);
    table["radius"] = value(gaps.radius as i64);
    table["maximise_inside"] = value(gaps.maximise_inside);
    Ok(doc.to_string())
}

/// Whether the compositor draws the shadows windows ask for (ADR-0112).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shadows {
    pub enabled: bool,
}

impl Default for Shadows {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `[shadows]` from the config's text, on when it's missing or wrong.
pub fn parse_shadows(text: &str) -> Shadows {
    let mut shadows = Shadows::default();
    if let Ok(doc) = text.parse::<DocumentMut>()
        && let Some(on) = doc
            .get("shadows")
            .and_then(|t| t.get("enabled"))
            .and_then(|v| v.as_bool())
    {
        shadows.enabled = on;
    }
    shadows
}

/// `text` with `[shadows]` set to `shadows`, everything else kept.
pub fn with_shadows(text: &str, shadows: &Shadows) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let table = doc
        .entry("shadows")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("shadows in the compositor's config isn't a section")?;
    table["enabled"] = value(shadows.enabled);
    Ok(doc.to_string())
}

/// The clipboard history and screenshots on the clipboard (ADR-0114, ADR-0116).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clipboard {
    pub enabled: bool,
    pub screenshots: bool,
    pub max_entries: u32,
    /// Seconds a copy is kept for; 0 for as long as there's room.
    pub expire_after: u64,
}

/// The compositor's limits on `max_entries`.
pub const CLIPBOARD_ENTRIES: std::ops::RangeInclusive<u32> = 1..=200;

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            enabled: true,
            screenshots: true,
            max_entries: 50,
            expire_after: 7 * 24 * 3600,
        }
    }
}

/// `[clipboard]` from the config's text; the defaults for what's missing or wrong.
pub fn parse_clipboard(text: &str) -> Clipboard {
    let mut clipboard = Clipboard::default();
    let Some(table) = text
        .parse::<DocumentMut>()
        .ok()
        .and_then(|d| d.get("clipboard").cloned())
    else {
        return clipboard;
    };
    let flag = |key: &str| table.get(key).and_then(|v| v.as_bool());
    clipboard.enabled = flag("enabled").unwrap_or(clipboard.enabled);
    clipboard.screenshots = flag("screenshots").unwrap_or(clipboard.screenshots);
    if let Some(n) = table
        .get("max_entries")
        .and_then(|v| v.as_integer())
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| CLIPBOARD_ENTRIES.contains(n))
    {
        clipboard.max_entries = n;
    }
    if let Some(n) = table
        .get("expire_after")
        .and_then(|v| v.as_integer())
        .and_then(|n| u64::try_from(n).ok())
    {
        clipboard.expire_after = n;
    }
    clipboard
}

/// `text` with `[clipboard]` set to `clipboard`, everything else kept.
pub fn with_clipboard(text: &str, clipboard: &Clipboard) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let table = doc
        .entry("clipboard")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("clipboard in the compositor's config isn't a section")?;
    table["enabled"] = value(clipboard.enabled);
    table["screenshots"] = value(clipboard.screenshots);
    table["max_entries"] = value(i64::from(clipboard.max_entries));
    table["expire_after"] = value(i64::try_from(clipboard.expire_after).unwrap_or(i64::MAX));
    Ok(doc.to_string())
}

/// `[keyboard]`'s layouts and key repeat, as the compositor has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyboard {
    /// xkb layout and variant names, in Ctrl+Space's order; none when the
    /// config leaves them to the system's (/etc/default/keyboard).
    pub layouts: Vec<(String, String)>,
    /// Milliseconds before a held key repeats.
    pub repeat_delay: u32,
    /// Repeats per second.
    pub repeat_rate: u32,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            layouts: Vec::new(),
            repeat_delay: 200,
            repeat_rate: 25,
        }
    }
}

/// `[keyboard]` from the config's text, the defaults for what's missing or
/// wrong.
pub fn parse_keyboard(text: &str) -> Keyboard {
    let mut keyboard = Keyboard::default();
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return keyboard;
    };
    let Some(table) = doc.get("keyboard") else {
        return keyboard;
    };
    let text_of = |key: &str| table.get(key).and_then(|v| v.as_str()).unwrap_or("");
    let layout = text_of("layout");
    if !layout.trim().is_empty() {
        let mut variants = text_of("variant").split(',');
        keyboard.layouts = layout
            .split(',')
            .map(|code| {
                let variant = variants.next().unwrap_or("").trim();
                (code.trim().to_string(), variant.to_string())
            })
            .collect();
    }
    for (key, field) in [
        ("repeat_delay", &mut keyboard.repeat_delay),
        ("repeat_rate", &mut keyboard.repeat_rate),
    ] {
        if let Some(n) = table.get(key).and_then(|v| v.as_integer())
            && n >= 0
        {
            *field = n as u32;
        }
    }
    keyboard
}

/// `text` with `[keyboard]`'s layouts and key repeat set to `keyboard`,
/// everything else kept. No layouts takes them out, leaving the system's.
pub fn with_keyboard(text: &str, keyboard: &Keyboard) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let table = doc
        .entry("keyboard")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("keyboard in the compositor's config isn't a section")?;
    if keyboard.layouts.is_empty() {
        table.remove("layout");
        table.remove("variant");
    } else {
        let join = |part: fn(&(String, String)) -> &str| {
            keyboard
                .layouts
                .iter()
                .map(part)
                .collect::<Vec<_>>()
                .join(",")
        };
        table["layout"] = value(join(|(code, _)| code));
        table["variant"] = value(join(|(_, variant)| variant));
    }
    table["repeat_delay"] = value(keyboard.repeat_delay as i64);
    table["repeat_rate"] = value(keyboard.repeat_rate as i64);
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

/// The gaps from the compositor's config.
pub fn read_gaps() -> Gaps {
    read_text().map(|t| parse_gaps(&t)).unwrap_or_default()
}

/// The shadows setting from the compositor's config.
pub fn read_shadows() -> Shadows {
    read_text().map(|t| parse_shadows(&t)).unwrap_or_default()
}

/// The clipboard's settings from the compositor's config.
pub fn read_clipboard() -> Clipboard {
    read_text().map(|t| parse_clipboard(&t)).unwrap_or_default()
}

/// Writes the clipboard's settings into the compositor's config.
pub fn write_clipboard(clipboard: &Clipboard) -> Result<(), String> {
    update(|text| with_clipboard(text, clipboard))
}

/// The keyboard's settings from the compositor's config.
pub fn read_keyboard() -> Keyboard {
    read_text().map(|t| parse_keyboard(&t)).unwrap_or_default()
}

/// Writes the keyboard's settings into the compositor's config.
pub fn write_keyboard(keyboard: &Keyboard) -> Result<(), String> {
    update(|text| with_keyboard(text, keyboard))
}

/// Writes the gaps into the compositor's config.
pub fn write_gaps(gaps: &Gaps) -> Result<(), String> {
    update(|text| with_gaps(text, gaps))
}

/// Writes the shadows setting into the compositor's config.
pub fn write_shadows(shadows: &Shadows) -> Result<(), String> {
    update(|text| with_shadows(text, shadows))
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
            pause_in_fullscreen: true,
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
    fn gaps_are_written_keeping_the_rest() {
        assert_eq!(parse_gaps(STARTER), Gaps::default());
        let gaps = Gaps {
            enabled: true,
            size: 12,
            radius: 4,
            maximise_inside: false,
        };
        let text = with_gaps(STARTER, &gaps).unwrap();
        assert!(
            text.contains("[gaps]\nenabled = true\nsize = 12\nradius = 4\nmaximise_inside = false"),
            "{text}"
        );
        assert!(text.contains("[night_light]\n# How warm."));
        assert_eq!(parse_gaps(&text), gaps);
        let text = with_gaps(&text, &Gaps::default()).unwrap();
        assert_eq!(text.matches("[gaps]").count(), 1);
        assert_eq!(parse_gaps(&text), Gaps::default());
        assert_eq!(parse_gaps("[gaps]\nsize = 500\n"), Gaps::default());
    }

    #[test]
    fn the_clipboard_is_written_keeping_the_rest() {
        assert_eq!(parse_clipboard(STARTER), Clipboard::default());
        let set = Clipboard {
            enabled: false,
            screenshots: true,
            max_entries: 20,
            expire_after: 3600,
        };
        let text = with_clipboard(STARTER, &set).unwrap();
        assert!(text.contains("[clipboard]\nenabled = false"), "{text}");
        assert!(text.contains("scale = 1.5"), "{text}");
        assert_eq!(parse_clipboard(&text), set);
        let text = with_clipboard(&text, &Clipboard::default()).unwrap();
        assert_eq!(text.matches("[clipboard]").count(), 1);
        assert_eq!(parse_clipboard(&text), Clipboard::default());
        // What the compositor would refuse falls back to its defaults.
        assert_eq!(
            parse_clipboard("[clipboard]\nmax_entries = 900\nenabled = 3\nexpire_after = -5\n"),
            Clipboard::default()
        );
    }

    #[test]
    fn shadows_are_written_keeping_the_rest() {
        assert_eq!(parse_shadows(STARTER), Shadows { enabled: true });
        let off = Shadows { enabled: false };
        let text = with_shadows(STARTER, &off).unwrap();
        assert!(text.contains("[shadows]\nenabled = false"), "{text}");
        assert!(text.contains("[night_light]\n# How warm."));
        assert_eq!(parse_shadows(&text), off);
        let text = with_shadows(&text, &Shadows::default()).unwrap();
        assert_eq!(text.matches("[shadows]").count(), 1);
        assert_eq!(parse_shadows(&text), Shadows::default());
        assert_eq!(
            parse_shadows("[shadows]\nenabled = 3\n"),
            Shadows::default()
        );
    }

    #[test]
    fn the_layouts_are_written_as_xkbs_lists_keeping_the_rest() {
        assert_eq!(parse_keyboard(STARTER), Keyboard::default());
        let keyboard = Keyboard {
            layouts: vec![
                ("us".into(), String::new()),
                ("se".into(), "nodeadkeys".into()),
            ],
            repeat_delay: 300,
            repeat_rate: 30,
        };
        let text = with_keyboard(STARTER, &keyboard).unwrap();
        assert!(
            text.contains("[keyboard]\nlayout = \"us,se\"\nvariant = \",nodeadkeys\""),
            "{text}"
        );
        assert!(text.contains("[night_light]\n# How warm."));
        assert_eq!(parse_keyboard(&text), keyboard);
        // Back to the system's: the lists go, the rest stays.
        let system = Keyboard {
            layouts: Vec::new(),
            ..keyboard
        };
        let text = with_keyboard(&text, &system).unwrap();
        assert!(!text.contains("layout ="), "{text}");
        assert_eq!(text.matches("[keyboard]").count(), 1);
        assert_eq!(parse_keyboard(&text), system);
        // A variant list shorter than the layouts' leaves the rest plain.
        assert_eq!(
            parse_keyboard("[keyboard]\nlayout = \"us,ru\"\n").layouts,
            [("us".into(), String::new()), ("ru".into(), String::new())]
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

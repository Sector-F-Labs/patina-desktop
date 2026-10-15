//! `~/.config/patina-compositor/config.toml` (ADR-0019). Every key is
//! optional; a missing file is the defaults. A broken file never stops the
//! session: at start it falls back to the defaults, and on a reload
//! (Super+Shift+R) the settings in use stay. Unknown keys are warnings, as
//! in patina-bar's config and Patina's skins.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The output's scale; unset, chosen from the panel's density (or the
    /// parent desktop's scale when nested). `PATINA_SCALE` overrides it.
    pub scale: Option<f64>,
    pub keyboard: Keyboard,
    pub touchpad: Touchpad,
    pub cursor: Cursor,
    pub media: Media,
    pub lock: Lock,
    /// What tapping Super on its own runs; "" for nothing.
    pub launcher: String,
    /// What a right click on the desktop runs, with `--at X,Y` (the
    /// pointer) added; "" for nothing.
    pub desktop_menu: String,
    /// Key combinations ("Alt+T") and the commands they run. A [bindings]
    /// section replaces these defaults.
    pub bindings: std::collections::BTreeMap<String, String>,
}

/// A key combination from [bindings], ready to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub logo: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    /// The key's base keysym (lower case, unshifted).
    pub sym: u32,
}

/// Reads "Super+Shift+Return": modifiers, then one key, by its xkb name.
pub fn combo(text: &str) -> Result<Combo, String> {
    let mut combo = Combo {
        logo: false,
        alt: false,
        ctrl: false,
        shift: false,
        sym: 0,
    };
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key = parts
        .pop()
        .filter(|k| !k.is_empty())
        .ok_or(format!("{text:?} has no key"))?;
    for m in parts {
        match m.to_ascii_lowercase().as_str() {
            "super" | "logo" | "mod4" => combo.logo = true,
            "alt" | "mod1" => combo.alt = true,
            "ctrl" | "control" => combo.ctrl = true,
            "shift" => combo.shift = true,
            other => return Err(format!("{text:?}: unknown modifier {other:?}")),
        }
    }
    use smithay::input::keyboard::xkb;
    let sym = xkb::keysym_from_name(key, xkb::KEYSYM_CASE_INSENSITIVE);
    if sym.raw() == 0 {
        return Err(format!("{text:?}: unknown key {key:?}"));
    }
    combo.sym = sym.raw();
    Ok(combo)
}

fn default_bindings() -> std::collections::BTreeMap<String, String> {
    [("Alt+T", "teleprinter"), ("Alt+B", "firefox")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl Config {
    /// The [bindings] that parse, with the commands they run.
    pub fn combos(&self) -> Vec<(Combo, String)> {
        self.bindings
            .iter()
            .filter_map(|(k, v)| combo(k).ok().map(|c| (c, v.clone())))
            .collect()
    }
}

/// The lock screen (ADR-0025).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Lock {
    /// An ext-session-lock locker, run through sh(1) by Ctrl+Super+Q,
    /// Super+Escape and before sleep; "" for none.
    pub command: String,
    /// Lock before the machine sleeps (the lid, or a suspend).
    pub on_suspend: bool,
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            command: "patina-lock".into(),
            on_suspend: true,
        }
    }
}

/// The brightness and volume keys (ADR-0020).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Media {
    /// Percent of the backlight's range per press.
    pub brightness_step: u32,
    /// Commands, run through sh(1); empty does nothing.
    pub volume_up: String,
    pub volume_down: String,
    pub mute: String,
    pub mic_mute: String,
    /// The volume menu, flashed (with --flash) after a volume key; ""
    /// for none.
    pub volume_menu: String,
}

impl Default for Media {
    fn default() -> Self {
        Self {
            brightness_step: 5,
            volume_up: "wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+".into(),
            volume_down: "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-".into(),
            mute: "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle".into(),
            mic_mute: "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle".into(),
            volume_menu: "patina-volume".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Keyboard {
    /// xkb names; an unset layout means the system's (/etc/default/keyboard).
    pub layout: String,
    pub variant: String,
    pub model: String,
    pub options: String,
    /// Milliseconds before a held key repeats.
    pub repeat_delay: i32,
    /// Repeats per second.
    pub repeat_rate: i32,
    /// What Caps Lock does, added to the xkb options in effect.
    pub caps_lock: CapsLock,
}

/// What the Caps Lock key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsLock {
    /// Caps Lock, as the key says.
    #[default]
    #[serde(rename = "caps_lock")]
    Lock,
    /// Escape, as vim users like.
    Escape,
    /// Another Ctrl.
    Ctrl,
    /// Escape, and Escape becomes Caps Lock.
    SwapEscape,
}

impl CapsLock {
    /// The xkb option that does it, if any.
    pub fn xkb_option(self) -> Option<&'static str> {
        match self {
            CapsLock::Lock => None,
            CapsLock::Escape => Some("caps:escape"),
            CapsLock::Ctrl => Some("ctrl:nocaps"),
            CapsLock::SwapEscape => Some("caps:swapescape"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Touchpad {
    /// Tap to click; libinput leaves it off.
    pub tap: bool,
    /// Content follows the fingers, as on macOS and phones. On by default.
    pub natural_scroll: bool,
    /// Pointer speed, -1.0 (slowest) to 1.0 (fastest); libinput's default
    /// is 0.
    pub speed: f64,
    /// Three fingers moving drag, as macOS's accessibility setting has it
    /// (ADR-0031). On by default.
    pub three_finger_drag: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: None,
            keyboard: Keyboard::default(),
            touchpad: Touchpad::default(),
            cursor: Cursor::default(),
            media: Media::default(),
            lock: Lock::default(),
            launcher: "patina-launcher".into(),
            desktop_menu: "patina-menu".into(),
            bindings: default_bindings(),
        }
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            layout: String::new(),
            variant: String::new(),
            model: String::new(),
            options: String::new(),
            repeat_delay: 200,
            repeat_rate: 25,
            caps_lock: CapsLock::default(),
        }
    }
}

/// The pointer's look (ADR-0032).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Cursor {
    /// An installed cursor theme (as in /usr/share/icons) to use instead
    /// of Patina's own; "" for Patina's.
    pub theme: String,
    /// Its size in pixels at scale 1, as XCURSOR_SIZE.
    pub size: u32,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            theme: String::new(),
            size: 24,
        }
    }
}

impl Default for Touchpad {
    fn default() -> Self {
        Self {
            tap: true,
            natural_scroll: true,
            speed: 0.0,
            three_finger_drag: true,
        }
    }
}

/// The file written on first start where there's none: every setting,
/// commented out at its default.
pub const STARTER: &str = include_str!("../config.example.toml");

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patina-compositor").join("config.toml"))
}

/// The config at `path`, or what's wrong with it. A missing file is the
/// defaults. Warnings (unknown keys, values out of range) come with it.
pub fn load(path: &Path) -> Result<(Config, Vec<String>), String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Config::default(), Vec::new())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn parse(text: &str) -> Result<(Config, Vec<String>), String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    let mut config: Config = serde_ignored::deserialize(toml::Value::Table(table), |path| {
        warnings.push(format!("unknown key `{path}`, ignored"))
    })
    .map_err(|e| e.to_string())?;
    if let Some(scale) = config.scale
        && !(0.5..=4.0).contains(&scale)
    {
        warnings.push(format!(
            "scale {scale} is outside 0.5 to 4; using the automatic one"
        ));
        config.scale = None;
    }
    if !(-1.0..=1.0).contains(&config.touchpad.speed) {
        warnings.push(format!(
            "touchpad speed {} is outside -1 to 1; using 0",
            config.touchpad.speed
        ));
        config.touchpad.speed = 0.0;
    }
    for key in config.bindings.keys() {
        if let Err(e) = combo(key) {
            warnings.push(format!("binding {e}; ignored"));
        }
    }
    if !(1..=50).contains(&config.media.brightness_step) {
        warnings.push(format!(
            "brightness step {} is outside 1 to 50; using 5",
            config.media.brightness_step
        ));
        config.media.brightness_step = 5;
    }
    if config.keyboard.repeat_rate < 0 || config.keyboard.repeat_delay < 0 {
        warnings.push("keyboard repeat values can't be negative; using the defaults".into());
        config.keyboard.repeat_rate = Keyboard::default().repeat_rate;
        config.keyboard.repeat_delay = Keyboard::default().repeat_delay;
    }
    Ok((config, warnings))
}

/// The scale to use: `PATINA_SCALE`, then the config, then `automatic`.
pub fn scale(config: &Config, automatic: f64) -> f64 {
    std::env::var("PATINA_SCALE")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| (0.5..=4.0).contains(s))
        .or(config.scale)
        .unwrap_or(automatic)
}

/// Writes the starter file at `path` if nothing is there, so the settings
/// can be found. It never replaces a file.
pub fn write_starter(path: &Path) -> std::io::Result<bool> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(STARTER.as_bytes()).map(|()| true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

/// The config file, or the defaults with what was wrong, for start-up. On
/// the first start it writes the starter file.
pub fn load_or_default() -> Config {
    let Some(path) = path() else {
        return Config::default();
    };
    match write_starter(&path) {
        Ok(true) => eprintln!(
            "patina-compositor: wrote {}, every setting at its default",
            path.display()
        ),
        Ok(false) => {}
        Err(e) => eprintln!("patina-compositor: can't write {}: {e}", path.display()),
    }
    match load(&path) {
        Ok((config, warnings)) => {
            for w in warnings {
                eprintln!("patina-compositor: {}: {w}", path.display());
            }
            config
        }
        Err(e) => {
            eprintln!(
                "patina-compositor: {}: {e}; using the defaults",
                path.display()
            );
            Config::default()
        }
    }
}

impl crate::state::State {
    /// Reads config.toml again and applies it: Super+Shift+R. If the file
    /// has an error, the settings in use stay.
    pub fn reload_config(&mut self) {
        let Some(path) = path() else { return };
        match load(&path) {
            Ok((config, warnings)) => {
                for w in warnings {
                    eprintln!("patina-compositor: {}: {w}", path.display());
                }
                self.apply_config(config);
                eprintln!("patina-compositor: reloaded {}", path.display());
            }
            Err(e) => eprintln!("patina-compositor: {}: {e}; not reloaded", path.display()),
        }
    }

    fn apply_config(&mut self, config: Config) {
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        let settings = crate::keyboard::KeyboardSettings::load(&config.keyboard);
        if let Err(e) = keyboard.set_xkb_config(self, settings.config()) {
            eprintln!(
                "patina-compositor: keyboard layout {:?}: {e:?}; keeping the current one",
                settings.layout
            );
        }
        keyboard.change_repeat_info(config.keyboard.repeat_rate, config.keyboard.repeat_delay);

        let new_scale = scale(&config, self.auto_scale);
        if new_scale != self.output.current_scale().fractional_scale() {
            self.set_scale(new_scale);
        }
        if let Some(u) = self.udev.as_mut() {
            u.configure_touchpads(&config.touchpad);
            if config.cursor != self.config.cursor {
                u.reload_cursors(
                    &config.cursor,
                    self.output.current_scale().fractional_scale(),
                );
            }
        }
        self.config = config;
        self.needs_redraw = true;
    }

    /// Changes the output's scale, and everything that follows from it.
    pub fn set_scale(&mut self, scale: f64) {
        self.output.change_current_state(
            None,
            None,
            Some(smithay::output::Scale::Fractional(scale)),
            None,
        );
        smithay::desktop::layer_map_for_output(&self.output).arrange();
        self.relayout_maximized();
        self.update_preferred_scale();
        if let Some(u) = self.udev.as_mut() {
            u.reload_cursors(&self.config.cursor, scale);
        }
        self.needs_redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_the_defaults() {
        let (c, w) = parse("").unwrap();
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
        assert!(c.touchpad.tap);
        assert!(c.touchpad.natural_scroll);
        assert_eq!(c.keyboard.repeat_rate, 25);
    }

    #[test]
    fn reads_every_section() {
        let (c, w) = parse(
            "scale = 1.25\n[keyboard]\nlayout = \"us,se\"\nrepeat_rate = 40\ncaps_lock = \"escape\"\n[touchpad]\nnatural_scroll = false\nspeed = 0.3\n",
        )
        .unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.scale, Some(1.25));
        assert_eq!(c.keyboard.layout, "us,se");
        assert_eq!(c.keyboard.repeat_rate, 40);
        assert_eq!(c.keyboard.repeat_delay, 200);
        assert_eq!(c.keyboard.caps_lock, CapsLock::Escape);
        assert!(!c.touchpad.natural_scroll);
        assert!(c.touchpad.tap);
        assert_eq!(c.touchpad.speed, 0.3);
    }

    #[test]
    fn caps_lock_defaults_to_itself_and_rejects_nonsense() {
        assert_eq!(Config::default().keyboard.caps_lock, CapsLock::Lock);
        let (c, _) = parse("[keyboard]\ncaps_lock = \"caps_lock\"\n").unwrap();
        assert_eq!(c.keyboard.caps_lock.xkb_option(), None);
        assert!(parse("[keyboard]\ncaps_lock = \"hyper\"\n").is_err());
    }

    #[test]
    fn unknown_keys_and_bad_ranges_are_warnings() {
        let (c, w) = parse("scale = 9\ncolour = \"red\"\n[touchpad]\nspeed = 3.0\n").unwrap();
        assert_eq!(c.scale, None);
        assert_eq!(c.touchpad.speed, 0.0);
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn the_starter_file_is_the_defaults() {
        let (c, w) = parse(STARTER).expect("the starter file parses");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn the_starter_file_is_written_once_and_never_replaced() {
        let dir = std::env::temp_dir().join(format!("patina-config-{}", std::process::id()));
        let path = dir.join("patina-compositor").join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(write_starter(&path).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), STARTER);
        std::fs::write(&path, "scale = 2.0\n").unwrap();
        assert!(!write_starter(&path).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "scale = 2.0\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bindings_default_and_a_section_replaces_them() {
        let (c, _) = parse("").unwrap();
        assert_eq!(
            c.bindings.get("Alt+T").map(String::as_str),
            Some("teleprinter")
        );
        let (c, w) =
            parse("[bindings]\n\"Ctrl+Alt+Return\" = \"foot\"\n\"Alt+Nope\" = \"x\"\n").unwrap();
        assert_eq!(c.bindings.len(), 2);
        assert_eq!(c.combos().len(), 1, "the bad one is left out");
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn combos_read_modifiers_and_keys() {
        use smithay::input::keyboard::keysyms;
        let c = combo("Alt+T").unwrap();
        assert!(c.alt && !c.logo && !c.ctrl && !c.shift);
        assert_eq!(c.sym, keysyms::KEY_t);
        let c = combo("super + shift + Return").unwrap();
        assert!(c.logo && c.shift);
        assert_eq!(c.sym, keysyms::KEY_Return);
        assert!(combo("Hyper+T").is_err());
        assert!(combo("Alt+").is_err());
    }

    #[test]
    fn a_broken_file_is_an_error() {
        assert!(parse("scale = ").is_err());
        assert!(parse("scale = \"big\"").is_err());
    }
}

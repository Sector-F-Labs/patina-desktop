//! The keyboard layout: `XKB_DEFAULT_*` environment variables first (as
//! xkbcommon's own defaults), then the `[keyboard]` section of the
//! compositor's config, then the system's, from `/etc/default/keyboard`
//! (which Ubuntu's installer and the text console use). Which layout is in
//! use, and the choice of one, are on the bus (`dbus.rs`, ADR-0068).

pub mod dbus;

use smithay::input::keyboard::Layout;

use crate::state::State;

/// xkb's rules, model, layout, variant and options, as names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyboardSettings {
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: Option<String>,
}

impl KeyboardSettings {
    /// The settings for this machine. Empty fields leave xkbcommon to its
    /// defaults (US).
    pub fn load(config: &crate::config::Keyboard) -> Self {
        let file = std::fs::read_to_string("/etc/default/keyboard").unwrap_or_default();
        let mut settings = parse(&file);
        if !config.layout.is_empty() {
            settings.layout = config.layout.clone();
            settings.variant = config.variant.clone();
        }
        if !config.model.is_empty() {
            settings.model = config.model.clone();
        }
        if !config.options.is_empty() {
            settings.options = Some(config.options.clone());
        }
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        if let Some(v) = env("XKB_DEFAULT_MODEL") {
            settings.model = v;
        }
        if let Some(v) = env("XKB_DEFAULT_LAYOUT") {
            settings.layout = v;
            // A variant only makes sense with the layout it came with.
            settings.variant = env("XKB_DEFAULT_VARIANT").unwrap_or_default();
        }
        if let Some(v) = env("XKB_DEFAULT_OPTIONS") {
            settings.options = Some(v);
        }
        if let Some(caps) = config.caps_lock.xkb_option() {
            settings.options = Some(with_option(settings.options.take(), caps));
        }
        settings
    }

    pub fn config(&self) -> smithay::input::keyboard::XkbConfig<'_> {
        smithay::input::keyboard::XkbConfig {
            rules: "",
            model: &self.model,
            layout: &self.layout,
            variant: &self.variant,
            options: self.options.clone(),
        }
    }
}

/// The layouts as last told to the bus, and the service telling it.
#[derive(Default)]
pub struct Layouts {
    service: Option<dbus::Service>,
    told: dbus::Status,
}

/// Each layout's xkb layout and variant names, from `settings`' lists
/// ("us,se" and ",nodeadkeys"), beside the names xkb gave the keymap's
/// layouts. A keymap that fell back to US has no names from the lists.
fn named(settings: &KeyboardSettings, names: Vec<String>) -> Vec<dbus::Layout> {
    let mut codes = settings.layout.split(',');
    let mut variants = settings.variant.split(',');
    names
        .into_iter()
        .map(|name| {
            let code = codes.next().unwrap_or_default().trim().to_string();
            let variant = variants.next().unwrap_or_default().trim().to_string();
            (code, variant, name)
        })
        .collect()
}

impl State {
    /// Serves the layouts on the bus; called once at start-up.
    pub fn start_keyboard(&mut self) {
        match dbus::Service::start(&self.loop_handle) {
            Ok(service) => self.layouts.service = Some(service),
            Err(e) => eprintln!("patina-compositor: keyboard layouts aren't on D-Bus: {e}"),
        }
        self.keymap_changed();
    }

    /// The xkb names of the keymap's layouts, and the index of the one in
    /// use.
    fn xkb_layouts(&mut self) -> (Vec<String>, u32) {
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().expect("xkb isn't poisoned");
            let names = xkb
                .layouts()
                .map(|l| xkb.layout_name(l).to_string())
                .collect();
            (names, xkb.active_layout().0)
        })
    }

    /// Tells the bus about a new keymap: at start-up, and after `[keyboard]`
    /// changed.
    pub fn keymap_changed(&mut self) {
        let settings = KeyboardSettings::load(&self.config.keyboard);
        let unused: Vec<&str> = settings
            .layout
            .split(',')
            .skip(patina_system::keyboard::MOST_LAYOUTS)
            .collect();
        if !unused.is_empty() {
            eprintln!(
                "patina-compositor: a keyboard holds four layouts at most; not using {}",
                unused.join(", ")
            );
        }
        if self.layouts.service.is_none() {
            return;
        }
        let (names, current) = self.xkb_layouts();
        self.tell_layouts(dbus::Status {
            layouts: named(&settings, names),
            current,
        });
    }

    /// Tells the bus if another layout is now in use: after Ctrl+Space, a
    /// choice from the bus, or any key (an xkb option can switch layouts
    /// itself, as grp:alt_shift_toggle does).
    pub fn layout_maybe_changed(&mut self) {
        if self.layouts.service.is_none() || self.layouts.told.layouts.len() < 2 {
            return;
        }
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        let current = keyboard.with_xkb_state(self, |context| {
            context
                .xkb()
                .lock()
                .expect("xkb isn't poisoned")
                .active_layout()
                .0
        });
        if current != self.layouts.told.current {
            self.tell_layouts(dbus::Status {
                current,
                ..self.layouts.told.clone()
            });
        }
    }

    fn tell_layouts(&mut self, status: dbus::Status) {
        if let Some(service) = &self.layouts.service {
            service.publish(status.clone());
        }
        self.layouts.told = status;
    }

    /// Switches to layout `index`, chosen on the bus.
    pub fn choose_layout(&mut self, index: u32) {
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        keyboard.with_xkb_state(self, |mut context| context.set_layout(Layout(index)));
        self.layout_maybe_changed();
    }
}

/// xkb `options` (comma-separated) with `option` added, replacing any
/// other choice in its group (`caps:…` for `caps:escape`), so Caps Lock's
/// setting wins over one from the system's options.
fn with_option(options: Option<String>, option: &str) -> String {
    let group = option.split(':').next().unwrap_or(option);
    let mut kept: Vec<&str> = options
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|o| !o.is_empty() && o.split(':').next() != Some(group))
        .collect();
    kept.push(option);
    kept.join(",")
}

/// Reads the `XKBMODEL="..."` style lines of `/etc/default/keyboard`.
pub fn parse(text: &str) -> KeyboardSettings {
    let mut settings = KeyboardSettings::default();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "XKBMODEL" => settings.model = value,
            "XKBLAYOUT" => settings.layout = value,
            "XKBVARIANT" => settings.variant = value,
            "XKBOPTIONS" if !value.is_empty() => settings.options = Some(value),
            _ => {}
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_debians_keyboard_file() {
        let s = parse(
            "# KEYBOARD CONFIGURATION FILE\n\nXKBMODEL=\"pc105\"\nXKBLAYOUT=\"us,se\"\nXKBVARIANT=\",\"\nXKBOPTIONS=\"grp_led:scroll\"\n\nBACKSPACE=\"guess\"\n",
        );
        assert_eq!(
            s,
            KeyboardSettings {
                model: "pc105".into(),
                layout: "us,se".into(),
                variant: ",".into(),
                options: Some("grp_led:scroll".into()),
            }
        );
    }

    #[test]
    fn caps_lock_joins_the_options_and_replaces_its_own_group() {
        assert_eq!(
            with_option(Some("grp_led:scroll".into()), "caps:escape"),
            "grp_led:scroll,caps:escape"
        );
        assert_eq!(with_option(None, "caps:escape"), "caps:escape");
        assert_eq!(
            with_option(
                Some("caps:ctrl_modifier,grp_led:scroll".into()),
                "caps:escape"
            ),
            "grp_led:scroll,caps:escape"
        );
        assert_eq!(
            with_option(Some("ctrl:swapcaps".into()), "ctrl:nocaps"),
            "ctrl:nocaps"
        );
    }

    #[test]
    fn each_layout_gets_its_names_from_the_lists() {
        let settings = KeyboardSettings {
            layout: "us,se,futhark".into(),
            variant: ",nodeadkeys".into(),
            ..Default::default()
        };
        let names = ["English (US)", "Swedish (no dead keys)", "Runic"].map(String::from);
        assert_eq!(
            named(&settings, names.to_vec()),
            [
                ("us".into(), "".into(), "English (US)".into()),
                (
                    "se".into(),
                    "nodeadkeys".into(),
                    "Swedish (no dead keys)".into()
                ),
                ("futhark".into(), "".into(), "Runic".into()),
            ]
        );
    }

    #[test]
    fn missing_or_empty_is_xkbs_default() {
        assert_eq!(parse(""), KeyboardSettings::default());
        assert_eq!(parse("XKBOPTIONS=\"\"").options, None);
    }
}

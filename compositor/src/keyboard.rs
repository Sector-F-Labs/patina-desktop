//! The keyboard layout: `XKB_DEFAULT_*` environment variables first (as
//! xkbcommon's own defaults), then the `[keyboard]` section of the
//! compositor's config, then the system's, from `/etc/default/keyboard`
//! (which Ubuntu's installer and the text console use).

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
    fn missing_or_empty_is_xkbs_default() {
        assert_eq!(parse(""), KeyboardSettings::default());
        assert_eq!(parse("XKBOPTIONS=\"\"").options, None);
    }
}

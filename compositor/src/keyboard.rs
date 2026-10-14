//! The keyboard layout: the system's, from `/etc/default/keyboard` (which
//! Ubuntu's installer and the text console use), with `XKB_DEFAULT_*`
//! environment variables taking precedence, as xkbcommon's own defaults do.

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
    pub fn load() -> Self {
        let file = std::fs::read_to_string("/etc/default/keyboard").unwrap_or_default();
        let mut settings = parse(&file);
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
    fn missing_or_empty_is_xkbs_default() {
        assert_eq!(parse(""), KeyboardSettings::default());
        assert_eq!(parse("XKBOPTIONS=\"\"").options, None);
    }
}

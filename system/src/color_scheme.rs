//! Style: light or dark (ADR-0087), chosen on Settings' Appearance page and the
//! desktop's right click menu. It's GNOME's
//! setting (`org.gnome.desktop.interface color-scheme`, through gsettings),
//! which xdg-desktop-portal hands on as `org.freedesktop.appearance
//! color-scheme`: Patina apps on the Default skin follow it
//! (`patina::appearance`), as do GTK 4, Firefox and others, live.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// No preference, which apps take as light.
    Default,
    Light,
    Dark,
}

impl Scheme {
    pub const ALL: [Scheme; 3] = [Scheme::Default, Scheme::Light, Scheme::Dark];

    /// The value gsettings holds.
    pub fn value(self) -> &'static str {
        match self {
            Scheme::Default => "default",
            Scheme::Light => "prefer-light",
            Scheme::Dark => "prefer-dark",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scheme::Default => "Default",
            Scheme::Light => "Light",
            Scheme::Dark => "Dark",
        }
    }

    pub fn parse(value: &str) -> Option<Scheme> {
        Scheme::ALL.into_iter().find(|s| s.value() == value)
    }
}

const SCHEMA: &str = "org.gnome.desktop.interface";

/// The style now; `None` where gsettings or the key is missing, so there's
/// nothing to set.
pub fn read() -> Option<Scheme> {
    let out = std::process::Command::new("gsettings")
        .args(["get", SCHEMA, "color-scheme"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    // An unknown value still means the key is there; show it as Default.
    let value = String::from_utf8_lossy(&out.stdout);
    Some(Scheme::parse(value.trim().trim_matches('\'')).unwrap_or(Scheme::Default))
}

pub fn write(scheme: Scheme) -> Result<(), String> {
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

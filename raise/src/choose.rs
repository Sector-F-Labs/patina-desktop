//! Which window to focus, or whether to start the app: the decision alone,
//! so it's easy to test.

/// An open window, as the compositor lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub app_id: String,
    /// What its app is called, from its `.desktop` entry, if it has one.
    pub name: Option<String>,
    pub activated: bool,
}

/// How a name is matched: whole and as written unless asked otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Matching {
    /// `-i`: ignoring case.
    pub insensitive: bool,
    /// `-p`: anywhere in the name.
    pub partial: bool,
}

impl Matching {
    /// Whether `text` is `wanted`.
    pub fn matches(self, text: &str, wanted: &str) -> bool {
        let (text, wanted) = if self.insensitive {
            (text.to_lowercase(), wanted.to_lowercase())
        } else {
            (text.to_string(), wanted.to_string())
        };
        if self.partial {
            text.contains(&wanted)
        } else {
            text == wanted
        }
    }

    /// Whether an app with `app_id` and `name` (from its `.desktop`
    /// entry) is `app`: by its app id, the last part of a reverse-DNS app
    /// id (`Nautilus` for org.gnome.Nautilus), or its name.
    pub fn is_app(self, app_id: &str, name: Option<&str>, app: &str) -> bool {
        let last = app_id.rsplit('.').next().unwrap_or(app_id);
        self.matches(app_id, app)
            || self.matches(last, app)
            || name.is_some_and(|n| self.matches(n, app))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Focus the window at this index.
    Focus(usize),
    /// None of the app's windows is open: start it.
    Start,
}

/// What to do for `app` with `windows` open: focus its first window, or,
/// if one of its windows has the focus already, its next one, round to the
/// first; start it if it has none.
pub fn choose(windows: &[Window], app: &str, how: Matching) -> Choice {
    let mine: Vec<usize> = (0..windows.len())
        .filter(|&i| how.is_app(&windows[i].app_id, windows[i].name.as_deref(), app))
        .collect();
    let Some(&first) = mine.first() else {
        return Choice::Start;
    };
    let focused = mine.iter().position(|&i| windows[i].activated);
    match focused {
        Some(at) => Choice::Focus(mine[(at + 1) % mine.len()]),
        None => Choice::Focus(first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(app_id: &str, activated: bool) -> Window {
        Window {
            app_id: app_id.into(),
            name: None,
            activated,
        }
    }

    const EXACT: Matching = Matching {
        insensitive: false,
        partial: false,
    };
    const LOOSE: Matching = Matching {
        insensitive: true,
        partial: true,
    };

    #[test]
    fn with_none_open_it_starts() {
        assert_eq!(choose(&[], "firefox", EXACT), Choice::Start);
        assert_eq!(choose(&[w("foot", true)], "firefox", EXACT), Choice::Start);
    }

    #[test]
    fn it_focuses_the_apps_window() {
        let open = [w("foot", true), w("firefox", false)];
        assert_eq!(choose(&open, "firefox", EXACT), Choice::Focus(1));
    }

    #[test]
    fn again_it_steps_through_the_apps_windows() {
        let open = [w("firefox", true), w("foot", false), w("firefox", false)];
        assert_eq!(choose(&open, "firefox", EXACT), Choice::Focus(2));
        let open = [w("firefox", false), w("foot", false), w("firefox", true)];
        assert_eq!(
            choose(&open, "firefox", EXACT),
            Choice::Focus(0),
            "round to the first"
        );
        // One window, already focused: it stays.
        assert_eq!(
            choose(&[w("firefox", true)], "firefox", EXACT),
            Choice::Focus(0)
        );
    }

    #[test]
    fn exact_is_whole_and_as_written() {
        assert!(
            EXACT.is_app("org.gnome.Nautilus", None, "Nautilus"),
            "the last part"
        );
        assert!(
            !EXACT.is_app("org.gnome.Nautilus", None, "nautilus"),
            "case counts"
        );
        assert!(!EXACT.is_app("telegram-desktop_telegram-desktop", None, "telegram"));
    }

    #[test]
    fn insensitive_ignores_case() {
        let i = Matching {
            insensitive: true,
            ..EXACT
        };
        assert!(i.is_app("com.sectorflabs.Teleprinter", None, "teleprinter"));
        assert!(!i.is_app("telegram-desktop_telegram-desktop", None, "telegram"));
    }

    #[test]
    fn partial_finds_it_anywhere_in_the_id_or_the_name() {
        assert!(LOOSE.is_app("telegram-desktop_telegram-desktop", None, "telegram"));
        // A web app's window id says nothing; its entry's name does.
        let whatsapp = "chrome-hnpfjngllnobngcgfapefoaidbinmjnm-Default";
        assert!(!LOOSE.is_app(whatsapp, None, "whatsapp"));
        assert!(LOOSE.is_app(whatsapp, Some("WhatsApp Web"), "whatsapp"));
        let p = Matching {
            partial: true,
            ..EXACT
        };
        assert!(
            !p.is_app(whatsapp, Some("WhatsApp Web"), "whatsapp"),
            "case counts"
        );
    }
}

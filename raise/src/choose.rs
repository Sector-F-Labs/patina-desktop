//! Which window to focus, or whether to start the app: the decision alone,
//! so it's easy to test.

/// An open window, as the compositor lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub app_id: String,
    pub activated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Focus the window at this index.
    Focus(usize),
    /// None of the app's windows is open: start it.
    Start,
}

/// Whether a window's `app_id` is `app`'s: the same, ignoring case, or the
/// last part of a reverse-DNS id (`nautilus` for org.gnome.Nautilus).
pub fn is_app(app_id: &str, app: &str) -> bool {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    app_id.eq_ignore_ascii_case(app) || last.eq_ignore_ascii_case(app)
}

/// What to do for `app` with `windows` open: focus its first window, or,
/// if one of its windows has the focus already, its next one, round to the
/// first; start it if it has none.
pub fn choose(windows: &[Window], app: &str) -> Choice {
    let mine: Vec<usize> = (0..windows.len())
        .filter(|&i| is_app(&windows[i].app_id, app))
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
            activated,
        }
    }

    #[test]
    fn with_none_open_it_starts() {
        assert_eq!(choose(&[], "firefox"), Choice::Start);
        assert_eq!(choose(&[w("foot", true)], "firefox"), Choice::Start);
    }

    #[test]
    fn it_focuses_the_apps_window() {
        let open = [w("foot", true), w("firefox", false)];
        assert_eq!(choose(&open, "firefox"), Choice::Focus(1));
        assert_eq!(choose(&open, "Firefox"), Choice::Focus(1), "ignoring case");
    }

    #[test]
    fn again_it_steps_through_the_apps_windows() {
        let open = [w("firefox", true), w("foot", false), w("firefox", false)];
        assert_eq!(choose(&open, "firefox"), Choice::Focus(2));
        let open = [w("firefox", false), w("foot", false), w("firefox", true)];
        assert_eq!(
            choose(&open, "firefox"),
            Choice::Focus(0),
            "round to the first"
        );
        // One window, already focused: it stays.
        assert_eq!(choose(&[w("firefox", true)], "firefox"), Choice::Focus(0));
    }

    #[test]
    fn reverse_dns_ids_match_their_last_part() {
        assert!(is_app("org.gnome.Nautilus", "nautilus"));
        assert!(is_app("org.gnome.Nautilus", "org.gnome.Nautilus"));
        assert!(!is_app("org.gnome.Nautilus", "gnome"));
    }
}

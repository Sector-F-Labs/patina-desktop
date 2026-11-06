//! What dragging a window tab does (ADR-0080). The tabs are the pinned
//! apps, each as its windows or a launcher, then the other windows
//! (`patina_system::pins::arrange`), so a drop changes one of two things: the
//! pins in the bar's config, or the order of the windows, which the
//! compositor keeps.

use patina_system::pins::Tab;

/// What a drop changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drop {
    /// The pinned apps, now in this order.
    Pins(Vec<String>),
    /// The window at `from` in the list of windows goes to `to`.
    Window {
        from: usize,
        to: usize,
    },
    Nothing,
}

/// What dropping tab `from` of `tabs` at `to` (as `Vec::remove` then
/// `insert` would put it) does, with `pinned` the pins and `app_ids` the
/// windows' apps. A pinned app's tab moves its app, with all its windows,
/// among the pins. Another window dropped among the pins pins its app
/// there; dropped among the other windows, it moves along them.
pub fn drop<S: AsRef<str>>(
    tabs: &[Tab],
    pinned: &[String],
    app_ids: &[S],
    from: usize,
    to: usize,
) -> Drop {
    let pin_of = |tab: &Tab| match *tab {
        Tab::Launcher(p) => Some(p),
        Tab::Window(i) => pinned.iter().position(|p| p == app_ids[i].as_ref()),
    };
    let Some(&moving) = tabs.get(from).filter(|_| from != to) else {
        return Drop::Nothing;
    };
    let mut rest = tabs.to_vec();
    rest.remove(from);
    let to = to.min(rest.len());
    let moving_pin = pin_of(&moving);
    // The other pins whose tabs come before the drop.
    let mut pins_before: Vec<usize> = rest[..to]
        .iter()
        .filter_map(pin_of)
        .filter(|&p| Some(p) != moving_pin)
        .collect();
    pins_before.dedup();
    let pinned_tabs = rest.iter().filter(|t| pin_of(t).is_some()).count();
    match (moving_pin, moving) {
        (Some(p), _) => {
            let mut pins = pinned.to_vec();
            let app = pins.remove(p);
            pins.insert(pins_before.len().min(pins.len()), app);
            if pins == pinned {
                Drop::Nothing
            } else {
                Drop::Pins(pins)
            }
        }
        (None, Tab::Window(w)) if to < pinned_tabs => {
            let mut pins = pinned.to_vec();
            pins.insert(pins_before.len(), app_ids[w].as_ref().to_string());
            Drop::Pins(pins)
        }
        (None, Tab::Window(w)) => {
            // Where it goes in the windows without it: before the window
            // it was dropped before, or after the last.
            let without = |i: usize| if i > w { i - 1 } else { i };
            let at = match rest.get(to) {
                Some(Tab::Window(next)) => without(*next),
                _ => match rest.last() {
                    Some(Tab::Window(last)) if pin_of(&Tab::Window(*last)).is_none() => {
                        without(*last) + 1
                    }
                    _ => return Drop::Nothing,
                },
            };
            if at == w {
                Drop::Nothing
            } else {
                Drop::Window { from: w, to: at }
            }
        }
        (None, Tab::Launcher(_)) => Drop::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_system::pins::arrange;

    fn pins(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// Firefox and Steam pinned, Steam not running; foot, Firefox, Files
    /// and another foot open.
    const OPEN: [&str; 4] = ["foot", "firefox", "files", "foot"];

    fn dropped(from: usize, to: usize) -> Drop {
        let pinned = pins(&["firefox", "steam"]);
        // Firefox, Steam, foot, Files, foot.
        let tabs = arrange(&pinned, &OPEN);
        drop(&tabs, &pinned, &OPEN, from, to)
    }

    #[test]
    fn a_pinned_app_moves_among_the_pins() {
        assert_eq!(dropped(0, 1), Drop::Pins(pins(&["steam", "firefox"])));
        assert_eq!(dropped(1, 0), Drop::Pins(pins(&["steam", "firefox"])));
        assert_eq!(
            dropped(0, 4),
            Drop::Pins(pins(&["steam", "firefox"])),
            "dropped past the pins, it stays the last of them"
        );
        assert_eq!(dropped(1, 1), Drop::Nothing);
    }

    #[test]
    fn a_window_dropped_among_the_pins_pins_its_app() {
        assert_eq!(
            dropped(3, 0),
            Drop::Pins(pins(&["files", "firefox", "steam"]))
        );
        assert_eq!(
            dropped(2, 1),
            Drop::Pins(pins(&["firefox", "foot", "steam"]))
        );
    }

    #[test]
    fn a_window_moves_among_the_others() {
        // Files before the first foot: window 2 to where window 0 was.
        assert_eq!(dropped(3, 2), Drop::Window { from: 2, to: 0 });
        // The first foot after the last: to the end of the windows.
        assert_eq!(dropped(2, 4), Drop::Window { from: 0, to: 3 });
        assert_eq!(dropped(2, 2), Drop::Nothing, "where it was");
    }

    #[test]
    fn without_pins_windows_only_move() {
        let tabs = arrange(&[], &OPEN);
        assert_eq!(
            drop(&tabs, &[], &OPEN, 0, 2),
            Drop::Window { from: 0, to: 2 }
        );
        assert_eq!(
            drop(&tabs, &[], &OPEN, 3, 0),
            Drop::Window { from: 3, to: 0 }
        );
    }
}

//! The compositor's own keys (ADR-0069): each action by name, with its
//! default keys, which `[keys]` in the config changes, applied within a
//! second of a save like the rest of it. What isn't bound goes to the
//! focused window, a Super key as Ctrl (ADR-0067), so unbinding one gives
//! it to the apps.
//!
//! The defaults are `keys.toml`, built in (ADR-0075).
//!
//! Not here, and not changeable, so a bad config can't take them: Alt+Tab
//! and Alt+Shift+Tab, quitting and restarting the desktop
//! (Ctrl+Alt+Backspace, Ctrl+Alt+Shift+Backspace), switching consoles, and
//! the brightness and volume keys (`input::key_action`).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;
use smithay::input::keyboard::ModifiersState;

use crate::config::{Combo, combo};
use crate::input::KeyAction;

/// Each action's keys unless `[keys]` sets them: `keys.toml`, built in,
/// where they're changed.
const DEFAULTS: &str = include_str!("../keys.toml");

fn defaults() -> &'static BTreeMap<String, KeyList> {
    static PARSED: LazyLock<BTreeMap<String, KeyList>> = LazyLock::new(|| {
        toml::from_str(DEFAULTS).expect("keys.toml parses (the tests read it)")
    });
    &PARSED
}

/// The defaults' lines commented out, for the config written on first start.
pub fn commented() -> String {
    DEFAULTS
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| format!("# {l}\n"))
        .collect()
}

/// The action called `name`.
fn action(name: &str) -> Option<KeyAction> {
    use crate::maximize::{Resize, Snap};
    use crate::screenshot::Shot;
    Some(match name {
        "launcher" => KeyAction::Launcher,
        "next_layout" => KeyAction::NextLayout,
        "quit_app" => KeyAction::QuitApp,
        "lock" => KeyAction::Lock,
        "reload" => KeyAction::Reload,
        "screenshot" => KeyAction::Screenshot(Shot::Desktop),
        "screenshot_window" => KeyAction::Screenshot(Shot::Window),
        "dictate" => KeyAction::Dictate,
        "snap_up" => KeyAction::Snap(Snap::Up),
        "snap_down" => KeyAction::Snap(Snap::Down),
        "snap_left" => KeyAction::Snap(Snap::Left),
        "snap_right" => KeyAction::Snap(Snap::Right),
        "grow_up" => KeyAction::Grow(Snap::Up),
        "grow_down" => KeyAction::Grow(Snap::Down),
        "grow_left" => KeyAction::Grow(Snap::Left),
        "grow_right" => KeyAction::Grow(Snap::Right),
        "resize_grow" => KeyAction::Resize(Resize::Grow),
        "resize_shrink" => KeyAction::Resize(Resize::Shrink),
        "resize_wider" => KeyAction::Resize(Resize::Wider),
        "resize_narrower" => KeyAction::Resize(Resize::Narrower),
        _ => {
            let n: usize = name.strip_prefix("window_")?.parse().ok()?;
            if !(1..=9).contains(&n) {
                return None;
            }
            KeyAction::Window(n)
        }
    })
}

/// An action's keys in `[keys]`: one ("Super+Q"), or a list; "" or [] for
/// none.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum KeyList {
    One(String),
    Many(Vec<String>),
}

impl KeyList {
    fn keys(&self) -> Vec<&str> {
        match self {
            KeyList::One(k) => vec![k.as_str()],
            KeyList::Many(ks) => ks.iter().map(String::as_str).collect(),
        }
        .into_iter()
        .filter(|k| !k.trim().is_empty())
        .collect()
    }
}

/// Reads modifiers alone, two or more of them ("Ctrl+Super+Alt"), as a
/// combination with no key; `None` if it has a key or only one modifier.
fn chord(text: &str) -> Option<Combo> {
    let mut chord = Combo {
        logo: false,
        alt: false,
        ctrl: false,
        shift: false,
        sym: 0,
    };
    let mut count = 0;
    for part in text.split('+') {
        match part.trim().to_ascii_lowercase().as_str() {
            "super" | "logo" | "mod4" => chord.logo = true,
            "alt" | "mod1" => chord.alt = true,
            "ctrl" | "control" => chord.ctrl = true,
            "shift" => chord.shift = true,
            _ => return None,
        }
        count += 1;
    }
    (count >= 2).then_some(chord)
}

/// Whether `c` is one of the keys fixed in code, matched before these.
fn fixed(c: &Combo) -> bool {
    let modifiers = ModifiersState {
        logo: c.logo,
        alt: c.alt,
        ctrl: c.ctrl,
        shift: c.shift,
        ..Default::default()
    };
    crate::input::key_action(&modifiers, c.sym).is_some()
}

/// The compositor's keys, ready to match.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Keys {
    combos: Vec<(Combo, KeyAction)>,
    /// Modifiers held together with no other key, for actions that last
    /// while they're held (dictate).
    chords: Vec<(Combo, KeyAction)>,
    /// What tapping Super on its own does.
    tap: Option<KeyAction>,
}

impl Keys {
    /// The defaults, with `set`'s actions in place of theirs. Mistakes (an
    /// unknown action or key, one key for two actions) are returned, and
    /// left out.
    pub fn new(set: &BTreeMap<String, KeyList>) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let mut wanted = defaults().clone();
        for (name, list) in set {
            if action(name).is_none() {
                warnings.push(format!("[keys] has no action {name:?}; ignored"));
                continue;
            }
            wanted.insert(name.clone(), list.clone());
        }
        let mut keys = Keys::default();
        let mut taken: Vec<(Combo, &str)> = Vec::new();
        for (name, list) in &wanted {
            let Some(act) = action(name) else {
                warnings.push(format!("keys.toml has no action {name:?}; ignored"));
                continue;
            };
            for text in list.keys() {
                if text.trim().eq_ignore_ascii_case("super") {
                    keys.tap = Some(act.clone());
                    continue;
                }
                let held = chord(text);
                if held.is_some() && act != KeyAction::Dictate {
                    warnings.push(format!(
                        "[keys] {name}: {text} is modifiers alone, which only dictate takes; ignored"
                    ));
                    continue;
                }
                let c = match held.map_or_else(|| combo(text), Ok) {
                    Ok(c) => c,
                    Err(e) => {
                        warnings.push(format!("[keys] {name}: {e}; ignored"));
                        continue;
                    }
                };
                if held.is_none() && fixed(&c) {
                    warnings.push(format!(
                        "[keys] {name}: {text} is the desktop's own, fixed; ignored"
                    ));
                    continue;
                }
                if let Some((_, other)) = taken.iter().find(|(t, _)| *t == c) {
                    warnings.push(format!(
                        "[keys] {text} is both {other} and {name}; it stays {other}"
                    ));
                    continue;
                }
                taken.push((c, name));
                if held.is_some() {
                    keys.chords.push((c, act.clone()));
                } else {
                    keys.combos.push((c, act.clone()));
                }
            }
        }
        (keys, warnings)
    }

    /// The action for a key: exact modifiers, and the key's base symbol
    /// (unshifted, on the first layout), so Shift or another layout don't
    /// change which key it is.
    pub fn lookup(&self, modifiers: &ModifiersState, base: u32) -> Option<KeyAction> {
        self.combos
            .iter()
            .find(|(c, _)| {
                c.logo == modifiers.logo
                    && c.alt == modifiers.alt
                    && c.ctrl == modifiers.ctrl
                    && c.shift == modifiers.shift
                    && c.sym == base
            })
            .map(|(_, action)| action.clone())
    }

    /// The action held with exactly `modifiers` and no other key, if any.
    pub fn chord(&self, modifiers: &ModifiersState) -> Option<KeyAction> {
        self.chords
            .iter()
            .find(|(c, _)| {
                c.logo == modifiers.logo
                    && c.alt == modifiers.alt
                    && c.ctrl == modifiers.ctrl
                    && c.shift == modifiers.shift
            })
            .map(|(_, action)| action.clone())
    }

    /// What tapping Super on its own does, if anything.
    pub fn tap(&self) -> Option<&KeyAction> {
        self.tap.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use smithay::input::keyboard::keysyms;

    use super::*;
    use crate::maximize::{Resize, Snap};
    use crate::screenshot::Shot;

    fn mods(text: &str) -> ModifiersState {
        let mut m = ModifiersState::default();
        for part in text.split('+').filter(|p| !p.is_empty()) {
            match part {
                "Super" => m.logo = true,
                "Ctrl" => m.ctrl = true,
                "Alt" => m.alt = true,
                "Shift" => m.shift = true,
                other => panic!("{other}"),
            }
        }
        m
    }

    fn defaults_keys() -> Keys {
        let (keys, warnings) = Keys::new(&BTreeMap::new());
        assert!(warnings.is_empty(), "{warnings:?}");
        keys
    }

    #[test]
    fn every_default_names_an_action_and_parses() {
        for (name, keys) in defaults() {
            assert!(action(name).is_some(), "{name}");
            for key in keys.keys() {
                assert!(
                    key == "Super" || chord(key).is_some() || combo(key).is_ok(),
                    "{name}: {key}"
                );
            }
        }
        let starter = commented();
        assert!(starter.contains("# snap_right = [\"Super+Right\", \"Ctrl+Super+L\"]\n"));
        assert!(!starter.contains("# #"), "only the settings, not the notes");
    }

    #[test]
    fn alt_tab_is_fixed() {
        let k = defaults_keys();
        assert_eq!(k.lookup(&mods("Alt"), keysyms::KEY_Tab), None, "input's");
        let set: BTreeMap<String, KeyList> =
            toml::from_str("lock = \"Alt+Tab\"\nreload = \"Ctrl+Alt+BackSpace\"\n").unwrap();
        let (k, warnings) = Keys::new(&set);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().all(|w| w.contains("fixed")));
        assert_eq!(k.lookup(&mods("Alt"), keysyms::KEY_Tab), None);
    }

    #[test]
    fn the_defaults() {
        let k = defaults_keys();
        let at = |m: &str, sym| k.lookup(&mods(m), sym);
        assert_eq!(k.tap(), Some(&KeyAction::Launcher));
        assert_eq!(at("Super", keysyms::KEY_space), Some(KeyAction::Launcher));
        assert_eq!(at("Ctrl", keysyms::KEY_space), Some(KeyAction::NextLayout));
        assert_eq!(at("Ctrl+Super", keysyms::KEY_space), None);
        assert_eq!(at("Super", keysyms::KEY_q), Some(KeyAction::QuitApp));
        assert_eq!(at("Ctrl+Super", keysyms::KEY_q), Some(KeyAction::Lock));
        assert_eq!(at("Super", keysyms::KEY_Escape), Some(KeyAction::Lock));
        assert_eq!(at("Super+Shift", keysyms::KEY_r), Some(KeyAction::Reload));
        assert_eq!(at("Super", keysyms::KEY_r), None);
        assert_eq!(
            at("Super+Shift", keysyms::KEY_3),
            Some(KeyAction::Screenshot(Shot::Desktop))
        );
        assert_eq!(
            at("", keysyms::KEY_Print),
            Some(KeyAction::Screenshot(Shot::Desktop))
        );
        assert_eq!(
            at("Super+Shift", keysyms::KEY_4),
            Some(KeyAction::Screenshot(Shot::Window))
        );
        assert_eq!(at("Super", keysyms::KEY_1), Some(KeyAction::Window(1)));
        assert_eq!(at("Super", keysyms::KEY_9), Some(KeyAction::Window(9)));
        assert_eq!(at("Super", keysyms::KEY_0), None);
        for (arrow, vim, snap) in [
            (keysyms::KEY_Up, keysyms::KEY_k, Snap::Up),
            (keysyms::KEY_Down, keysyms::KEY_j, Snap::Down),
            (keysyms::KEY_Left, keysyms::KEY_h, Snap::Left),
            (keysyms::KEY_Right, keysyms::KEY_l, Snap::Right),
        ] {
            assert_eq!(at("Super", arrow), Some(KeyAction::Snap(snap)));
            assert_eq!(at("Ctrl+Super", vim), Some(KeyAction::Snap(snap)));
            assert_eq!(at("Super+Shift", arrow), Some(KeyAction::Grow(snap)));
            // Super alone with a vim key is the app's, as Ctrl (ADR-0067).
            assert_eq!(at("Super", vim), None);
        }
        for (vim, how) in [
            (keysyms::KEY_k, Resize::Grow),
            (keysyms::KEY_j, Resize::Shrink),
            (keysyms::KEY_l, Resize::Wider),
            (keysyms::KEY_h, Resize::Narrower),
        ] {
            assert_eq!(at("Ctrl+Super+Shift", vim), Some(KeyAction::Resize(how)));
            assert_eq!(at("Super+Shift", vim), None, "the app's, as Ctrl+Shift");
        }
        // Super+T and W are the apps'.
        assert_eq!(at("Super", keysyms::KEY_t), None);
        assert_eq!(at("Super", keysyms::KEY_w), None);
    }

    #[test]
    fn dictation_is_held_with_modifiers_alone() {
        let k = defaults_keys();
        assert_eq!(k.chord(&mods("Ctrl+Super+Alt")), Some(KeyAction::Dictate));
        assert_eq!(k.chord(&mods("Ctrl+Super")), None, "not yet all of them");
        assert_eq!(k.chord(&mods("Ctrl+Super+Alt+Shift")), None, "exact");
        assert_eq!(k.lookup(&mods("Ctrl+Super+Alt"), keysyms::KEY_d), None);

        let set: BTreeMap<String, KeyList> =
            toml::from_str("dictate = \"Alt+Space\"\nlock = \"Ctrl+Alt\"\n").unwrap();
        let (k, warnings) = Keys::new(&set);
        assert_eq!(
            k.lookup(&mods("Alt"), keysyms::KEY_space),
            Some(KeyAction::Dictate),
            "a key held works too"
        );
        assert_eq!(k.chord(&mods("Ctrl+Super+Alt")), None);
        assert_eq!(k.chord(&mods("Ctrl+Alt")), None, "only dictate is held");
        assert!(warnings.iter().any(|w| w.contains("only dictate")), "{warnings:?}");
        assert!(chord("Alt").is_none(), "one modifier is too easy to hit");
        assert!(chord("Super+D").is_none());
    }

    #[test]
    fn a_set_action_replaces_its_defaults_and_the_rest_stay() {
        let set: BTreeMap<String, KeyList> = toml::from_str(
            "snap_right = \"Super+Right\"\nlauncher = [\"Super+Space\"]\nquit_app = \"\"\n",
        )
        .unwrap();
        let (k, warnings) = Keys::new(&set);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            k.lookup(&mods("Ctrl+Super"), keysyms::KEY_l),
            None,
            "unbound"
        );
        assert_eq!(
            k.lookup(&mods("Super"), keysyms::KEY_Right),
            Some(KeyAction::Snap(Snap::Right))
        );
        assert_eq!(k.tap(), None, "Super alone no longer opens it");
        assert_eq!(k.lookup(&mods("Super"), keysyms::KEY_q), None);
        assert_eq!(
            k.lookup(&mods("Super"), keysyms::KEY_Left),
            Some(KeyAction::Snap(Snap::Left))
        );
    }

    #[test]
    fn mistakes_are_warnings_and_left_out() {
        let set: BTreeMap<String, KeyList> = toml::from_str(
            "frobnicate = \"Super+F\"\nsnap_up = \"Super+Nope\"\nreload = \"Super+Q\"\n",
        )
        .unwrap();
        let (k, warnings) = Keys::new(&set);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("frobnicate")));
        assert!(warnings.iter().any(|w| w.contains("Super+Nope")));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("both quit_app and reload"))
        );
        assert_eq!(
            k.lookup(&mods("Super"), keysyms::KEY_q),
            Some(KeyAction::QuitApp)
        );
        assert_eq!(k.lookup(&mods("Super"), keysyms::KEY_Up), None);
    }
}

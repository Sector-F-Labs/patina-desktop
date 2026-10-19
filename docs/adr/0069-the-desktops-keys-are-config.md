# 0069. The desktop's own keys are config, by action, applied without a restart

Date: 2026-10-19

## Status

Accepted. The user asked not to have to restart the compositor whenever they want to change a
keyboard shortcut, and agreed to this design.

## Context

The compositor's own keys (the launcher, tiling, the bar's tabs, locking, screenshots) were
written into `input::key_action` in Rust. Moving one meant editing it, rebuilding and restarting
the desktop, which happened several times in a day: Super+Space went to the launcher and
Ctrl+Space to layouts (ADR-0068), and the vim tiling keys moved to Ctrl+Super so that Super+L
reached Files as Ctrl+L (ADR-0067). Meanwhile `[bindings]`, keys that run commands, were already
config, reloaded within a second of a save (ADR-0019).

Considered and ruled out:

- **Restarting without losing windows.** Wayland clients end with their compositor; there's no
  handing them to a new one.
- **One flat table of every key, with nothing built in.** A config written before an action
  existed would leave the new action without keys, and a mistake could leave none at all.

## Decision

- **`[keys]` in the compositor's config names each action and its keys:** one, or a list, written
  as `[bindings]`' are, with "Super" alone for Super tapped on its own. The defaults live in
  `compositor/src/keys.rs` (`DEFAULTS`). An action set in `[keys]` replaces that action's
  defaults; the others keep theirs. "" or [] takes an action's keys away.
- **Applied like the rest of the config:** within a second of a save, or with Super+Shift+R.
  `Keys::new` builds the table on every load. An unknown action or key, or one key given two
  actions, is a warning in the log and left out, keeping the first action.
- **What no action has goes to the app.** A Super key goes as Ctrl (ADR-0067), so unbinding one
  hands it to the apps with nothing more to change.
- **Fixed in code, so no config can take them:** quitting and restarting the desktop
  (Ctrl+Alt+Backspace, Ctrl+Alt+Shift+Backspace), switching consoles, and the brightness and
  volume keys (`input::key_action`). They're matched first.
- **Keys match on exact modifiers and the key's base symbol**, unshifted on the first layout, as
  `[bindings]` do. So "Super+Shift+3" is the 3 key with Shift and Super under any layout, and
  "Alt+Shift+Tab" is Tab, though Shift makes it ISO_Left_Tab.
- **The config written on first start lists every action with its default keys,** commented
  out.

## Consequences

- Moving, adding or taking away a key for an existing action is a config edit, live within a
  second. A new kind of action is still new code and a restart.
- Lookups are one list of combos instead of hand-written conditions, and its tests check the
  defaults key by key.
- Existing configs keep working: with no `[keys]`, every action has its defaults.
- The base-symbol rule means a key is named by its place on the first layout: on a layout where
  that key types another letter, "Super+Q" is still the key in the Q position.
- Settings could get a Keys page writing `[keys]`, as it writes `[gaps]` and `[keyboard]`; not
  yet.

# 0067. Super is Cmd: the compositor sends Ctrl, or Ctrl+Shift to a terminal

Date: 2026-10-19

## Status

Accepted. The user asked for Super+T to open a tab in Teleprinter and the other Patina apps, with
one key for terminals and apps alike, as Cmd is on a Mac. The terminal rule is theirs; the
mechanism is my design.

## Context

The desktop already treats Super as the Mac's Cmd for its own keys: Super+Q quits the app, Super+W
closes the window, Super+Shift+3 and 4 take screenshots (ADR-0026, ADR-0034). Those are the
compositor's to do, through xdg-shell. A new tab, copy or find are the app's, and Wayland has no
request for them, so on Linux they're Ctrl keys. Terminals can't use Ctrl+letter, which belongs to
the shell (Ctrl+T transposes, Ctrl+W deletes a word), so they use Ctrl+Shift+letter. Moving between
a browser and a terminal means two sets of keys; the Mac's Cmd is one.

The apps can't take Super themselves. egui-winit reads Super on Linux but drops it
(`egui-winit` 0.35, `lib.rs:474`): egui's `Modifiers` has Super only as macOS's `mac_cmd`, so
Super+T reaches a Patina app as a plain T, and types a "t". Considered and ruled out:

- **Super in Patina,** by forking egui and egui-winit to add the modifier. A fork of the UI
  toolkit to carry through every egui release, and it would still leave non-Patina apps out.
- **A remap table in config.toml,** key by key and app by app. The apps agree with each other;
  only terminals differ, and they differ the same way for every key.
- **Always Ctrl+Shift+key,** which Patina's tabs use today (Patina ADR-0009). In a browser
  Ctrl+Shift+T reopens the last closed tab and Ctrl+Shift+W closes the window.

## Decision

- **A Super key the compositor doesn't keep goes to the focused window as Ctrl+key; to a
  terminal, as Ctrl+Shift+key.** Super+Shift+key goes as Ctrl+Shift+key to both. For that key the
  client is sent Ctrl (and Shift) in place of Super, in `compositor/src/input.rs`'s key filter,
  with `KeyboardHandle::set_modifier_state`; the real modifiers are sent again when it's released.
- **The compositor keeps its own Super keys,** which win: Q, Escape, Space, 1 to 9, the arrows and
  H, J, K, L (with or without Shift), Shift+3 and 4, Shift+R, Ctrl+Super+Q, and the user's
  `[bindings]`. *Amended 2026-10-19:* the vim keys H, J, K and L tile with Ctrl+Super (and
  resize with Ctrl+Super+Shift), so Super+H, J, K and L go on as Ctrl keys too, Super+L to a
  location bar as Cmd+L on a Mac.
- **A terminal is a window whose desktop entry says so:** `Categories` holds `TerminalEmulator`,
  found from its app id (an X window's class) with patina-system's `apps::entry_for`, which gains
  the field. Teleprinter's desktop entry gets the category.
- **Super+W leaves the compositor** and closes a tab like the rest: Ctrl+W, or Ctrl+Shift+W in a
  terminal. Closing the last tab closes the window, as the app decides. A window's close button
  and its tab's menu still close the window.
- **Locked or under a fullscreen game, no change:** Super goes as it is, as the keys do now.
- **Patina's side** is recorded in Patina's own ADR: `tabs::keys` takes Ctrl+T and Ctrl+W, and a
  terminal's variant, which Teleprinter uses, takes Ctrl+Shift+T and Ctrl+Shift+W.

## Consequences

- One set of keys everywhere, as on a Mac: Super+T, W, C, V, F, N in Firefox, Notebook and
  Teleprinter alike, and Super+Shift+T reopens a closed tab in a browser as Cmd+Shift+T does. The
  Ctrl keys still work as before.
- No Patina app needs to know about Super. Off this desktop (on GNOME), the apps have their Ctrl
  keys and Super does nothing in them.
- An app that binds Super keys itself loses them, as a game does outside fullscreen.
- A terminal whose desktop entry lacks the category gets Ctrl+T, and an app ignoring Ctrl+W no
  longer closes on Super+W (Super+Q still quits it). If that happens, a config list of terminal
  app ids can be added beside the categories.
- ADR-0050's note that Super+W closes an icon-only tab's window no longer holds; a middle click or
  the tab's menu still does.
- Revisit if egui exposes Super on Linux, or if a terminal's keys can't be told from its desktop
  entry.

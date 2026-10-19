# 0068. Keyboard layouts on the bus: the bar shows the one in use, Settings manages them

Date: 2026-10-19

## Status

Accepted. The user asked for the tray to say which layout is in use when there's more than one, and
for a place in Settings to manage them, and agreed to this design. They chose Ctrl+Space for
switching layouts, as on a Mac, when Super+Space became the launcher's.

## Context

The compositor holds the keymap: the layouts come from `[keyboard]` in its config, or else from
`/etc/default/keyboard` (ADR-0019), and Super+Space stepped to the next. Nothing outside the
compositor knew which layout was in use, so with six layouts (the user's: us, za, es, ru, se and a
runic one of their own in `~/.config/xkb`) there was no telling what a key would type. Adding or
reordering layouts meant editing the config by hand.

The user wanted Super+Space to open the launcher, as Cmd+Space does on a Mac, so layouts needed
another key. They picked Ctrl+Space, also the Mac's, over Super+Shift+Space and Ctrl+Alt+Space,
knowing editors use Ctrl+Space for completion and the compositor now takes it.

Considered and ruled out:

- **The bar reading the config itself.** The config can leave the layouts to the system's, and
  `XKB_DEFAULT_*` overrides both; only the compositor knows what the keymap is, and only it
  knows which layout is in use.
- **wlr's or ext's protocols.** There's no Wayland protocol for the active layout. A client
  sees its own keymap's group in `wl_keyboard.modifiers`, but only while it has the keyboard,
  which the bar never does.

## Decision

- **The compositor serves `com.sectorflabs.Patina.Keyboard`** at `/com/sectorflabs/Patina/Keyboard`
  (`compositor/src/keyboard/dbus.rs`), nested too: `Layouts`, `a(sss)`, each layout's xkb layout
  and variant names with xkb's name for it, in Ctrl+Space's order; `Current`, the index in use;
  and `SetCurrent(u)`. Both properties announce changes. The compositor checks the layout after
  every key, so an xkb option that switches layouts itself is caught too, and after a new keymap.
- **The bar's tray shows the layout in use when there are two or more**, as its code ("US", "SE"),
  or xkb's short description for a longer name ("ᚠ"), or the first three letters when the skin's
  font lacks the glyphs. The tooltip gives xkb's name. A click lists the layouts in patina-menu,
  the one in use ticked, and switches to the one chosen. It follows the property signals, so
  Ctrl+Space shows at once.
- **Settings has a Keyboard page** listing the layouts with their labels and names. Each can be
  moved up or down, removed (not the last), or switched to. Layouts are added from those xkb
  offers, read from the user's and the system's `evdev.xml` and `evdev.extras.xml`, by search.
  The page writes `layout` and `variant` into `[keyboard]` with `toml_edit`. While the config has
  none, it shows the system's, and the first change copies them in. Key repeat delay and rate are
  on the same page.
- **Key repeat alone no longer rebuilds the keymap.** Setting a keymap goes back to the first
  layout, so the compositor only does it when the xkb part of `[keyboard]` changes
  (`Keyboard::same_keymap`).
- **Super+Space opens the launcher, Ctrl+Space switches layouts.**

The shared client (status, `SetCurrent`, `watch`, the catalogue of xkb's layouts and the labels)
is `system/src/keyboard.rs`.

## Consequences

- With several layouts, the tray always shows which is in use, and choosing one by name is a
  click away; with one, nothing changes.
- Apps no longer get Ctrl+Space. Completion in editors and IDEs needs another key.
- Changing the list in Settings goes back to the first layout, since the keymap is set again.
  The page says so.
- Settings writes the system's layouts into the compositor's config on the first change, after
  which `/etc/default/keyboard` no longer applies. Removing them all from the config by hand
  goes back to it.
- The catalogue only knows layouts listed in an `evdev.xml`. A custom layout with only a symbols
  file shows by its code, and can't be added from the page.
- Another interface on the session bus to keep stable for the bar and Settings. Revisit if
  Wayland gains a protocol for the active layout.

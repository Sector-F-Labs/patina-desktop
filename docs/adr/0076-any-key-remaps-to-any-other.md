# 0076. Any key remaps to any other, sent to the focused window

Date: 2026-10-21

## Status

Accepted. The user asked to be able to remap any key combination to any other, such as Alt+T
to Ctrl+T, as Super+T already reaches apps as Ctrl+T (ADR-0067).

## Context

Super as Ctrl (ADR-0067) is a rule, not a table: any Super key no action has goes on as
Ctrl+key. Nothing else could be sent as other keys. `[keys]` (ADR-0069) gives keys to the
compositor's actions and `[bindings]` to commands, but neither sends the app a different key.

`KeyboardTarget::key` needs a `KeysymHandle`, which Smithay only makes for a key going through
`KeyboardHandle::input`; its fields are private, so another key can't be made for it in place.

## Decision

- **`[remap]` maps a combination pressed to the one the focused window gets:** `"Alt+T" =
  "Ctrl+T"`, both written as for `[bindings]`, matched the same way (exact modifiers, the key
  unshifted on the first layout). Defaults are `[remap]` in `compositor/keys.toml` (ADR-0075),
  none yet; the config's add to them or replace one for the same key, and `""` takes one away.
- **Order:** the fixed keys, then `[keys]`, then `[remap]`, then `[bindings]`, then Super as
  Ctrl. A remap on a fixed key or a `[keys]` key is a warning and left out; one on a binding
  wins, with a warning, so remapping a default binding's key works.
- **The key sent goes through the seat** (`remap.rs`): after the key pressed is kept from the
  window, the sent key's code (found on the first layout) is put through `KeyboardHandle::input`,
  and its filter tells only the focused window the remap's modifiers are down and sends it, as
  `cmd.rs` does. Its release follows the pressed key's, then the window is told the modifiers
  really down.
- Like the compositor's keys, remaps are off while locked and under a fullscreen game.

## Consequences

- Any shortcut an app has can be reached by another combination, live within a second of a
  save.
- The seat counts the sent key as down while it's held; when it's the same key as the one
  pressed (Alt+T to Ctrl+T), it's pressed and released twice in the seat's xkb state, which a
  key that isn't a modifier doesn't notice. Remapping to a modifier key isn't supported.
- Super as Ctrl stays a rule in code; a remap overrides it for its key.
- Modifiers alone aren't remapped here: swapping Alt and Super is xkb's `altwin:swap_alt_win`
  in `[keyboard] options`, under everything, so the desktop's keys see the swap too.

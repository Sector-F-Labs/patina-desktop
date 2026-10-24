# 0086. Caps Lock held is a layer of keys, tapped Escape

Date: 2026-10-24

## Status

Accepted. The user asked for H, J, K and L to be arrows while Fn is held on nifl. nifl's Fn
never reaches the kernel (no input device there declares `KEY_FN`), so Caps Lock is the key
held, and tapped alone it can still be Escape. The user chose the names `caps_hold` and
`caps_tap`, and tap and hold only, no locking the layer.

## Context

`[keyboard] caps_lock` is an xkb option: it makes the whole key something else, and can't tell
a tap from a hold. `[remap]` (ADR-0076) sends a combination as another, but needs a modifier
xkb knows, and Caps Lock held isn't one. The compositor sees every key before the seat does.

## Decision

- **`[keyboard] caps_hold = "layer"`** (default `"none"`): Caps Lock is the compositor's
  (`caps.rs`) and never reaches the seat, so xkb's Caps Lock never toggles and `caps_lock` is
  left out of the options, with a warning if set.
- **`[caps_layer]`** maps one key alone to what it sends, `"H" = "Left"`, written as for
  `[bindings]`. A key pressed with Caps Lock held is looked up first, on its base keysym, and
  sent through the remap path, with the modifiers held added to the remap's, so Shift+Caps+L
  is Shift+Right. A key not in the layer goes on as usual.
- **`caps_tap`**: Caps Lock pressed and released within 200 ms, with no key between, sends
  `"caps_lock"` (the default, the real key through the seat), `"escape"` or `"none"`, when it
  comes up.
- Off while locked and under a fullscreen game, as remaps are: Caps Lock is then plain Caps
  Lock. A release goes where its press went.

## Consequences

- vim's Escape comes on the release, not the press; held alone, Caps Lock doesn't repeat it.
- Caps Lock as caps lock is lost with `caps_tap = "escape"`.
- No layer lock and nothing on screen for the layer; both left out until asked for.
- Only Caps Lock can be a layer key.

## Revisit when

- Another key is wanted as a layer key (Space, right Alt), or a locked layer.
- A machine's Fn reaches the kernel as `KEY_FN`: it could hold the same layer.

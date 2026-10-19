# 0071. Text is typed into the focused window through the clipboard, given back after

Date: 2026-10-19

## Status

Accepted. The user chose the clipboard path first for dictation (ADR-0070), and to leave X11
windows out.

## Context

Dictation (ADR-0070) needs the compositor to type a text into whatever window has the focus,
asked by another program. There are three ways:

- **text-input-v3**, the input-method protocol: the compositor commits text straight into the
  focused field, any Unicode, no clipboard. GTK, Qt, Firefox and Chromium have it, but not every
  client, terminals among them, and Smithay's support needs an input method to drive it.
- **Synthetic key presses** per character, as `wtype` does: every character needs a key in the
  keymap, which means sending a keymap of the text's own; apps take that badly, and long texts
  are slow.
- **The clipboard and the paste key**, as Handy does: works in every Wayland app that pastes,
  terminals too (with Ctrl+Shift+V). Its cost is the clipboard: Handy's approach replaces what
  the user had copied. The compositor can do better than an app here: it owns the seat, so it
  can read the clipboard out first, serve the text itself, and press the keys without a
  virtual-keyboard protocol.

X11 windows would need the X11 and Wayland clipboards bridged, which the compositor doesn't do.
X11 is mostly games and Steam here, nothing anyone dictates into, and apps are moving to
Wayland, so X11 support is kept minimal.

## Decision

- **`com.sectorflabs.Patina.Input` with `Commit(text)`** on the session bus, served nested too
  (`compositor/src/typing.rs`, `typing/dbus.rs`).
- **The clipboard first.** On Commit the compositor:
  1. Reads what's on the clipboard out of the app that owns it, every type it offers, for up to
     half a second and 64 MB a type. A clipboard the compositor already owns is kept as it is.
  2. Sets the text as its own selection (`Served::Text`, the `SelectionUserData`), in the usual
     text types.
  3. Presses Ctrl+V, or Ctrl+Shift+V in a terminal (as `cmd.rs` tells them, ADR-0067), through
     `KeyboardHandle::input`, so the compositor's own keys never see them. V is looked up in the
     layout in use, so it's V on Dvorak too.
  4. Gives the old clipboard back (`Served::Saved`), 150 ms after the text was read, or after a
     second if it never is. An empty clipboard is left empty.
  Texts committed meanwhile wait their turn, and the clipboard is given back after the last.
- **Wayland windows only.** With an X11 window focused, or none, a text is dropped, with a line
  in the log.
- **text-input-v3 can come later**, in front of this, where the focused client has it.

## Consequences

- Dictation works in every Wayland app that pastes, terminals included, and leaves the clipboard
  as it found it, except that the compositor then owns it: the app that copied no longer does,
  so a type it would have produced lazily is only there if it was read out in step 1.
- Clipboard managers see each dictated text go by, and may keep it in their history.
- A window that takes more than a second to read the clipboard pastes the old clipboard. A
  clipboard manager reading the text first starts the 150 ms early.
- Apps that bind Ctrl+V to something other than paste (Vim's visual block in a GUI) do that
  instead.
- If a modifier is physically held when Commit arrives, the window sees it with Ctrl+V.
  Dictation's key acting on release (next) avoids that.
- The selection's user data is now `Served`, not `()`, which is where an X11 bridge would have to
  fit if one is ever wanted.

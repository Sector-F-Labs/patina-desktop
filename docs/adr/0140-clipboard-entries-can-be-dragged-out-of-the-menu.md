# 0140. Clipboard entries are dragged out of the menu, and the bus serves their content

Date: 2026-11-14

## Status

Accepted. The user asked to drag items out of the clipboard menu into apps, chose a bus method
for the content over a drag the compositor starts itself, and asked that the menu close when the
button is let go, rather than when the pointer or focus moves away. (They first asked for a
toggle that stays open, then changed their mind before it was tried.)

## Context

The clipboard menu (ADR-0114) copies an entry again on a click and closes. Pasting means
going back to the app and pressing Ctrl+V. Dragging a row onto the app it's meant for is a
shorter path, and the only way to put an image into an app that takes drops but not pastes.

A drag on Wayland is started by the client that owns the pointer press. It makes a
`wl_data_source`, offers MIME types, and gives the bytes of one type to the app that asks when
it's dropped. The menu has none of those bytes: `List` gives previews cut short and thumbnail
paths (ADR-0114), and the full content is reachable only by `Pick`, which sets the selection.

The menu also closes as soon as it loses keyboard focus. A drag leaves its surface and can
take focus with it, so the menu would close mid-drag, at the first sign of what it was opened
to do.

Options for the content:

- **The compositor starts the drag.** The bytes never cross the bus, so the bus exposure of
  ADR-0114 doesn't grow. Smithay's `start_dnd` needs a grab, and the compositor would have to
  start it from a press that went to the menu's surface and that it didn't see as a drag.
- **A bus method for the content, and the menu starts the drag.** The same shape as Files'
  drag out (ADR-0006), with no grab code in the compositor. The cost is that content crosses
  the bus. We chose this one.

## Decision

- **`Types(id) -> as` and `Content(id, mime) -> ay`** on `com.sectorflabs.Patina.Clipboard`: the
  MIME types an entry holds, and its bytes as one of them. They fail for an id the history
  doesn't have, and for a type the entry doesn't hold. `List` stays as it was: previews only.
  patina-system's `clipboard.rs` has `types` and `content`.
- **patina-shell starts drags.** A `Shell` returns a `Drag` (types, a function that gives the
  bytes of a type, an optional icon) from `take_drag`; the host continues the press that's held
  as a Wayland drag. Like Files' drag out, it uses a queue of its own on the host's connection
  with a data device and a thread that answers `send`, made when the first drag starts, so
  pieces that never drag pay nothing.
- **The menu drags a row** once the pointer has moved past egui's drag threshold, so a click
  still picks. The drag offers every type the entry holds, with a picture of the row under the
  pointer, and gets the bytes from `Content` when the target asks, on a thread.
- **Closing waits for the button.** Losing focus still closes the menu, but not while a drag
  from it is under way. The host tells the shell when the drag ends (`Shell::drag_ended`: the
  source's `dnd_finished` or `cancelled`), and the menu closes then, on the mouse up, not on
  the mouse down that started the drag or on leaving the surface.
- **A Pin checkbox in the menu's header** keeps it open: it then ignores losing focus, doesn't
  close after a pick or a drop, and closes on Escape or the bar's icon. It's per opening and
  isn't saved. The user asked for it after the close-on-release change, for dragging several
  entries in a row.

## Consequences

- Anything that can drop onto an app can use history: text into editors and terminals, images
  into apps that take them. Rich types aren't kept (ADR-0114), so text drops as plain text.
- **`Content` lets any program on the session bus read the whole history**, where `List`
  gave previews. Before, it could only set the selection. This is a real loss, as ADR-0114
  said: the same user runs every program, and a socket alone wouldn't stop that. `Paused`,
  expiry and the password filter now matter more. If it proves too much, revisit with the
  compositor-side drag above, which needs no `Content` at all, or a socket the compositor hands
  only to the menu.
- Bytes cross the bus whole, up to the 16 MB an entry can hold, though a D-Bus message may be
  larger. Images are large; they're read only when a target asks.
- The compositor's `Status` now holds each entry's bytes, shared with the history, not copied.
- One drag per opening unless it's pinned: the menu closes after the drop, so dragging several
  entries means pinning it first.
- A touch can't start a drag: the host only continues a pointer press.

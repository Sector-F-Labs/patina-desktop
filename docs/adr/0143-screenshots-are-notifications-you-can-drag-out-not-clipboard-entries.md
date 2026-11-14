# 0143. Screenshots are notifications you can drag out, not clipboard entries

Date: 2026-11-14

## Status

Accepted. The user proposed it. Supersedes the part of
[0116](0116-screenshots-go-to-the-clipboard-and-clipboard-settings.md) that put every screenshot on
the clipboard, and its Settings checkbox; the rest of 0116 (the `[clipboard]` section, the history
settings) stands. Builds on [0140](0140-clipboard-entries-can-be-dragged-out-of-the-menu.md) and
[0141](0141-notifications-are-a-column-at-the-right-edge-that-keeps-its-space.md).

## Context

A screenshot is already a file in `~/Pictures/Screenshots` (ADR-0034). ADR-0116 also put its
picture on the clipboard, so it could be pasted. That made the clipboard a place that points at
files and takes pictures pushed into it, and it was untidy: a screenshot replaced whatever was
copied, filled the history with multi-megabyte entries, and getting one into an app that takes
drops but not pastes needed the history menu's drag (ADR-0140), which hands out a copy in a temporary
file rather than the screenshot itself. The clipboard should be the clipboard.

The notification column (ADR-0141) is where something that has just happened and wants a
gesture belongs, and a card can be dragged the way a menu row can.

## Decision

- **Screenshots no longer go on the clipboard.** The compositor stops copying them. The history,
  its settings and the clipboard menu are as before.
- **A saved screenshot is announced as a notification.** After saving, the compositor calls
  `org.freedesktop.Notifications.Notify` on the session bus: app "Screenshot", summary
  "Screenshot saved", the file name as the body, and two hints: the standard `image-path`, and
  `x-patina-file`, both the saved path. Nothing else changes if no daemon answers: the file is
  where it was saved, and the flash still shows.
- **A card with an `x-patina-file` is a file card.** It shows the picture as a thumbnail, read
  and cut down on a thread of its own, and can be pulled out of the column: the drag offers
  `text/uri-list` (a `file://` URI, percent-encoded, as a file manager gives one, so an app
  treats it as the file and a web page gets an upload) and, for a PNG or a JPEG, its own type,
  with the thumbnail under the pointer. It is the screenshot itself that is dropped, not a copy.
  Other notifications cannot be dragged.
- **The card goes when an app takes the drop.** The host tells the shell whether the app it was
  dropped on took it (`dnd_finished`) or it was refused or called off (`cancelled`), and only the
  first closes the card, as a dismissal. One that was called off stays. The user asked for this
  after first seeing the card stay; the screenshot is still in its folder to drag again.
- **It can be turned off**: `screenshots` in patina-notify's config (default true), a checkbox on
  Settings' Notifications page. The daemon then takes no card that stands for a file, and nothing
  shows; the compositor still announces and still saves.
- **`[clipboard] screenshots` is read and ignored**, not an unknown key, since Settings had
  written it into files; the Settings page loses the checkbox.

## Consequences

- Taking a screenshot no longer replaces what was copied, and no longer fills the history.
- Pasting a screenshot with Ctrl+V is gone. The way in is dragging its card, or the file.
- With notifications off, or no daemon (nested, or another desktop's), a screenshot is saved with
  no card, so it can't be dragged; the file is in the folder. The daemon holds the card back
  while a fullscreen window has the keyboard, and shows it after.
- Cards stay until dismissed (ADR-0141), so screenshots taken in a row stack up in the column,
  each to dismiss. If that proves too much, screenshot cards could take a timeout.
- The notification carries a vendor hint, `x-patina-file`, that other servers ignore; another app
  could send one to get a draggable card.
- An app that takes a drop but never says it has finished (`wl_data_offer.finish`) leaves the
  card in place, which is the safe way to be wrong.
- A click on a screenshot card matches no window, so it does nothing and the card stays.

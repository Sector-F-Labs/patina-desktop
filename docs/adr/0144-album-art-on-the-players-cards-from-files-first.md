# 0144. Album art on the players' cards, from files first

Date: 2026-11-14

## Status

Accepted. Builds on [0117](0117-the-volume-menu-lists-the-players.md), which left album art for later.

## Context

The cards in the volume menu (ADR-0117) show a track's title, its artist and the player, with
the transport buttons. They have no picture, though most players publish one: the MPRIS
`mpris:artUrl` property. It comes in three kinds:

- **`file://` URLs**, to a file on disk. Desktop players such as UnAmp send these, and so can a
  browser, which writes the cover of a tab's media to a temporary file.
- **`http(s)://` URLs.** Some players (Spotify's, for one) send the address of the cover on their
  server instead of a file.
- **Nothing.**

## Decision

- **Only `file://` covers, for now.** `patina_system::mpris::art_path` turns a `mpris:artUrl` into
  a path when it is a `file://` URL (percent-escapes undone) to a file that exists, and into
  nothing otherwise. `Player` has an `art: Option<PathBuf>`.
- **`http(s)` covers are not fetched yet.** A player that sends one gets a card as before. This is
  a gap on purpose, to be built next: it needs an HTTP client dependency, a fetch off the UI
  thread with a timeout and a size limit, and a place to keep what was fetched. The menu
  would make outbound requests of its own, which no part of the desktop's session does at the
  moment, so it wants its own decision. When it is built, `art_path` is where a URL would
  become a fetch, and `art.rs`'s `Covers` is where the result would be kept: the rest (the
  thumbnail, the card's size, the layout) doesn't change.
- **Only when there is art.** A card whose player has a cover that can be read looks as follows; one
  without keeps the look it had (the same height, the text from the left edge).
- **The picture is 64 × 64** at the card's left, with the same corner radius as the card's buttons,
  and the card is that and 6 px of margin round it, 76 tall instead of 48. The text sits in the
  middle of the taller card and the buttons stay at its right, centred.
- **Read off the UI thread.** The thread that reads the players (every 400 ms while the menu is
  up) reads each new cover, fills a square of 128 × 128 (twice the size it is drawn at, for a
  screen at scale 2) and sends the thumbnail with the players. It reads a cover again only when
  its file's time changes. A cover that can't be read (not a picture, or a format we don't
  decode: PNG, JPEG and WebP are) is taken off its player, so its card is the plain one.
- **The menu is sized with the covers.** `height` takes the players, not their number: a card with a
  cover is taller, so the menu opens at the height it will have, as ADR-0117 asked, because the
  menu's first read of the players also reads their covers. When one arrives or goes while the menu
  is open, it resizes, as it does when a player does.

## Consequences

- With UnAmp, and any player that publishes a file, the menu shows the cover; Firefox and
  Chromium vary by the page and the version, and show it only when they write a file.
- A card with art is 28 px taller. The menu still shows four cards before it scrolls, but with
  covers on all of them it is taller.
- The bar's media button is untouched; the art is on the cards only.
- `patina-volume` depends on `image` (PNG, JPEG and WebP, as `patina-system` does).
- Players that publish only an `http(s)` cover still have none. Spotify is the usual case.

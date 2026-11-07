# 0117. The volume menu lists the media players, and the bar counts them

Date: 2026-11-07

## Status

Accepted. Builds on [0101](0101-media-controls-in-the-bar.md).

## Context

The bar's media controls (ADR-0101) follow one player: the first playing, else the first
paused, by bus name. With a YouTube tab in Firefox and something in UnAmp both going, one of
them has controls and the other has none, and which one is down to its name on the bus. ADR-0101
left "a menu of players" for later.

The user wanted the way GNOME has it: one place, a list of players, each with its own controls.
The speaker button already opens a small menu (patina-volume), and the clipboard has a list in
one. The speaker is where people go for sound.

## Decision

- **The volume menu lists the players.** Under the slider, a card for each player that is playing
  or paused: the title, the artist and who plays it, and previous, play or pause and next. Playing
  ones come first, otherwise the order stays as the bus gives it, so a card doesn't jump when
  another starts. A stopped player has nothing to control and isn't listed. The cards follow the
  bus while the menu is open, so a pause made in the browser shows.
- **Only the menu, never the flash.** After a volume key it is the slider alone, as before: a key
  must not pop up a panel. The menu is as tall as its cards, up to four, then it scrolls, and
  with no player it is the slider, as before. It is sized from a read of the players at start, so it
  opens at its height and doesn't grow after.
- **The media controls are one button with more than one player.** The bar's `media` module keeps
  its previous, play and next for a single player. With two or more it is one play or pause
  button for the one shown, with a badge: their number in a small disc in the icon's own colour, the number cut out of it in
  the bar's background, not red, since it's information and not a warning (ten or more show as 9+). It opens the volume menu instead of acting on
  one player, since which one a click would mean is the thing that was unclear. Back to one
  player, the controls come back.
- **The speaker stays as it was**, with no badge. A badge there was tried and was redundant next
  to the media button's, which is where the players are seen and reached.
- **`patina_system::mpris::active`** orders the players for the menu and the count.

## Consequences

- With two players there is a way to reach both from one place, and a sign on the media button that
  there are two.
- patina-volume talks to the session bus: a connection of its own, polled with the levels every
  400 ms while it is up, and one thread for the cards' commands. The flash reads them too, though it
  doesn't show them.
- Which player the bar's single button shows with several is still by bus name, playing first.
- No seek bar, volume per player or album art on the cards; they would fit there later.

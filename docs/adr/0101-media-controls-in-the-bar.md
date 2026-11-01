# 0101. Media controls in the bar, from MPRIS

Date: 2026-11-01

## Status

Accepted

## Context

Music and video played in an app had no controls on the desktop: to pause UnAmp or a YouTube tab
you went to its window. Players already publish what's playing and take commands on the session
bus, as MPRIS (`org.mpris.MediaPlayer2.*`): browsers, UnAmp (its ADR-0016), Spotify and
Bluetooth headsets through `mpris-proxy` all do.

Holding the screen on while something plays was weighed with it and left out. Browsers already
hold it on while a video plays, through idle-inhibit (ADR-0047); music is fine with the screen off
and locked, and tying it to "a player is Playing" would keep the screen lit through every playlist.
That belongs to the player, as a setting, not to the bar.

## Decision

- **`patina_system::mpris`:** lists the players on the session bus and reads each one's status,
  title, artist and whether it can skip; sends PlayPause, Next and Previous. The one shown is the
  first playing, else the first paused; a stopped one has nothing to control. Nothing playing,
  nothing shown.
- **A `media` module of the bar**, with its own state in the bar (a thread asks twice a second, so a
  click shows soon): previous, play or pause, and next, as buttons in the skin's colours, in a row
  or a column as the bar is, with the track and who plays it on hover. Previous and next are left out
  where the player says it can't. Clicks go to the player on a thread of their own.
- **Not in the tray's own list of icons:** it is a module next to `tray` in the default `right`
  (`["media", "tray", "clock", "power"]`), so it's turned off by taking it out of the sides' lists.
  A config that spells out `right` without it has no media controls until it's added.
- **Split bar:** the controls are in the tray's part, as in the whole bar, until `media_at` gives
  them a spot of their own, with the same names as `tray_at`. That part is a `--section media`,
  kept like the others by its edge's reserve, and with nothing playing it takes a single pixel and
  draws nothing, not an empty island. `media_at` equal to `tray_at`, or without `media` among the
  modules, leaves them where they were.

## Consequences

- Any MPRIS player gets controls, with no change to it. A Firefox tab on YouTube shows as paused or
  playing as the browser reports it.
- The bar asks the bus twice a second while it has the module; the calls are a name listing and a
  few property reads per player.
- Which player is shown can't be chosen: with two playing, the first by bus name wins. A "last
  active" rule or a menu of players could come later.
- No seek bar, volume or album art; a click on the title area could open the player's window
  (`Raise`) later.
- Settings' Bar page doesn't place the media part yet; `media_at` is set in the file.

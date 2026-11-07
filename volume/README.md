# patina-volume

The volume menu in the bar's tray, which also flashes up when the volume or brightness keys
are used (ADR-0021, ADR-0033).

## On the desktop

- **As a menu**, from the bar's speaker: a mute button, a slider and the level, at the bar's
  right end, with a card for each media player playing or paused: its track and previous, play or
  pause and next (ADR-0117). It stays until Escape, a click elsewhere or the speaker again.
- **As a flash**, after a volume or brightness key: it fades in without taking the keyboard,
  shows what the key changed, holds while the keys keep coming, and fades out on its own.

## How it fits in

- **Started by** the bar's speaker (`patina-volume`, with the bar's skin and edge), and by the
  compositor after a media key: `--flash` after a volume key, `--brightness` after a brightness
  key. A run while one is open is passed to it over its socket (patina-shell's `Single`): a
  second plain run closes the menu, a flash keeps an open one up or restarts its hold.
- **Doesn't change anything itself on a key.** The compositor runs the `[media]` volume commands
  (`wpctl` by default) and steps the backlight; this only shows the result.
- **Reads and sets** the default output's volume through `wpctl` (patina-system's `audio.rs`),
  and the brightness through sysfs and logind (`backlight.rs`), both off the UI thread.

## Code

`main.rs` (one copy, the mode, the surface), `ui.rs` (the menu, the flash and its fades).

## Running

```sh
cargo run -p patina-volume                 # the menu
cargo run -p patina-volume -- --flash      # the flash
```

It opens at the bar's button that opened it, from `PATINA_OPEN_AT` (patina-shell's
`piece::opening`, ADR-0063), or on the bar's edge without it.

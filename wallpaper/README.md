# patina-wallpaper

The desktop's wallpaper, behind every window on every screen (ADR-0030): the image chosen in the
desktop's menu, filling the screen, or by default the Sector F Labs flask and name, in Ubuntu
Mono, in the colours of the skin.

## How it fits in

- **Started by** patina-compositor at start-up, before the bar, so it's there first.
- **A click-through surface on the background layer** of each output (patina-shell's host with
  `per_output` and `click_through`), so clicks on the desktop reach the compositor, which opens
  patina-menu on a right click.
- **What it draws** comes from the bar's config, `~/.config/patina-bar/config.toml`: `wallpaper`
  (an image, scaled to cover the screen) and `skin` (the default's colours). patina-menu writes
  both; the wallpaper watches the file and redraws when either changes.
- **The default** is drawn from patina-system's `logo.rs`, the same flask as the bar's launcher
  button, tinted from the skin like everything else.

## Code

`main.rs`.

## Running

```sh
cargo run -p patina-wallpaper
# Renders the default in every built-in skin into a folder, for checking the look:
PATINA_WALLPAPER_PREVIEW=/tmp/walls cargo test -p patina-wallpaper -- --ignored
```

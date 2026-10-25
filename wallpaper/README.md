# patina-wallpaper

The desktop's wallpaper, behind every window on every screen (ADR-0030): the image chosen in the
desktop's menu or Settings, filling the screen, fitting in it or centred, or by default the Sector F Labs flask and name, in Ubuntu
Mono, in the colours of the skin.

## How it fits in

- **Started by** patina-compositor at start-up, before the bar, so it's there first.
- **A click-through surface on the background layer** of each output (patina-shell's host with
  `per_output` and `click_through`), so clicks on the desktop reach the compositor, which opens
  patina-menu on a right click.
- **What it draws** comes from the bar's config, `~/.config/patina-bar/config.toml`: `wallpaper`
  (an image), `wallpaper_fit` (`fill`, `fit` or `centre`, `patina_system::wallpaper::Fit`, which
  says where the image goes) and `skin` (the default's colours). Settings writes them, and patina-menu the skin; the
  wallpaper watches the file and redraws when any changes.
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

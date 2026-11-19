# Draw

A Patina (egui) bitmap paint program, the Unix MS Paint. Significant decisions are ADRs in
`docs/adr/`; use the `adr` skill to add one.

## Commands

```bash
make check         # tests + clippy -D warnings
make screenshot    # docs/screenshot-*.png, from a built-in demo picture, in a scratch config
make icons         # assets/draw-icon-128.png from packaging/linux/draw.svg
make install       # per user; test with PREFIX=<scratch dir>
```

Test and screenshot runs use `target/scratch-*` for config and data, never the real `~/.config/draw`.
`DRAW_SCREENSHOT=path.png` saves the window and quits; `DRAW_SKIN=name` picks that run's skin.

## Layout

- `src/bitmap.rs`: the pixels and every drawing operation, no egui; tested.
- `src/document.rs`: the open picture, tools, strokes (`begin`/`drag`/`end`) and undo; tested.
- `src/file.rs`: open and save on a thread, dialogs included (rfd); `Done` comes back over a channel.
- `src/app.rs`: the window: menus, toolbox, canvas, palette, dialogs. Keep logic out of it.
- `src/config.rs`: `~/.config/draw/config.toml`.

## Gotchas

- Ctrl+wheel arrives as egui's `zoom_delta`, not as scroll.
- The canvas reads raw pointer state rather than egui's drag events, which wait for movement and
  would drop the first pixel of a click (and Fill and Picker act on the press).
- On Wayland the window icon comes from the installed launcher, so `cargo run` shows it only after
  `make install`.

# patina-bar

A dwl status bar: egui on a wlr-layer-shell surface, skinned with Patina. Run as `dwl -s patina-bar`.

## Commands

- `make` lists targets; `make check` (tests + clippy -D warnings) before committing.
- Test ground is a FreeBSD VM from `bsdt.toml` running dwl from packages: `make vm`, then
  `make vm-restart` after changes and `make screenshot` (grim → docs/screenshot.png) to see the bar.
  Read the PNG back to check visually.
- Drive the VM's desktop with `bsdt key alt+shift+return` (new foot), `bsdt key alt+shift+3`
  (send window to tag 3), `bsdt type`. dwl's log is /tmp/bsdt-session.log in the guest; the bar's
  stderr goes there too.

## Layout

- `src/host.rs`: layer-shell egui host (sctk). Generic over a `Shell`; knows nothing about bars
  (ADR-0003). One surface + egui context per output.
- `src/raster.rs`: CPU painter for egui meshes into ARGB8888 wl_shm buffers (ADR-0002).
- `src/dwl.rs`: parser for dwl's status lines.
- `src/bar.rs`: the bar's modules and layout; `src/config.rs`: config.toml.

## Gotchas

- `bsdt exec -- cmd ~/x` expands `~` on the host; use `bsdt exec -- sh -c '... $HOME/x'`.
- dwl blocks if the bar stops reading stdin, so stdin stays on its own thread.
- Paint colours only from `ui.visuals()`, so skins reach them.
- In egui, item_spacing changed inside a row leaks to the next widget; change it in `ui.scope`.

# Patina desktop (patina-compositor + patina-bar)

A cargo workspace (ADR-0009): `compositor/` is a floating Smithay compositor (ADR-0006, 0007),
`bar/` is a layer-shell egui status bar skinned with Patina. The compositor starts the bar from
beside its own binary.

## Commands

- `make` lists targets; `make check` (workspace tests + clippy -D warnings) before committing.
- `make run` opens the compositor as a window on the current desktop (it pops up on the user's
  GNOME session, so prefer the VM).
- Test ground is a FreeBSD VM from `bsdt.toml`: dwl from packages is only the parent desktop;
  `gui.start` runs `target/release/patina-compositor` in it. `make vm`, then `make vm-restart`
  after changes, `make screenshot` (grim → docs/screenshot.png) and read the PNG back.
- The user often uses the VM live over VNC. Don't `bsdt type`/`bsdt key` into it without asking;
  grim screenshots are fine.
- Logs in the guest: /tmp/bsdt-gui.log (compositor, bar and foot), /tmp/bsdt-session.log (dwl).

## Layout

- `compositor/src/state.rs`: server side: protocols, placement, focus, input into the seat.
- `compositor/src/nested.rs`: the window on the parent desktop (sctk), pixman rendering copied
  into wl_shm, parent input → seat (ADR-0008). sctk traits are aliased (`Client*`) where their
  names clash with Smithay's.
- `bar/src/host.rs`: layer-shell egui host (sctk), generic over a `Shell` (ADR-0003).
- `bar/src/raster.rs`: CPU painter for egui meshes (ADR-0002).
- `bar/src/dwl.rs`: dwl's status lines; `bar/src/bar.rs`: modules; `bar/src/config.rs`.

## Gotchas

- `bsdt exec -- cmd ~/x` expands `~` on the host; use `bsdt exec -- sh -c '... $HOME/x'`.
- dwl blocks if a `-s` command stops reading stdin, so the bar reads stdin on its own thread.
- Paint colours only from `ui.visuals()`, so skins reach them.
- In egui, item_spacing changed inside a row leaks to the next widget; change it in `ui.scope`.
- sctk's `delegate_dispatch2!` blanket impl means extra client objects need a local user-data
  type implementing `Dispatch2`, not `()`.

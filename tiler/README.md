# patina-tiler

The Patina desktop's tiler (ADR-0053): it decides how windows tile, as Ubuntu does. The
compositor only reports drags and Super+arrows and places windows where it's told; this is the
policy, in a process of its own, so it can change and be restarted without restarting the
desktop.

## How it fits in

- **Started by** patina-compositor with the wallpaper and the bar, and started again when it
  exits; listed in Settings' Processes page, where Restart picks up a newly installed build.
- **Talks to** the compositor's `com.sectorflabs.Patina.Tiling` on the session bus (ADR-0049),
  through `patina_system::tiling`: drags, Super+arrows, work-area changes and windows opening
  and closing in; `Place`, `Move`, `Restore` and the preview out.
- **What it does:** a window dragged so the pointer meets the left or right edge takes that half,
  and the top fills the screen (tiled, not maximised; ADR-0052), with the preview while it's
  held there. Super+Left/Right tile, or restore from the other half; Up fills, Down restores.
  Gaps (ADR-0051) come from `WorkArea()` (inside the outer gaps) and `Gap()`. When the line
  between tiled windows is dragged (the compositor's `Resized`, ADR-0054), it keeps the split,
  and a window tiled to the other side takes the rest. A window opens where its app's window was
  last (`WindowOpened`, saved from `WindowClosed` and drops) if no other of its app's is open,
  and fills if it's too big for the work area or closed filled (ADR-0065). It exits when the
  compositor it started with leaves the bus.
- **Gives way** to another tiler: it holds `com.sectorflabs.Patina.Tiler` allowing replacement
  (`claim_default`), so one that `claim`s it (`cargo run -p patina-system --example thirds`)
  takes over, and patina-tiler decides again when it leaves. A newer patina-tiler takes over
  from an older one still running.

## Code

`policy.rs`, what tiles where, as pure functions with their tests; `positions.rs`, where apps'
windows were, in `~/.local/state/patina-tiler/windows.toml`; `main.rs`, the bus.

## Running

```sh
cargo test -p patina-tiler
pkill patina-tiler   # inside the desktop: the compositor starts the installed one again
```

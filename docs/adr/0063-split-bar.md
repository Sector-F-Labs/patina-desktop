# 0063. The bar can be split into its parts, placed anywhere along the edges

Date: 2026-10-17

## Status

Accepted, as an experiment (`layout = "split"`, off by default). The user asked for an optional
mode where the launcher, the window list and the tray each sit on their own, moved anywhere
along the screen's edges, with columns at the sides. They also asked for the menus they open to
come from their buttons, and for the parts to be placed in Settings by dragging them on a map.

## Context

The bar is one layer-shell surface across an edge (ADR-0011), holding modules in three groups:
left, centre and right. People who like docks (macOS, Ubuntu's dock on the left), or a tray in
the top corner, can't have them with one strip.

Constraints that shaped the design:

- **Surfaces.** A layer surface draws only inside itself, and patina-shell's `Host` drives one set
  of surfaces from one `Options`. Separate places mean separate surfaces.
- **Reserving space.** The compositor reserves space for a layer surface from its anchors. For a
  surface in a corner, Smithay takes the side edge (a bottom-left surface keeps the left edge,
  and a bottom-right one the right edge), so a corner part can't reserve its own edge.
- **Menus.** The pieces the bar opens (Wi-Fi, volume, power, the launcher) placed themselves on
  the bar's edge, in a fixed corner. They didn't know where their button was.
- **Already split.** The bar already draws from module lists, so a part is the bar drawing one
  module list.

## Decision

- **Parts are separate bar processes.**
  - `patina-hud` in split mode draws the window list.
  - It runs `patina-hud --section launcher` and `--section tray` with a pipe on their standard
    input. They end when it closes, as when the bar exits, or when it restarts itself in place
    (exec) after a change to the layout keys. Before restarting, it waits for them, so none is
    left a zombie.
  - The tray part draws the bar's other modules (tray, clock, power). dwl's modules, which say
    nothing under Patina's compositor, are left out.
- **Twelve spots.** Each part goes in a spot named for its edge, then where along it:
  `bottom-left` is a row along the bottom at the left, and `left-bottom` is a column up the
  left side at the bottom. Earlier names (`bottom`, `center-left`) still read.
  - Up a side, the window list is a column of icons, with the same clicks and tooltips as tabs.
    The tray stacks its icons, the clock shows hours over minutes, and the battery is its icon
    alone.
- **Parts stack where their sizes are known.** A column at a side's end sits past a row in that
  corner. A part sharing the launcher's spot lines up after the launcher. Other parts in one
  spot overlap.
- **Each part sizes its surface to what it shows**, through a new `Shell::wanted_size` that
  patina-shell checks after every frame (`Host::set_size`).
- **The parts reserve no space; "reserve" sections do.** Each edge with a part on it gets a
  `--section reserve-EDGE`: a pixel-thin surface along the whole edge, clicks passing through,
  keeping the bar's thickness and gap.
- **Menus, tooltips and pieces open from their button.** Placing them needs the screen's size,
  which patina-shell now passes on (`output_size`).
  - `split::Placement::popup` turns a point on a part into a place on the screen:
    `edge,x,y,far`.
  - patina-menu takes `--edge left` and `right`, for columns.
  - The pieces get the place in `PATINA_OPEN_AT`, and patina-shell's `piece::opening` anchors
    them there, turned towards the side with more room.
  - The single bar uses the same path, so Wi-Fi now opens above the Wi-Fi icon.
- **Settings' Bar page** chooses one bar or split. When split, it draws a map of the screen
  with each part's mark: a row's mark lies along its edge, a column's stands up its side.
  Dragging a mark snaps it to the nearest spot, and letting go saves it. Lists give the same
  choice by keyboard.
- **Patina's `TabStrip` gains `separator(false)`**, for tabs that stand alone: the split window
  list has no line before its first tab.

## Consequences

- The bar can be a dock, a corner tray, or a column, with no separate program. The pieces open
  where their buttons are, in either layout.
- Split, the bar is two to six processes, and each restart re-execs the bar. A crash in one
  part leaves the others until the bar notices nothing.
- Two parts in one spot overlap, unless one is the launcher. A long window list can grow into
  its neighbours.
- With several monitors, every part has the same size on each, and menus open on the output the
  compositor chooses.
- The launcher reads `PATINA_OPEN_AT` only from the patina-launcher build that has it.

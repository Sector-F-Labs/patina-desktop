# 0056. The bar's menus open in patina-menu

Date: 2026-10-17

## Status

Accepted. The user saw the bar's menus cut off, since they could only draw inside the bar, and
suggested making patina-menu a generic tool that can be filled with any menu.

## Context

A layer-shell surface draws only inside itself, and the bar's is 41 px tall. egui draws menus
and tooltips as areas in the same surface, so a window tab's right-click menu (ADR-0055) was cut
off at the bar's edge.

Options considered:

- **Grow the bar's surface while a menu or tooltip shows.** The extra room would be transparent,
  with an input region over just the bar and the menu. egui's own menus and tooltips would work
  unchanged. But a bottom bar grows upward, so its contents move inside the surface. egui has
  already placed the popup by the time the bar knows it's open. And a click outside the room
  never reaches the bar, so the menu can't close.
- **xdg_popup on the layer surface**, the Wayland way: the compositor grabs, and dismisses on a
  click elsewhere. But egui draws every popup into the one surface, so the host would have to
  split one egui frame across two surfaces, positioner and all.
- **A menu in a process of its own, over the whole screen.** patina-menu already does this for
  the desktop's menu (ADR-0029): a transparent overlay surface that takes the keyboard, sits
  anywhere, keeps the menu on screen, and closes on a click elsewhere, Escape or losing the
  keyboard. The bar already opens its tray menus as pieces of their own (ADR-0021, ADR-0022,
  ADR-0048).

## Decision

- **patina-menu also shows a menu of any items.**
  - Arguments: `--item ID=LABEL` and `--separator`, in order, at `--at X,Y`. `--edge bottom`
    counts Y up from the screen's bottom and opens the menu upward.
  - It prints the chosen item's ID and exits, or prints nothing if closed without a choice.
  - It wears the bar's skin. With no items, it's the desktop's menu as before.
  - The placing and closing are shared (`overlay.rs`).
- **Patina's `TabStrip` gains `own_menu`.** A right click calls it with the tab and the pointer,
  and the strip draws no menu of its own.
- **The bar's window tabs use it** (`bar/src/menu.rs`). A right click runs `patina-menu` just past
  the bar's edge, under the pointer: New Window and Close Window. The bar runs it as its own
  child, not through the compositor's `Spawn`, to read the answer, which comes back to the
  event loop.

## Consequences

- The bar's menus are never cut off, and they behave like the desktop's menu: keyboard, Escape,
  click elsewhere.
- Opening one starts a process, and the menu takes a moment to appear.
- A menu's items are strings on a command line. Submenus, checkmarks and icons would need more
  than `--item`.
- The menu opens on the output the compositor puts it on, not necessarily the one under the bar.
- Tooltips weren't solved by this. They followed in
  [ADR-0057](0057-the-bars-tooltips-open-in-patina-menu.md), the same way.

# 0001. Build patina-hud as an egui layer-shell client on Patina

Date: 2026-10-14

## Status

Accepted

## Context

dwl draws no bar of its own; its README leaves that to an external client. The "Patina desktop"
idea (2026-10-13) is to work backwards from dwl and build the shell out of egui components styled
with Patina skins, starting with a bar, then a launcher, notifications and a wallpaper. Waybar,
somebar and dwlb already exist, but none of them takes Patina skins, and the point is a desktop
whose shell matches its apps.

eframe/winit can only make xdg-shell windows, which dwl tiles like any other app. A bar has to be
a wlr-layer-shell surface, anchored to an edge with an exclusive zone, so eframe is out.

## Decision

We will build patina-hud in Rust with egui, driven directly rather than through eframe, on a
wlr-layer-shell surface (smithay-client-toolkit), and take its look from
[Patina](https://github.com/Sector-F-Labs/patina) (`patina-ui`):

- Skins through `patina::skin`: Default, Patina's built-ins, and the bar's own folder
  `~/.config/patina-hud/skins` (`skin::app_dir`). The skin is picked by name in the bar's config.
- Default follows the desktop's light/dark setting through `patina::appearance`.
- Stock egui widgets, fonts and visuals otherwise; the bar paints only its tags itself, in colours
  from the active visuals.
- No window frame or title bar: a bar has neither.

## Consequences

- The bar takes the same skins as the Patina apps.
- It moves to a new egui version only together with Patina.
- It needs a host for egui on layer-shell, which eframe won't provide (ADR-0003).
- It only runs on compositors with wlr-layer-shell (dwl, sway, Hyprland, river...), not GNOME.

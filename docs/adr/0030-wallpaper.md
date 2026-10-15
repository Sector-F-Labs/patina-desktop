# 0030. A wallpaper piece, chosen from the desktop's menu, with a Sector F Labs default in the skin's colours

Date: 2026-10-15

## Status

Accepted

## Context

The desktop showed the compositor's clear colour behind the windows. The user asked for wallpapers,
chosen the same way as the skin (ADR-0029): from the desktop's menu, kept in the bar's config. They
also asked for a default with the Sector F Labs logo and the name in Ubuntu Mono, dark to match
egui's default dark theme, and matching the other skins.

The compositor could draw the wallpaper, but it has three backends (GL, pixman, the hardware), and
a layer-shell background surface is what wlroots desktops use (swaybg); it also works on any
layer-shell compositor and keeps the image decoding out of the display server. Shipping one image
per skin was ruled out: a drawn default follows any skin, including a user's own.

## Decision

- **patina-wallpaper**, a new piece, puts a surface on the background layer of every output,
  under the bar (exclusive zone -1) and click-through (an empty input region, new
  `Options::click_through`), so a right click still reaches the desktop and the pointer moving
  redraws nothing. The compositor starts it before the bar.
- It draws `wallpaper = "…"` from the bar's config, an image (JPEG, PNG or WebP) scaled to fill each
  output and cropped, decoded once and scaled once per output size. The host now lets egui make
  textures up to 8192 pixels a side (egui's 2048 is a GPU's limit, not the CPU painter's).
- Without one it draws the default: the Sector F flask, a fifth of the screen, over "Sector F
  Labs" in Ubuntu Mono (bundled, with its licence), on the skin's panel colour; the flask in the
  skin's accent (its selection colour) if that stands out, else the text colour, and the name in a
  muted text colour. On egui's dark theme that's the blue flask on egui's dark grey.
- It checks the bar's config each second and redraws when the wallpaper or skin changes.
- The desktop's menu gains a Wallpaper submenu: the default ("Sector F Labs"), the images in
  `~/Pictures/Wallpapers` and the system's backgrounds folder, and Open wallpapers folder.
- The logo and font moved to patina-shell (`logo.rs`), shared by the bar's launcher button and the
  wallpaper.

## Consequences

- The desktop has a wallpaper on every backend and on other layer-shell compositors, and the
  default always matches the skin.
- Scaling a large photo takes a moment of CPU when it's chosen or the screen changes size; it isn't
  redrawn otherwise.
- Only fill-and-crop; no centre, fit or tile, and no per-output wallpapers.
- With the menu's surface on top, the wallpaper list is names only, no thumbnails.

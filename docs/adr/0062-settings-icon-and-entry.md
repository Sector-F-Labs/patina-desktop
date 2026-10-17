# 0062. Settings has an icon and a desktop entry

Date: 2026-10-17

## Status

Accepted. The user asked for the Settings app to have an icon and its own desktop entry.

## Context

patina-settings was reached from the desktop's menu and the bar, but it had no `.desktop` entry.
So the launcher didn't list it, the bar's tab showed a letter instead of an icon and its app id
as its name (ADR-0050, ADR-0061), and `patina-raise` couldn't find it by name. The Patina apps
(Teleprinter, Reader, Files) each install an entry and a hicolor icon from `make install`, and
draw their window icon from the same artwork.

## Decision

- **An icon, `settings/packaging/settings.svg`,** in the family's style: a warm grey control
  panel of sliders, one knob in Patina orange, and a steel gear over its corner.
  - `make install` puts it in the hicolor theme as `com.sectorflabs.Settings.svg`.
  - The window icon is drawn from the same SVG at start-up, with `patina_system::logo::render_svg`
    (resvg, already in the workspace), so there's no PNG to keep in step.
- **A desktop entry, `com.sectorflabs.Settings.desktop`,** named Settings, in the Settings
  category, with `StartupWMClass` matching the app id.
  - `make install` writes it to `~/.local/share/applications` with Exec's absolute path, and
    `make uninstall` removes it.
  - It's `OnlyShowIn=Patina`, the name the compositor puts in `XDG_CURRENT_DESKTOP` and the
    launcher checks, so GNOME doesn't list a second "Settings".
    `desktop-file-validate` wants unregistered names spelled `X-Patina`, but that wouldn't match.

## Consequences

- Settings is in the launcher, its tab has its icon and reads "Settings - …", and
  `patina-raise -i settings` finds it.
- `desktop-file-validate` reports the `OnlyShowIn` value as an error. It's a deliberate choice.
- Under other desktops it isn't listed. It can still be run as `patina-settings`.

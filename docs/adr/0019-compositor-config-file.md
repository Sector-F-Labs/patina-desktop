# 0019. A config file for the compositor, reloaded with Super+Shift+R

Date: 2026-10-14

## Status

Accepted

## Context

On the laptop the compositor's settings (scale, keyboard layout) came from environment variables
(`PATINA_SCALE`, `XKB_DEFAULT_*`) or the system, and a login session doesn't read `.zshrc`. The
user asked for a config file, reloadable live with a shortcut, and for natural scrolling on by
default.

## Decision

`~/.config/patina-compositor/config.toml`, in the style of patina-bar's: every key optional, unknown
keys and out-of-range values are warnings, and a broken file never stops the session (the defaults
at start-up, the settings in use on a reload).

- `scale`: unset means the backend's own choice (the panel's density, or the parent desktop's
  scale when nested).
- `[keyboard]`: `layout`, `variant`, `model`, `options` (unset: `/etc/default/keyboard`),
  `repeat_delay` (200 ms) and `repeat_rate` (25/s).
- `[touchpad]`: `tap` (on), `natural_scroll` (on), `speed` (-1 to 1, default 0); touchpads only,
  not mice.

Environment variables still come first, for quick tests: `PATINA_SCALE`, then the file, then the
system or the backend. **Super+Shift+R** reads the file again and applies it straight away: a new
scale to every surface, the cursor and maximised windows; the layout and repeat to the keyboard;
the touchpad settings to every touchpad.

The first start writes the file where there's none, with every setting commented out at its
default (`compositor/config.example.toml`, checked by a test to parse to exactly the defaults), so
the settings can be found; it never replaces a file.

## Consequences

- Settings live in one file a future Patina settings app can edit.
- No file watching yet: a change applies at the next login or on Super+Shift+R.
- One scale for the one output; per-monitor sections come with multiple monitors.

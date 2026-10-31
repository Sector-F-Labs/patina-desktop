# 0100. The desktop's default skin is Everforest Dark

Date: 2026-10-31

## Status

Accepted

## Context

The shell's skin is the bar's `skin` key (ADR-0029), and with none set every piece used Patina's
`Default`. That is the right default for a Patina app, which should look as the platform does. For
the desktop it is the plain grey face of a first start, and Everforest Dark, a built-in skin with
low-contrast green-tinted greys and muted accents, is the one the desktop was being shown in.

Changing Patina's `DEFAULT_SKIN` would restyle every Patina app and the base skin its format falls
back on. The default was also in several places: the bar's config, the fallback each piece took
when the bar's file said nothing (`patina_shell::piece::skin`), Settings, the process manager and
the desktop menu, and the compositor's frames for X11 windows. Changing only the bar's would have
left the wallpaper, the menus and the frames in `Default`.

## Decision

- patina-shell has `bar::DEFAULT_SKIN`, "Everforest Dark", and `bar::skin_or_default()`: the
  bar's `skin` (or `PATINA_SKIN`), else that. Every piece's skin comes from it.
- The bar's config default, Settings, the process manager, the desktop menu and the compositor's
  frames use it too, so nothing falls back to `Default` by itself.
- A skin the bar's file names that isn't there is still `Default`, with a warning, as before.
- Patina's `DEFAULT_SKIN` is unchanged: Patina apps keep `Default`.

## Consequences

- A first start, and anyone whose `config.toml` has no `skin` line, now gets Everforest Dark.
  A file that says `skin = "Default"` keeps it.
- The shell's pieces and the launcher agree, from the one constant in patina-shell, so a later
  change of default is one line there.
- Everforest Dark is built into Patina; if it were removed from there, `DEFAULT_SKIN` would fall
  back to `Default` and a test in patina-shell would fail first.

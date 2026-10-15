# 0033. The volume flash holds through repeated keys, and shows brightness too

Date: 2026-10-15

## Status

Accepted

## Context

After a volume key the compositor runs `patina-volume --flash`, which fades in, holds and fades out
(ADR-0021). A second key tells the flash that's up, which restarted its clock at zero, so the next
frame began the fade-in again from transparent: holding or tapping the volume key made the
indicator blink on every press instead of staying up. Its level was also read only every 400 ms,
so the number lagged behind the presses.

The brightness keys (ADR-0020) changed the backlight with nothing on screen. ADR-0021 noted that
the same flash could carry it, and the principle of not repeating the UI (ADR-0027) says it should,
rather than a second indicator.

## Decision

- A key again keeps the flash shown: still fading in, it carries on; shown or fading out, it's at
  full opacity and holds from the start. It never goes back to the start of the fade.
- Each key also reads the levels at once (the poll thread waits on a channel, not a sleep), so the
  number keeps up.
- `patina-volume --brightness`, which the compositor runs after a brightness key, flashes the same
  indicator with a sun (its rays growing with the level), the brightness and a slider that sets it.
  A volume key while it's up switches it to the volume, and back, so one indicator shows at a time;
  the menu from the bar stays the volume.
- The backlight code (finding the device, reading it, stepping or setting it through logind's
  SetBrightness) moves from the compositor to patina-shell's `backlight`, used by both the keys and
  the slider; the compositor now depends on patina-shell.

## Consequences

- Holding a volume or brightness key shows one steady indicator that follows the level.
- Brightness can be set with the slider as well as the keys.
- `[media] volume_menu` now names what both kinds of key flash.
- Without a backlight (a desktop monitor), the brightness flash says so; the keys did nothing there
  before either.

# 0020. Brightness through logind, volume through configurable commands

Date: 2026-10-14

## Status

Accepted

## Context

On the laptop the compositor is the whole session, so nothing handled the brightness and volume
keys; the user asked for them next. Brightness can be set by writing the backlight's sysfs file,
which needs root, by a helper such as `brightnessctl` (not installed on nifl), or through logind's
`Session.SetBrightness`, which lets the session on the seat set it without root; GNOME uses that.
Volume has no such interface: it belongs to the sound server, PipeWire on Ubuntu, whose `wpctl` is
installed with it; FreeBSD uses `mixer`.

## Decision

The media keys (XF86MonBrightnessUp/Down, XF86AudioRaiseVolume/LowerVolume, XF86AudioMute and
XF86AudioMicMute) are the compositor's own, and each press runs on a thread of its own:

- **Brightness:** the backlight from `/sys/class/backlight` (firmware, then platform, then raw),
  stepped by `brightness_step` percent of its range (5 by default) and never below 1%, set through
  logind's `SetBrightness`.
- **Volume and microphone:** commands from the config's `[media]` section, run through sh; the
  defaults are `wpctl` for PipeWire, and "" turns a key off.

## Consequences

- Works on any systemd session with no extra packages; logind refuses sessions without a seat
  (ssh), which is what it should do.
- No on-screen feedback yet: an OSD layer surface showing the level is a later shell piece.
- Holding a key doesn't repeat; each press is one step.
- FreeBSD (no logind) needs its own brightness path; its volume works by setting the commands to
  `mixer`.

# 0025. A lock screen through ext-session-lock, swaylock by default

Date: 2026-10-15

## Status

Accepted; the lock key is superseded by [ADR-0026](0026-lock-with-ctrl-super-q.md), swaylock as the default
by [ADR-0028](0028-patina-lock.md)

## Context

On the laptop, closing the lid suspends it through logind, as it should, but nothing locked the
screen: opening the lid showed the unlocked desktop. GNOME locks on suspend with its own shell. The
standard protocol for a separate locker is `ext-session-lock-v1`, which Smithay implements; swaylock
and others speak it. The user asked for swaylock first and for the locker to be configurable, with
Super+L to lock as on Ubuntu.

## Decision

- The compositor implements ext-session-lock. Once a locker locks, only its surface is drawn (black
  until it has one, on every backend) and only it gets input: the pointer, the keyboard (no window,
  launcher or menu can take focus), and no shortcuts except switching consoles and the brightness
  and volume keys. If the locker dies, the session stays locked, and a new locker can take over.
- `[lock] command` (default `swaylock -f -c 1b1b1b`) is run by **Super+L** and **Super+Escape**.
- With `[lock] on_suspend` (on by default), on the hardware, the compositor holds a logind "delay"
  sleep inhibitor; when logind announces sleep, it runs the locker and lets go once the lock has been
  drawn (or after 3 s), then takes the inhibitor again after waking.
- Super+L was tile right (ADR-0023); tiling right is now Super+Right only.

## Consequences

- Waking the laptop shows the lock screen, not the desktop.
- The locker must be installed (`swaylock` on Ubuntu) and checks the password through PAM itself.
- If no locker runs, the session isn't locked; the compositor doesn't draw a lock screen of its
  own. A Patina locker, in the desktop's skins, can replace swaylock through the same config key.

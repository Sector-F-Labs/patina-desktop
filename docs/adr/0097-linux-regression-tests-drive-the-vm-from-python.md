# 0097. Regression tests drive the Linux VM from Python, with uv

Date: 2026-10-29

## Status

Accepted: 12 tests pass, repeatedly, and fail when the Linux battery reader is broken on purpose.

## Context

Supporting FreeBSD (ADR-0095) changes code that Linux uses, and Linux was only checked by hand,
on a laptop. The Linux VM (ADR-0017) with its fake battery and Wi-Fi (ADR-0096) can be checked by
a program, which a bash script does badly: screenshots to compare, polling for a window to
appear, and failures that say what was expected.

## Decision

`vm-linux/tests/` is a Python project run by uv (`make test-vm`, or `uv run pytest` there), using
pytest and Pillow. `vm.sh` stays bash and does the VM; the tests call it (`ssh`, `shot`, `fake`).

- **The VM is a fixture.** It boots the VM if it isn't up and waits for the session.
- **`test_session.py`:** the compositor, bar and wallpaper are running, nothing has panicked, and
  the screen isn't flat.
- **`test_apps.py`:** Settings, the Process Manager, Teleprinter and foot each open and draw: the
  screen above the bar changes, even slightly. A window that maps but never draws (Settings on
  FreeBSD before `WGPU_BACKEND=gl`) changes nothing.
- **`test_hardware.py`:** the bar's tray changes when the fake battery's charge and state change,
  when the kernel's battery goes, and when NetworkManager joins the fake access point.
- **No golden images.** Fonts and skins move pixels; a test asks whether a region changed, not
  what it looks like. The tray is a region left of the clock, which changes every minute, so
  the bar's layout (flat or floating) doesn't matter.
- **The guest's screen stays on** (`off_after = 0`, set by `guest-setup.sh`): the compositor
  blanks and locks it after five idle minutes, and every screenshot would be black.
- FreeBSD isn't tested this way yet; the point is that Linux doesn't regress.

## Consequences

- `make test-vm` takes about a minute on a running VM, and boots it (minutes) if it's down.
- Needs uv, QEMU and the VM's prerequisites on the machine running it; not part of `make check`.
- The tests find what Linux lost, not what FreeBSD gained.
- Changing the bar's tray so the icons leave the strip left of the clock needs `ICONS` in
  `test_hardware.py` changed with it.
- Not covered yet: joining a network from the Wi-Fi menu (the test joins with `nmcli`, as root,
  and checks the bar), Files, the lock screen, keys and the pointer.

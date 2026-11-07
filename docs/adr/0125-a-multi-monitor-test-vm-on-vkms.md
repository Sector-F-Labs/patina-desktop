# 0125. A multi-monitor test VM on vkms

Date: 2026-11-08

## Status

Accepted

Adds to [0017](0017-linux-test-vm.md), the Linux VM, and [0097](0097-vm-regression-tests.md), its tests.

## Context

Several monitors (ADR-0121, ADR-0124) had only been tried on nifl with one external monitor, by hand:
plugging in, unplugging, a degraded link, a stale gamma ramp. None of it could be run again, and the
hot-desk behaviour (two Dells of one model, a third never seen) needs more monitors than anyone owns.
The existing guest has one virtio GPU with one monitor, which QEMU can neither add to nor take away.

## Decision

- **A second guest, `PATINA_VM_INSTANCE=displays`**, beside the dev one (its own disk, ports 2225 and
  VNC 4): the same image, the same shared checkout, but **vkms**, the kernel's virtual display driver,
  in place of the virtio GPU (`-vga none`). The compositor runs on it as on real hardware: DRM
  and GBM, one `DrmCompositor` for each connector, udev connector changes.
- **vkms's configfs makes the monitors** (`vkms.sh up`): three connectors, each with its own plane,
  CRTC and encoder, the first connected and the rest not. Writing a connector's `status` plugs or
  unplugs it, and the kernel sends the same udev `change` event a cable does. `vm.sh display plug N
  PRESET` and `unplug N` do it; a systemd unit makes them before the session starts.
- **A fake EDID stands in for the monitor's own.** vkms on this kernel (7.0) reads no EDID, and
  DRM's debugfs `edid_override` is ignored by it, so a monitor has neither a model nor a size.
  `PATINA_FAKE_EDID_DIR` makes the compositor read a connector's EDID from `<connector>.bin` in that
  folder instead of from the kernel; nothing but the VM sets it, and without it nothing changes.
  `edid.py` makes them (a laptop panel, a 21:9 LG that prefers 4K, Dells that differ in serial only).
  The panel's size comes from the EDID when the connector reports none.
- **The screen is captured from inside**, with the compositor's own `capture` example, since QEMU has no
  display to dump. `vm.sh shot` does it for this guest; `vm.sh session CMD` runs a command in the
  session's environment, which is how the tests reach the Displays service.
- **`make test-vm-displays`** runs `vm-linux/tests/test_displays.py`: a monitor plugged in stays off,
  extending drives it, unplugging drops it, a placement is remembered and listed, a hot desk's monitor
  of the same model goes where one was put, Forget makes a model new again, and the primary alone.

## Consequences

- The behaviours that were tried by hand are tests, and a change that breaks hotplug, the saved layout
  or the idle-until-chosen rule fails one.
- What vkms can't give isn't tested end to end: modes that match a panel's shape (its modes come from
  no EDID, so `pick_mode` is tested with unit tests only), a link that degrades, and real scanout.
- The compositor has one test-only environment variable, read in one place.
- The guest has no screen to look at: a person debugging it reads captures, not a VNC window.
- It needs the dev guest's image and the same packages, so the first boot downloads and installs as that
  one did.

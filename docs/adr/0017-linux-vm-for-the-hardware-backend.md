# 0017. Test the hardware backend in a Linux VM with a virtio GPU

Date: 2026-10-14

## Status

Proposed

## Context

The hardware backend (ADR-0016) has to own a GPU, its monitors and the input devices through a login
session. None of the test grounds so far can give it that:

- **This machine, inside GNOME:** GNOME holds the only GPU (`card1`, an RTX 2000 Ada), so libseat
  can't open it. The backend exits, by design.
- **A text console on this machine, or the user's Intel laptop:** the real thing, but each try costs
  the user a console switch and a report back, and a hang means a black screen to recover from.
- **bsdt's FreeBSD VM** (ADR-0005): FreeBSD has no KMS driver for QEMU's graphics, so there's no DRM
  device at all; that's why dwl runs headless there.
- **An LXC/LXD container:** it can be given a render node for GPU rendering, but not a seat, a
  console or KMS connectors.
- **GPU virtualisation:** the card's virtualisation mode is None, and a virtual function has no
  connectors anyway.

A Linux guest in QEMU has `virtio_gpu`, a real KMS driver: `/dev/dri/card0` with a "Virtual-1"
connector whose modes follow the display, input through virtio or USB devices, systemd-logind
seats and real consoles. With `virtio-vga-gl` (virgl) the guest's GL runs on the host's GPU through
virglrenderer, so GBM, EGL and dmabuf work as on hardware. Smithay's own developers test this way.

## Decision

A Linux test VM, driven from this repo, beside bsdt's FreeBSD one:

- **The guest:** an Ubuntu cloud image of the same release as the host (26.04), so binaries built
  on the host run in it unchanged, on an overlay disk so a broken guest is one command from fresh.
  cloud-init installs the runtime packages (libinput, libseat, Mesa, foot, a cursor theme) and a
  user.
- **The machine:** QEMU with KVM and `-device virtio-vga`, shown over VNC (localhost:5903) so it
  needs no window on the desktop. Mesa draws in software in the guest for now: `virtio-vga-gl`
  (virgl) needs a GL display (`egl-headless`), and the host's QEMU has only `none` and `curses`
  without the `qemu-system-modules-opengl` package. With it installed, the GL device is a one-line
  change.
- **The seed:** cloud-init's NoCloud files in a folder QEMU shows as a FAT disk labelled CIDATA
  (vvfat), so no ISO tool is needed on the host.
- **The binaries:** built on the host (`make build`) and shared read-only into the guest over 9p
  (`-virtfs`, no daemon to run): this checkout at `/patina`, and Teleprinter's beside it at
  `/teleprinter` when it's there. A rebuild needs no copy and no Rust toolchain in the guest.
- **The session:** getty logs the test user in on tty1, and their `~/.profile` starts
  `patina-session` there under `dbus-run-session`, as a login session would.
  `vm-linux/guest-setup.sh`, run from the share on every start and restart, writes that profile
  and the launcher entries for Settings, the Process Manager and Teleprinter, so changing it
  needs no fresh guest.
- **Driving it:** SSH as the test user on localhost:2224, with a key made for the VM, and QEMU's
  monitor for `screendump`. Restarting the session ends the user's login (`loginctl
  terminate-user`); getty logs them in again.
- **Make targets:** `vm-linux` (create or boot), `vm-linux-restart` (rebuild, then restart the
  session), `vm-linux-shot` (screenshot), `vm-linux-gui` and `vm-linux-stop`, in the same style as
  the FreeBSD ones; `vm-linux/vm.sh` does the work, and `vm-linux/vm.sh reset` deletes the guest's
  disk for a fresh one.

## Consequences

- The hardware path (libseat, KMS mode-setting and page flips, libinput, console switching, GBM
  and dmabuf) can be exercised and fixed without the user, and a crash costs a VM restart.
- virtio-gpu behaves like a simple, well-behaved driver. It won't show what NVIDIA's driver (GBM
  quirks, explicit sync, cursor planes) or the Intel laptop's real panel does differently; those
  still need a text console on the real machines.
- The first run downloads a cloud image (about 800 MB) into `~/.cache/patina-desktop`; the
  guest's overlay disk and key are in `target/vm-linux`.
- Building on the host ties the guest to the host's Ubuntu release; when the host moves on, the
  guest image moves with it.
- It's QEMU wiring, not bsdt. If it proves useful, Linux guests could become a bsdt feature, but
  bsdt stays focused on the BSDs until then.

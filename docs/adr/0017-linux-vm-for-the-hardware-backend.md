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
- **The machine:** QEMU with KVM and `-device virtio-vga-gl`, displayed with `egl-headless` and
  VNC so it needs no window on the desktop (`make vm-linux-gui` opens a viewer for a person);
  `-display gtk,gl=on` when someone wants it in a window.
- **The binaries:** built on the host (`make build`) and shared read-only into the guest with
  virtiofs or 9p, so a rebuild needs no copy and no Rust toolchain in the guest.
- **The session:** getty logs the test user in on tty1, and their `~/.profile` starts
  `patina-compositor` there, so the VM boots straight into the hardware backend, as a login
  session would.
- **Driving it:** QEMU's monitor (QMP) gives `screendump` for screenshots and `send-key` and
  `input-send-event` for keys and pointer, so a change can be checked without a person: rebuild,
  restart the session, screenshot, read the PNG.
- **Make targets:** `vm-linux` (create or boot), `vm-linux-restart` (restart the session after a
  rebuild), `vm-linux-shot` (screenshot) and `vm-linux-gui`, in the same style as the FreeBSD ones.

## Consequences

- The hardware path (libseat, KMS mode-setting and page flips, libinput, console switching, GBM
  and dmabuf) can be exercised and fixed without the user, and a crash costs a VM restart.
- virtio-gpu behaves like a simple, well-behaved driver. It won't show what NVIDIA's driver (GBM
  quirks, explicit sync, cursor planes) or the Intel laptop's real panel does differently; those
  still need a text console on the real machines.
- The first run downloads a cloud image (about 700 MB) into a cache outside the repo.
- Building on the host ties the guest to the host's Ubuntu release; when the host moves on, the
  guest image moves with it.
- It's QEMU wiring, not bsdt. If it proves useful, Linux guests could become a bsdt feature, but
  bsdt stays focused on the BSDs until then.

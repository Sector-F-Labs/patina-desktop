#!/usr/bin/env python3
"""Makes EDIDs for the multi-monitor test VM's fake monitors (ADR-0125).

    edid.py NAME > file     one of the presets below
    edid.py --list          the presets

An EDID says who made a monitor, which model and serial it is, how big it is
and which modes it takes, which is everything the compositor goes by. vkms has
no EDID of its own on this kernel; vkms.sh writes these through DRM's debugfs
`edid_override`.
"""

import sys


def pnp(letters: str) -> bytes:
    """The three-letter maker id, five bits a letter."""
    a, b, c = (ord(x) - 64 for x in letters)
    return ((a << 10) | (b << 5) | c).to_bytes(2, "big")


def dtd(w: int, h: int, hz: int, mm: tuple[int, int]) -> bytes:
    """A detailed timing for w x h at hz, with reduced blanking (CVT-RB-like)."""
    hblank, hfront, hsync = 160, 48, 32
    vfront, vsync, vback = 3, 5, 32
    vblank = vfront + vsync + vback
    clock = (w + hblank) * (h + vblank) * hz // 10_000  # in 10 kHz
    d = bytearray(18)
    d[0:2] = clock.to_bytes(2, "little")
    d[2], d[3] = w & 0xFF, hblank & 0xFF
    d[4] = ((w >> 8) << 4) | (hblank >> 8)
    d[5], d[6] = h & 0xFF, vblank & 0xFF
    d[7] = ((h >> 8) << 4) | (vblank >> 8)
    d[8], d[9] = hfront & 0xFF, hsync & 0xFF
    d[10] = ((vfront & 0xF) << 4) | (vsync & 0xF)
    d[11] = ((hfront >> 8) << 6) | ((hsync >> 8) << 4) | ((vfront >> 4) << 2) | (vsync >> 4)
    d[12], d[13] = mm[0] & 0xFF, mm[1] & 0xFF
    d[14] = ((mm[0] >> 8) << 4) | (mm[1] >> 8)
    d[17] = 0x1E  # digital, separate sync, positive both
    return bytes(d)


def text(kind: int, value: str) -> bytes:
    d = bytearray(18)
    d[3] = kind
    body = (value + "\n").encode("ascii")[:13].ljust(13, b" ")
    d[5:18] = body
    return bytes(d)


def edid(make, product, serial, name, modes, mm, serial_text=None) -> bytes:
    """`modes` are (w, h, hz), the first the preferred; `mm` the panel's size."""
    b = bytearray(128)
    b[0:8] = bytes([0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0])
    b[8:10] = pnp(make)
    b[10:12] = product.to_bytes(2, "little")
    b[12:16] = serial.to_bytes(4, "little")
    b[16], b[17] = 10, 34  # week 10 of 2024
    b[18], b[19] = 1, 4  # EDID 1.4
    b[20] = 0xA5  # digital, 8 bits, DisplayPort
    b[21], b[22] = round(mm[0] / 10), round(mm[1] / 10)  # cm
    b[23] = 120  # gamma 2.2
    b[24] = 0x0A
    b[38:54] = bytes([0x01, 0x01] * 8)  # no standard timings
    slots = [dtd(*m, mm) for m in modes[:2]]
    if name:
        slots.append(text(0xFC, name))
    if serial_text:
        slots.append(text(0xFF, serial_text))
    for i, slot in enumerate(slots[:4]):
        b[54 + 18 * i : 72 + 18 * i] = slot
    for i in range(len(slots), 4):  # unused descriptors
        b[54 + 18 * i + 3] = 0x10
    b[127] = (-sum(b[:127])) & 0xFF
    return bytes(b)


PRESETS = {
    # A laptop panel: no name, no serial, as the real ones are.
    "builtin": lambda: edid("IVO", 0x8C44, 0, None, [(1920, 1200, 60)], (301, 188)),
    # The awkward one: a 21:9 panel that prefers 4K, which squashes it.
    "lg-ultrawide": lambda: edid(
        "GSM", 0x772E, 0x5F79F, "LG HDR WQHD",
        [(3840, 2160, 60), (3440, 1440, 60)], (800, 340), serial_text="812NTAB"),
    # Two Dells of one model, as at a hot desk: only the serial differs.
    "dell-home": lambda: edid(
        "DEL", 0xA0D3, 1, "DELL U2720Q", [(2560, 1440, 60)], (597, 336), serial_text="HOME1"),
    "dell-desk": lambda: edid(
        "DEL", 0xA0D3, 2, "DELL U2720Q", [(2560, 1440, 60)], (597, 336), serial_text="DESK9"),
    "dell-desk2": lambda: edid(
        "DEL", 0xA0D3, 3, "DELL U2720Q", [(2560, 1440, 60)], (597, 336), serial_text="DESK10"),
    # A plain 1080p monitor.
    "full-hd": lambda: edid(
        "ACR", 0x0A55, 7, "ACER KG241", [(1920, 1080, 60)], (527, 296), serial_text="KG7"),
}

if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] in PRESETS:
        sys.stdout.buffer.write(PRESETS[sys.argv[1]]())
    elif sys.argv[1:] == ["--list"]:
        print("\n".join(PRESETS))
    else:
        sys.exit(__doc__)

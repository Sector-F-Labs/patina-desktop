# 0148. Teleprinter is a member of this workspace

Date: 2026-11-19

## Status

Accepted. Changes how [0099](0099-one-deb-of-the-desktop-and-its-apps.md) gets Teleprinter.

## Context

Teleprinter, the terminal, was its own repository. The desktop installed it from the checkout
beside this one (`make deploy`, the Linux VM) or from GitHub at a pinned commit (`bundle.toml`,
`make install-full`, the .deb). A desktop has a terminal, and the two change together: every
Patina change Teleprinter needed meant a push, a `cargo update` and a new pin.

## Decision

- Teleprinter lives in `teleprinter/` here, with its history, as one more workspace member. It
  takes `patina-ui` from the workspace, so there is one Patina pin and one `Cargo.lock`.
- It is still its own app: `teleprinter/Makefile` builds with `-p teleprinter`, installs on its
  own, and `make dmg` builds the macOS app. Being in the repository doesn't tie it to this desktop;
  it runs on macOS and other Linux desktops as before.
- `make stage` installs it like Files, Reader and Notebook (binary, entry, icons), so `make
  install`, `make deploy`, `make install-full` and the .deb all carry it. `bundle.toml` no longer
  pins it; a `[[member]]` entry only lets the .deb's description list it.
- Calendar joined the same way (`calendar/`), and `stage` installs it too, since the clock menu
  runs `calendar --agenda`.
- The Linux VM no longer shares a second checkout: Teleprinter is at `/patina/target/release`
  with the rest.

## Consequences

- Changing Teleprinter and the desktop together is one commit.
- Teleprinter's own repository stops being the source. It should be archived with a pointer here.
- On macOS build only `-p teleprinter`: the workspace holds Linux-only crates.
- Calendar's binary is `/usr/bin/calendar`, the path Ubuntu's `calendar` package (the BSD reminder
  tool, not installed by default) uses, so the two clash in a .deb as ImageMagick's `montage` does
  for Montage's (ADR-0099). Rename the binary, or add the package to the bundle's conflicts, if
  that bites.

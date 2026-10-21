# 0078. The desktop is installed for every user, in /usr/local, as a BSD port is

Date: 2026-10-21

## Status

Accepted. The user asked that pulling the code and running `make install` give a desktop that
works for every user of the machine, BSD-inspired, with doas, which this machine and nifl have.

## Context

`make install` put everything in the user's home (`PREFIX=~/.local`), and `make install-session`
added the two files only a system folder can hold, the login session's entry and the lock screen's
PAM service, with sudo. That suited development: a change was installed in seconds without root.
As a desktop for real use it falls short:

- Binaries the user can write can be replaced by anything running as the user: a lock screen
  that saves passwords, a compositor that logs keys.
- The session entry pointed into one user's home, so only that user had the desktop, and a home
  that's encrypted or mounted at login may not be there when the login screen starts it.
- Login sessions, PAM, and later portals, D-Bus services and systemd units are read from system
  folders; a per-user install can only put half of it in place.

The project leans toward BSD ways (Patina OS's FreeBSD experiment; doas on both machines). There,
software outside the base system lives in `/usr/local`, built as yourself and installed as root.

Considered and ruled out:

- **Keeping the per-user install for development beside a system one:** two layouts mean
  testing something other than what's shipped, and `~/.local/bin` comes first on `PATH`, so stale
  copies shadow the installed ones.
- **Building as root** (`doas make install` building as well): `target/` ends up root's.

## Decision

- **`PREFIX` defaults to `/usr/local`.** Binaries, man pages, Settings' entry and icon, and the
  session's default apps go under it.
- **`make install` builds as you, then installs as root:** run as yourself it builds (the
  launcher too, from its repository into `target/launcher`), then runs itself again through doas,
  or sudo where there's no doas. Run as root it only installs, and stops if nothing is built.
- **It installs the login session and PAM service too;** `install-session` is gone. They go
  where the system reads them: on Linux `/usr/share/wayland-sessions/patina.desktop`, as not every
  login manager reads `/usr/local/share`, and `/etc/pam.d/patina-lock`; on FreeBSD under `PREFIX`
  (`share/wayland-sessions`, `etc/pam.d`), as ports put them. `make uninstall` removes it all.
- **`make deploy <host>` installs the same on the host:** staged for `/usr/local` with
  `DESTDIR`, then `rsync --rsync-path="doas rsync"`, owned by root there, with Teleprinter
  installed the same way. Development uses the one layout that ships.

## Consequences

- Any account on the machine can choose Patina at the login screen; the binaries can only be
  changed as root.
- Every install asks for doas (or sudo), and `make deploy` needs doas on the host, without a
  password for it to run unattended.
- Machines with the old per-user install keep copies in `~/.local` that shadow the new ones in a
  terminal (the session itself finds its own, as the compositor puts its folder first on its
  clients' `PATH`); they're removed by hand once, as is `~/.local`'s session entry's target.
- `make deploy` stages with this machine's layout (`uname`), so a FreeBSD host from a Linux one
  would get Linux's session and PAM folders.

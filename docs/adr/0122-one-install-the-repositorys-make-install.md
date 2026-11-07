# 0122. The repository's make install is the only install

Date: 2026-11-07

## Status

Accepted

Partly supersedes [0106](0106-files-lives-in-this-repository.md), which kept Files' own `make
install`, and [Notebook's 0008](../../notebook/docs/adr/0008-install-per-user-replacing-the-gtk-binary.md).

## Context

Files, Reader and Notebook were repositories of their own, each with a `make install` that put them
under `~/.local`: the binary, a launcher and an icon, and for Files a D-Bus service file for its
file-chooser portal backend. They are crates of this workspace now (ADR-0106, ADR-0107), and the
root `make install` installs them system-wide under `/usr/local`, as it does everything else. Both
installs kept working, and nothing said which won.

The per-user one wins, and silently. A D-Bus service file in `~/.local/share/dbus-1/services` is
found before the system's, and `~/.local/bin` can come before `/usr/local/bin` on the PATH. On
2 November (huginn) and 4 November (nifl) Files' per-user install wrote a service file pointing at
`~/.local/bin/files`. That binary was later gone, on both machines, and the file stayed: every
request for a file chooser made D-Bus try to run a program that wasn't there ("Failed to execute
program … No such file or directory"), and Draw's save and Programmer's open-folder did nothing. The
correct service file in `/usr/local` was never reached, and `make install` could not fix it because
it never writes to a user's home.

Even without a vanished binary the shape is wrong: a per-user copy shadows the system one with
whatever was built when it was last installed, so a stale build can be what runs, with no sign why.

## Decision

- **There is one install: the repository's root `make install`** (and `make deploy` and `make deb`,
  which stage the same files). Files, Reader and Notebook lose their own `install` and `uninstall`,
  and Files its `install-portal` and `uninstall-portal`. What is left in their Makefiles builds,
  runs, tests and makes artefacts (`make app`, `make default`).
- **Launcher keeps its per-user install for now.** Its README documents it as the way to run it on
  its own, on another compositor, which the root install is too large for. It writes no service
  file. Whether that goes too is for a later decision.

## Consequences

- Nothing in this repository writes a per-user copy that shadows an installed one, so the failure
  above can't be made again from here.
- A machine that has one from before is not cleaned: the root `make uninstall` doesn't touch a
  home directory. What to look for, per user: `~/.local/bin/{files,reader,notebook}`, their
  `~/.local/share/applications/com.sectorflabs.{Files,Reader,Notebook}.desktop` and icons, and
  `~/.local/share/dbus-1/services/org.freedesktop.impl.portal.desktop.patina.service`. The service
  file is the one that breaks things; the others only shadow.
- Notebook on GNOME, with no Patina desktop, now needs the root install for its launcher and icon.
- A guard in the root `make install` that warns about a per-user copy shadowing the installed one
  would catch the leftovers; it is not part of this decision.

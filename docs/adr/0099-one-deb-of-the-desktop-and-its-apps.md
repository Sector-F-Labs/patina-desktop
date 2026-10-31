# 0099. The desktop ships as one .deb, with the apps it was tested with, pinned in bundle.toml

Date: 2026-10-30

## Status

Accepted

## Context

The desktop reached machines in three ways: `make install` from a checkout, `make deploy <host>`
(staged here, rsynced as root, Teleprinter alongside from whatever `../teleprinter` held), and
Patina OS's `linux/desktop/build.sh`, with a binary list of its own that had fallen behind the
Makefile's (seven pieces of seventeen). None of them said which commits of the apps went out
together, apt knew nothing of the files, and removing them was a hand-kept `uninstall` target.

Patina OS is Ubuntu, and its ADR-0007 already planned the desktop as `.deb`s from an apt repository
of our own. The choice was how many. Splitting it (a package per app, and a `patina` metapackage)
is how GNOME and KDE ship, and lets someone on another desktop install only Teleprinter. But the
point here is an opinionated desktop that is known to work as a whole: the apps are tested with
it, the bar, launcher and Process Manager start them, and each app can still be installed from its
own repository for anyone who wants it alone.

## Decision

- The desktop is one package, `patina-desktop`, holding this repository (as `make stage` lays it
  out, under `/usr`) and the apps `packaging/bundle.toml` lists, each at a pinned commit:
  patina-launcher, Teleprinter, Files, Reader, Notebook, Draw, Montage and UnAmp.
- `make deb` (`packaging/deb/build.py`) builds it from this repository's last commit, and each app
  from a clean `git archive` of its commit, on the machine it runs on, into `target/deb/`.
  `WORKTREE=1` builds uncommitted changes, and the version says `+dirty`. Work is kept in
  `~/.cache/patina-desktop/deb` so rebuilds are incremental.
- Changing what ships is a commit to `bundle.toml`: a new pin, an app added or left out.
- The version is the compositor crate's, then the commit's time and hash
  (`0.1.0+202610291530.g5a48125`), so apt orders builds by when they were made.
  `/usr/share/patina-desktop/versions` lists every part's commit.
- Depends are what `dpkg-shlibdeps` finds the binaries link, and the programs the desktop runs
  (`[depends] required`); `recommended` are Recommends. An app that ships a `.deb` of its own
  (UnAmp) is in `replaces`, so apt swaps it for the bundle.
- What's under `/etc` (the lock screen's PAM service, the portal config) is a conffile.

## Consequences

- One file to install, test and roll back; apt removes it cleanly, and Patina OS and `make deploy`
  can install the same file instead of laying out their own.
- The package is only as tested as the run that tested it: `make test-vm` should install this
  `.deb` in the Linux VM rather than share in `target/release`, so what passes is what ships.
- It builds on the machine it runs on: an Ubuntu 26.04 build is for 26.04. Other releases, other
  architectures and FreeBSD need builds of their own (FreeBSD a port).
- Apps' binaries go in `/usr/bin` under plain names (`files`, `draw`, `montage`, `reader`). A
  source install in `~/.local/bin` still comes first on PATH. ImageMagick's `montage` is an
  alternatives link at the same path, so the bundle and ImageMagick clash until Montage's binary is
  renamed or moved.
- Every app is fetched from its public repository under Sector-F-Labs (UnAmp from divanvisagie's),
  so anyone can build the bundle.
- `make install-full` installs the same set without a `.deb`, for FreeBSD and other distros:
  `build.py --stage` lays the tree out under any `--prefix` and skips the dpkg steps. The portal
  files `bundle.toml` places under `/usr/share` stay there whatever the prefix. It leaves the list
  of what it installed in `share/patina-desktop/files`, which `make uninstall-full` removes.
- Still to come: an apt repository on heimdallr, Patina OS and `make deploy` installing the `.deb`,
  and `make test-vm` installing it.

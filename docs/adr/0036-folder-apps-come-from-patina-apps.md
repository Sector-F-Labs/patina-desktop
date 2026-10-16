# 0036. Folder apps are read by patina-apps, a crate shared with Files

Date: 2026-10-16

## Status

Accepted; moved with the launcher to its own repository by
[ADR-0037](0037-patina-shell-and-the-launcher-are-public.md)

## Context

ADR-0035 put the reading of `~/Apps/*.app/app.toml` in the launcher (`launcher/src/folders.rs`).
Files, a Patina file manager in its own repository, now needs the same rules: it shows a `.app`
anywhere as one item with its icon and runs it when it's opened, as the Finder does. Nautilus
can't: it has no idea of a folder that acts as one file, and a thumbnailer could only give it
the icon, from a binary installed under `/usr` for its sandbox. Two copies of format 1's rules
would drift apart.

## Decision

- Format 1's rules, running an app and drawing its icon move into **patina-apps**, a crate in its
  own private repository (`remotes.divanv.com:~/Remotes/src/com/sectorflabs/patina-apps`). The
  launcher and Files depend on it by git.
- The launcher keeps only the step from a `patina_apps::App` to its own list entry
  (`folders.rs`), and draws icons with `patina_apps::icon::render`.
- `.cargo/config.toml` sets `net.git-fetch-with-cli`, so cargo fetches it with the user's ssh
  setup. Builds without access to remotes (Patina OS's builders) point cargo at a copy with a
  `[patch]`.

## Consequences

- A change to the format is made once, and both the launcher and Files follow it after a
  `cargo update -p patina-apps`.
- Building patina-desktop needs ssh access to remotes, or a staged copy of patina-apps.
- A third thing that shows apps (a thumbnailer, a dock) reuses the crate rather than the
  launcher's code.

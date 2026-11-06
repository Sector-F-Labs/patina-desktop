# 0003. Folder apps are read by patina-apps, a crate shared with Files

Date: 2026-10-16

## Status

Accepted

## Context

ADR-0002 put the reading of `~/Apps/*.app/app.toml` in the launcher (`launcher/src/folders.rs`).
Files, a Patina file manager in its own repository, now needs the same rules: it shows a `.app`
anywhere as one item with its icon and runs it when it's opened, as the Finder does. Nautilus
can't: it has no idea of a folder that acts as one file, and a thumbnailer could only give it
the icon, from a binary installed under `/usr` for its sandbox. Two copies of format 1's rules
would drift apart.

## Decision

- Format 1's rules, running an app and drawing its icon move into **patina-apps**, a crate in its
  own repository, [Sector-F-Labs/patina-apps](https://github.com/Sector-F-Labs/patina-apps)
  (private at first, public since 2026-10-16). The launcher and Files depend on it by git.
- The launcher keeps only the step from a `patina_apps::App` to its own list entry
  (`folders.rs`), and draws icons with `patina_apps::icon::render`.

## Consequences

- A change to the format is made once, and both the launcher and Files follow it after a
  `cargo update -p patina-apps`.
- A third thing that shows apps (a thumbnailer, a dock) reuses the crate rather than the
  launcher's code.

> Recorded first as patina-desktop ADR-0036, when the launcher lived there.

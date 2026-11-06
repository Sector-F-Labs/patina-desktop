# 0001. Rewrite Notebook as an egui app on Patina, in a new repo

Date: 2026-10-11

## Status

Accepted

## Context

Notebook was built with GTK4, libadwaita and Relm4 (the GTK repo's ADR-0001), in
`com/divanv/notebook`. Since then Photograph and UnAmp, both egui apps, have settled on a shared
style. That style is now a published library, [Patina](https://github.com/Sector-F-Labs/patina): one
window frame, skins any app can read, and a design language that recommends ADRs. Notebook looked
like GNOME rather than like its siblings, and couldn't share their skins.

The user asked for Notebook to be rewritten on Patina, in a new repo under Sector F Labs. The GTK
version keeps its own repo, uncommitted work included, untouched.

## Decision

- **Toolkit:** egui/eframe 0.35 with `patina-ui` 0.1, at `com/sectorflabs/notebook`.
- **Patina parts:** its own window frame (`frame::TitleBar`), with the toolbar in the header bar.
  The desktop's light/dark setting (`appearance`) and skins (`skin`, including Dark Orange) come
  from Patina too.
- **Carried over unchanged:** the markdown model (`markdown.rs`, which had no GTK in it) and its
  tests.
- **Carried-over decisions** are restated here as ADRs 0004, 0005 and 0008, so this repo stands on
  its own.
- **Rewritten:** the rest, mainly the rich-text editor ([ADR-0002](0002-own-wysiwyg-editor.md)).
- **Not carried over (yet):**
  - Single-instance launching through GApplication (part of the GTK repo's ADR-0008).
  - The installed launcher.
  - Folder collapse state across runs.

## Consequences

- Notebook looks and behaves like Photograph and UnAmp, and takes the same skins.
- No more GTK, libadwaita or GtkSourceView: building needs only Rust and the usual egui system
  libraries.
- GTK gave Notebook text editing, IME, accessibility and spell checking. Here the editor is ours,
  so all of that is ours to build. Accessibility and spell checking aren't done.
- The binary is also called `notebook`. Installing both versions at once would clash, so there's
  no install target until the GTK one retires.
- The config folder `~/.config/notebook/` is shared. This version writes `config.toml`, and reads
  the GTK version's `last-folder` once, when `config.toml` has no folder yet.

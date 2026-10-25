# 0001. Build a text editor as a native egui app on Patina

Date: 2026-10-25

## Status

Accepted. The user asked for "a utility app that will do what GNOME's text editor does", in its
own repository (`~/Src/com/sectorflabs/editor`), with GNOME Text Editor's core editing, find and
replace, syntax highlighting, and session restore with autosaved drafts in the first version.

## Context

The Patina desktop and its apps open text from several places: Files' Edit on a script (Files'
ADR-0011) looks for the desktop's app for `text/plain`, and Settings' Defaults picks an app per
role. Until now that has been GNOME Text Editor or `$EDITOR` in a terminal: a GTK app in a desktop
whose other apps are all Patina, with its own look, header bar and hamburger menu.

Other options: keep using GNOME Text Editor (works, but doesn't follow skins or Patina's frame and
menus); or make Notebook edit plain text (Notebook is a markdown notebook over a folder of notes,
not a general file editor, and its editor is rich text).

## Decision

We will build Text Editor with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`, from its `develop` branch), as the other Sector F apps are:

- Patina's own window frame: File, Edit, View and Skin menus on the left, the tabs in the title
  bar, and Save on the right as the action that makes the app's output (Patina's ADR-0006). The
  Patina Frame setting is the escape hatch.
- The desktop's light/dark setting followed through `patina::appearance`, and skins through
  `patina::skin`, loaded from `~/.config/text-editor/skins`.
- Patina's status bar (Patina's ADR-0016), holding the cursor's line and column and, as GNOME's
  does, the language, indentation and line ends, each a menu.
- It does what GNOME Text Editor does: tabs, new/open/save, undo, find and replace, go to line,
  syntax highlighting, line numbers, wrapping, zoom, and unsaved work kept across restarts.

## Consequences

- The desktop has a text editor that looks and behaves like the rest of the Patina family and
  takes the same skins.
- Its `.desktop` file claims `text/plain` and common source types, so it can be the default for
  text.
- It moves to a new egui version only together with Patina.
- It has to earn parity with a mature GTK editor feature by feature; what GtkSourceView gives
  GNOME's for free (encodings, spell checking, printing, very large files) is work here, and is
  left out of the first version.

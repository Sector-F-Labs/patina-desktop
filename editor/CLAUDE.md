# Text Editor

A Patina text editor doing what GNOME Text Editor does (ADR-0001): tabs, find and replace,
highlighting, and the session with its unsaved drafts kept across restarts. The binary is
`text-editor`, not `editor` (ADR-0008). Read the ADRs in `docs/adr/` before changing how it edits,
saves or keeps the session; use the `adr` skill to add one.

## Commands

- `make check` (tests + clippy -D warnings) before committing.
- `make run` opens this README with a scratch config and data folder.
- Never run test instances against the user's config or session: set `XDG_CONFIG_HOME` and
  `XDG_DATA_HOME` to scratch folders (`SCRATCH_ENV` in the Makefile). The data folder also names
  the single-instance socket, so a scratch run never hands its files to the user's window.
- `TEXT_EDITOR_SCREENSHOT=file.png text-editor FILE` saves the window and quits;
  `TEXT_EDITOR_SKIN`, `TEXT_EDITOR_FIND` and `TEXT_EDITOR_REPLACE` set it up. It needs no input,
  so it runs on a headless sway (`WLR_BACKENDS=headless sway`, then `WAYLAND_DISPLAY` set to its
  socket), off the user's screen.

## Layout

- `src/app.rs`: the window: menus, tabs, find and go-to bars, status bar, dialogs, threads'
  results, drafts and session.
- `src/view.rs`: the editing area, egui's `TextEdit` (ADR-0002) with line numbers, the cursor's
  line, indent keys, undo, the context menu; tested with real pointer and key events.
- `src/highlight.rs`: syntect a line at a time, kept between edits (ADR-0003).
- `src/find.rs`: searches as regexes, replacing, marking matches in a layout job.
- `src/indent.rs`: Tab, Shift+Tab and Enter; guessing a file's indentation. Positions in chars.
- `src/document.rs`: a tab's text, file, saved fingerprint and state.
- `src/files.rs`: reading and saving (ADR-0007).
- `src/session.rs`: the session and drafts (ADR-0004).
- `src/instance.rs`: one window, over a socket (ADR-0005).
- `src/watch.rs`: files changed on disk (ADR-0006).
- `src/config.rs`: `~/.config/text-editor/config.toml`.
- Patina comes from its `develop` branch: change it there (`~/Src/com/sectorflabs/patina`),
  push, then `cargo update -p patina-ui` here. Never publish Patina without asking.

## Rules

- Paint colours only from `ui.visuals()`, so skins reach them (syntax colours are syntect's,
  ADR-0003).
- Files change on disk only when the user saves. I/O stays off the window's thread.
- In egui, `consume_key` with no modifiers also matches the key with Shift: take Shift's first.

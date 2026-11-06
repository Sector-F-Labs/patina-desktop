# Reader

A read-only Patina reader for a project's markdown, man pages and Rust, to become Patina's help
viewer. Read the ADRs in `docs/adr/` before changing what it shows or how; use the `adr` skill to
add one.

## Commands

- `make check` (tests + clippy -D warnings) before committing.
- `make run` reads this repository; `make install` installs per user.
- Never run test instances against the user's config: set `XDG_CONFIG_HOME` to a scratch folder
  (`make screenshot` does). `READER_SCREENSHOT=file.png reader PATH` saves the window and quits;
  on a headless sway it needs no input.

## Layout

- `src/app.rs`: the window: menus, the tree with its filter, the file on screen, history, links.
- `src/tree.rs`: scanning a project for what Reader shows, ordering, filtering.
- `src/links.rs`: where a clicked link goes.
- `src/man.rs`: man pages through mandoc or man-db, overstrikes parsed into bold and underline.
- `src/config.rs`: `~/.config/reader/config.toml`.
- Markdown and highlighted source come from Patina's `markdown` feature (`patina::markdown`,
  Patina's ADR-0015): `Editor` read-only, `embeds::code_file`. Change them in Patina
  (`~/Src/com/sectorflabs/patina`, push `develop`), then `cargo update -p patina-ui` here. Never
  publish Patina without asking.

## Rules

- Paint colours only from `ui.visuals()`, so skins reach them.
- Reader never writes to what it reads.

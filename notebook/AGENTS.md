# Notebook

A notes app in Rust on egui and [Patina](https://github.com/Sector-F-Labs/patina): open a folder of
markdown files and edit them as rich text, Apple Notes style, with the markdown generated on save.
A rewrite of the GTK Notebook in `com/divanv/notebook-old-gtk`.

Significant, hard-to-reverse architecture decisions are recorded as ADRs in `docs/adr/` (see
`docs/adr/README.md` for the convention, and the `adr` skill for creating one). Read the relevant
ADRs before changing how notes are edited, saved or stored.

## Commands

```bash
make test          # unit tests, including editor tests driven through egui events
make check         # tests + clippy -D warnings
make run           # run on a copy of examples/notes (target/demo-notes)
make screenshot    # docs/screenshot-*.png, in a throwaway config dir
make icons         # assets/notebook-icon-128.png from packaging/linux/notebook.svg (via resvg)
make install       # per-user install to ~/.local (ADR-0008); test with PREFIX=<scratch dir>
```

The user may be running Notebook (this one or the GTK one) while you work. Never touch their
config: run test instances with `XDG_CONFIG_HOME` and `XDG_DATA_HOME` pointing at a scratch
directory (as `make screenshot` does), and never against their own notes folder.

On Wayland, `with_icon` does nothing. GNOME takes the window icon from the installed
`com.sectorflabs.Notebook.desktop`, matched by app id. So "no icon when I cargo run" means the
launcher isn't installed, or the window was opened before it was.

## Layout

- The markdown model (`Line`/`Block`/`Span`), paragraphs, the WYSIWYG widget, embeds and fonts are
  in Patina, `patina::markdown` (`model`, `doc`, `editor`, `embeds`, `fonts`; ADR-0013, Patina's
  ADR-0015), from its develop branch. Change them there (`~/Src/com/sectorflabs/patina`, push
  `develop`), then `cargo update -p patina-ui` here; `main.rs` brings them in as `editor` and
  `markdown`. Never publish Patina without asking.
- `src/notes.rs`: workspace scanning and file helpers, including image paths.
- `src/app.rs`: window, Patina header bar, tabs and toolbar, sidebar, dialogs, autosave, skins.
  `Tab` holds one workspace (folder, tree, open note, editor); `App` holds the tabs and the
  window-wide state (ADR-0011).
- `src/config.rs`: `~/.config/notebook/config.toml`.
- Skins: the Notebook skin lives in Patina (`skins/notebook.toml` there, ADR-0010); this app only
  makes it the starting one (`HOME_SKIN` in `app.rs`). Patina's `TitleBar` colours the bar's
  controls when a skin colours the bar.

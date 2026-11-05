# Ledger

A Patina (egui) bookkeeping app: tabbed spreadsheets, each ledger a SQLite database. Built on
Patina's `develop` branch (see `../AGENTS.md`).

Significant decisions are ADRs in `docs/adr/`; read them before changing the file format, saving
or the formula language. Use the `adr` skill to add one. Several are still `Proposed`: they were
the assistant's choices and wait for the user's sign-off.

## Commands

```bash
make check         # tests + clippy -D warnings
make dev           # demo books in a scratch config (LEDGER_DEMO=path makes them)
make screenshot    # docs/screenshot-*.png, from the demo books
make install       # per-user install; test with PREFIX=<scratch dir>
```

Never run test instances against the user's config or files. The Makefile's `SCRATCH_ENV` points
XDG config and data at `target/scratch`. `LEDGER_SCREENSHOT=path.png` saves the window and quits,
`LEDGER_SKIN=name` sets that run's skin without saving it.

On Wayland the window icon comes from the installed launcher (`com.sectorflabs.Ledger.desktop`),
matched by app id, so `cargo run` shows it only after `make install`.

## Layout

- `src/formula.rs`: cell names, input parsing, formula parser and evaluator (cycle-safe).
- `src/sheet.rs`: one sheet's inputs and computed values.
- `src/store.rs`: the SQLite file: schema, open/create, and the writer thread.
- `src/grid.rs`: the grid widget: painting visible cells only, selection, keyboard, editing,
  clipboard. Tested with real pointer and key events through egui (`Harness`).
- `src/app.rs`: window, title bar with menus and the sheet tabs, formula bar, status bar, dialogs,
  file dialogs (in a thread), screenshot mode.
- `src/config.rs`: `~/.config/ledger/config.toml` (skin, system title bar, recent files).
- `src/demo.rs`: the sample books for `make dev` and screenshots.

## Gotchas

- egui takes focus from a TextEdit created on the frame of the click that opened it (it counts
  as a click elsewhere): the grid editor asks for focus again after it draws. Don't "simplify"
  that away; `double_click_edits_what_is_there...` fails without it.
- When typing starts an edit, the grid removes the `Event::Text` it used, or the editor would
  type it a second time.
- Formulas aren't adjusted when pasted, and don't cross sheets (ADR-0005).

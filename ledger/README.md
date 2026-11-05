# Ledger

![Ledger's icon](docs/icon.png)

Ledger is a desktop app for bookkeeping in tabbed spreadsheets. A ledger is one SQLite database
file: every sheet is a tab, and the file can be read with any SQLite tool. It's built with
[egui](https://github.com/emilk/egui) on [Patina](https://github.com/Sector-F-Labs/patina), so it
shares a window frame and skins with Notebook, Calendar and Photograph.

![Ledger with the demo books](docs/screenshot-default.png)

## Features

- **Tabbed sheets:** the tabs sit in the title bar. `+` or Ctrl+T adds a sheet, a tab's right-click
  menu renames it, and tabs can be dragged into another order.
- **A real grid:** click or use the arrows to move, Shift to select a block, Ctrl+arrows to jump
  to the edge of the data. Type to replace a cell, double-click or F2 to edit what's in it, Enter
  and Tab to commit and move on, Esc to cancel. Cut, copy and paste work as tab-separated text, so
  they work with other spreadsheets.
- **Formulas:** `=D2+E2`, `=SUM(D2:D9)`, `=ROUND(D2*0.25, 2)`. Operators `+ - * / ^ &`, and
  `SUM`, `AVERAGE`, `MIN`, `MAX`, `COUNT`, `ABS`, `ROUND`. Errors show in the cell (`#DIV/0`,
  `#CYCLE`, ...) and nothing else stops. References stay within one sheet.
- **Shell formulas (prototype):** `=SH("df -h", :F20)` runs a command and fills a block of cells
  with its columns, starting at the formula's cell. Only in a ledger you've trusted (File → Trust
  This Ledger); run them with Ctrl+R. See [Shell formulas](#shell-formulas) below.
- **A status bar** with the selected range, and its sum, average and count.
- **No Save:** every edit is written to the file as you make it, off the UI thread.
- **Skins:** Patina's skins, and any in `~/.config/ledger/skins/`, are in the Skins menu.

![Dark Orange](docs/screenshot-dark-orange.png)

## Shell formulas

A formula can run a program and spill its output into the sheet. The formula lives in the
top-left cell of the block:

```
=SH("df -h")                      the block is as big as the output
=SH("df -h", :F20)                columns up to F, rows up to 20
=SH("df -h", :F)                  columns up to F, as many rows as there are
=SH("ps aux", :K, skip=1)         leave out the header line
=SH("cut -d, -f1,3 data.csv", sep=",")
=SH("grep -c \"$1\" log.txt", arg=A1)
```

Lines become rows and each line is split on whitespace (or `sep=`). If a line has more fields than
the block has columns, the last column keeps the rest (`rest=keep`, the default) or drops it
(`rest=drop`). Output that doesn't fit is marked: the block's bottom or right edge turns heavy
and orange with a **…** on it, and hovering says how much was cut. Cell values reach the command
as arguments (`arg=A1` is `$1`), never as part of its text. The command has 10 seconds
(`timeout=`).

Commands run only in a ledger you've trusted, which is a setting in your own config: a file
can't trust itself, so opening one never runs anything. Untrusted, the cells show their last
results. Other cells can read a block (`=SUM(C3:C9)`). `#SPILL` means the block would cover
something typed. See `docs/adr/0006` for what's missing.

![Shell output cut off at its block's edges](docs/screenshot-shell.png)

## The file

A ledger is a plain SQLite file (`.sqlite3` by default) with three tables:

```sql
sheets        (id, name, position)
cells         (sheet_id, row, col, input)   -- input is exactly what was typed; "=..." is a formula
shell_results (sheet_id, row, col, formula, status, stdout, stderr, ran_at)  -- what =SH() printed
```

`PRAGMA application_id` is `0x4C444752` ("LDGR") so Ledger can tell its files from other
databases, and `user_version` is the format version. See `docs/adr/` for why.

```bash
sqlite3 books.sqlite3 "select s.name, c.row+1, c.col, c.input from cells c join sheets s on s.id = c.sheet_id"
```

Ledger opens the file you give it, or the last one you used:

```bash
ledger books.sqlite3
```

## Shortcuts

| Keys | Does |
|---|---|
| Ctrl+N / Ctrl+O | New ledger / open one |
| Ctrl+T / Ctrl+W | New sheet / delete the shown one |
| Ctrl+R | Run the shell cells (trusted ledgers) |
| Ctrl+PgUp / Ctrl+PgDn, Alt+1…9 | Another sheet |
| Arrows, Tab, Enter | Move (Shift for the opposite direction or to select) |
| Ctrl+arrows, Ctrl+Home | Jump to the edge of the data / to A1 |
| F2 | Edit the cell |
| Delete | Clear the selection |
| Ctrl+X / C / V, Ctrl+A | Cut, copy, paste, select the data |
| Ctrl+Q | Quit |

## Build

```bash
make            # list the targets
make run        # run it
make dev        # run it on demo books, in a scratch config
make check      # tests and clippy -D warnings
make screenshot # regenerate docs/screenshot-*.png
make install    # per-user install, with the launcher and icon (PREFIX=~/.local)
```

Ledger depends on Patina's `develop` branch on git, as every Sector F app does.

## Licence

BSD 3-Clause. See [LICENSE](LICENSE).

# Reader

A [Patina](https://github.com/Sector-F-Labs/patina) reader for a project's documentation and code.
Point it at a folder, such as the Patina desktop's, and browse its markdown, shown exactly as
Notebook shows it, its man pages, and its Rust source, highlighted. It only reads: nothing can be
changed. It's meant to become Patina's help viewer, opened when you choose Help in a Patina app.

![The Patina desktop's README in Reader](docs/screenshot-markdown.png)

- **Markdown** (`.md`) through Patina's markdown widget in read-only mode: headings, lists, tasks,
  quotes, tables, highlighted code blocks and images, with no markdown syntax on screen. Text can
  be selected and copied. A link to another file in the project opens it here, a folder opens
  its README, a web link the browser, anything else its usual app.
- **Rust** (`.rs`) highlighted as code blocks are, with line numbers, on the skin's code
  background.
- **Man pages** (`.1` to `.9`, gzipped or not) formatted by the system's man tools (mandoc, else
  man-db), with man's bold and underline. Go → Man Page opens any installed one by name (`ls`,
  `printf(3)`).
- **System mode** (`reader --man [PAGE]`, or Go → System Manual): the tree is every man page on
  the system, by section (1 User Commands, 8 System Administration, …), and nothing else; the
  filter finds a page among them. It opens PAGE (`ls`, `printf(3)`), else man(1).
- **Tabs**, as in Notebook: each its own project, or the manual, with its own tree and history
  (Ctrl+T, Ctrl+W, Ctrl+PageUp/PageDown, Alt+1 to 9). The tabs open last time open again.
- **Search** in the title bar (Ctrl+F) through the text on screen: every match marked, the
  current one selected; Enter or F3 for the next, Shift+Enter or Shift+F3 the previous.
- **Links in man pages**: `printf(3)` opens that page, web addresses the browser, and paths that
  exist (`/usr/share/doc/…/examples`) open here if Reader shows them, else in Files.
- The project as a tree, with only what Reader shows in it (no `target/`, `.git` or hidden
  folders), READMEs first; a filter to find a file by name (Ctrl+P, Enter opens the first).
- Back and forward (Alt+Left and Alt+Right, or the mouse's side buttons), Reload (Ctrl+R), Open
  Project (Ctrl+O, through the desktop's file chooser), and the last project and file read open
  again next time.
- Patina skins, starting in Notebook's.

```sh
reader ~/Src/com/sectorflabs/patina-desktop        # a project
reader docs/adr/0001-build-on-patina.md            # a file, in its repository
reader --file docs/adr/0001-build-on-patina.md     # the same, named with --file
reader ~/Src/project --file ~/Src/project/docs/x.md   # a file in a given project
reader --single --file help.md                     # the file alone, no tree (-s)
reader --man printf(3)                             # every man page on the system, at printf(3)
```

Single-file mode (`-s`, `--single`) shows the file alone, in a smaller window, as help will:
links and Back still work, File → Project Tree brings the tree back, and the last project read
isn't changed. Given a folder, it opens its README.

| Rust in Steam Classic | A man page in Dark Orange |
|---|---|
| ![](docs/screenshot-rust.png) | ![](docs/screenshot-man.png) |

## Install

```sh
make install      # ~/.local/bin/reader, its launcher and icon
```

It depends on Patina's develop branch from GitHub, for its markdown widget.

## Develop

```sh
make check        # tests and clippy
make run          # read this repository
make screenshot   # docs/screenshot-markdown.png, in a scratch config
```

Decisions are in [docs/adr](docs/adr/README.md).

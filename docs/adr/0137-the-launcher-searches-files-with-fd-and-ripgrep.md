# 0137. The launcher searches files with fd and ripgrep, in folders Settings' Search page chooses

Date: 2026-11-13

## Status

Accepted. The user asked for it, to make the launcher more like Spotlight.

## Context

The launcher lists installed apps and nothing else. Spotlight also finds files: by name, and by
what's in them. A first version should be small: a few folders in the home folder, not an index of
the whole disk.

Ways to search:

- **An index** (a daemon watching the disk, as Tracker or Baloo do). Fast to ask, but a process
  that's always running and a database to keep right; far more than a first version needs.
  Rejected.
- **Walking and reading the files in the launcher's own code**, with the `ignore` and `grep-*`
  crates fd and ripgrep are made of. No program to install, but it's the same work again and
  fd's and ripgrep's defaults (ignore rules, binary and hidden files, smart case) are the ones
  users already know.
- **Running fd and ripgrep** on the folders when the user types. Both are fast on a home folder's
  media, documents and notes; nothing runs while the launcher is closed. Chosen: the user named
  them.

fd is called `fdfind` on Debian and Ubuntu.

## Decision

- **A new crate, `patina-search`** (`search/`), holds what the launcher and Settings share: the
  config and the runs of fd and ripgrep. The launcher (which runs alone on other desktops) and
  Settings don't take on `patina-system` for it.
- **The config** is `~/.config/patina-search/config.toml`: `enabled`, and a `[[folder]]` for each
  folder with its `path` (`~` for home), `enabled` and `style`: `fd` (names), `rg` (contents) or
  `both`. With no file: on, with Pictures, Music and Videos by name and Documents and Notes by name
  and contents (the XDG folders, so a localised home is found).
- **Settings' Search page** turns it on or off, turns folders on or off, sets each folder's style,
  adds and removes folders, resets to the defaults, and says when fd or ripgrep isn't installed.
  It writes the file at once, as the other pages do; the launcher reads it each time it opens.
- **The launcher** lists the files under the apps, with a small heading for each when there are
  both, and Enter or a click opens one with its app. A file hit shows its name and folder; a hit in
  the contents shows the line it was found on.
- **How it searches:** nothing until two characters are typed, and then 150 ms after the typing
  pauses, on a thread, so keys never wait; only the newest question is answered. Each folder and
  tool runs at once. fd looks for the text as typed (`--fixed-strings`) in names; ripgrep for it
  (`--fixed-strings --smart-case`) in files up to 2 MB, one line from each file. Each run reads at
  most 20 lines and is killed after 1.5 s. Names that equal the query, then begin with it, come
  first, then the rest of the names, then the contents; each file once, 12 at most.
- **Packaging:** the deb recommends `fd-find` and `ripgrep`.

## Consequences

- No daemon, no index, nothing running while the launcher is closed; a search is as fast as fd and
  ripgrep are on the folders chosen, and gives up after 1.5 s.
- It finds only what's in the folders chosen, and skips hidden files and what a `.gitignore` leaves
  out. A file bigger than 2 MB isn't searched inside; PDFs and Office files aren't read at all, as
  ripgrep reads text.
- Without fd or ripgrep, that half of the search finds nothing, and Settings says so. Neither is
  required.
- A query is text, not a pattern: no regular expressions.
- File rows draw a plain page glyph, not the file's own icon.
- The folders are on the launcher's own list, not the portal's or Files' bookmarks; adding one is
  typing its path in Settings.
- Other places to search (the whole home folder, mounted disks, apps' own data) and other ways
  (fuzzy names, recent files first) are for later; the config's shape leaves room.

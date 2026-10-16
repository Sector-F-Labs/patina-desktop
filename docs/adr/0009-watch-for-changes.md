# 0009. Watch the project's folders, and show changes as they're saved

Date: 2026-10-16

## Status

Proposed

## Context

The tree was scanned once when a project opened, and the file was read once when it was shown;
only Reload (Ctrl+R) read them again. The user found Reader slow to react when files changed,
in the tree and in the file on screen. Reader is often open beside an editor or an agent writing
the docs it shows.

Polling the file's modification time on a timer was the simpler option. It was ruled out
because it would be slow to react, or wake the window often while nothing changed. It also
wouldn't notice files added to the tree without rescanning on every tick.

## Decision

- Reader watches the folders of its open projects through the `notify` crate (inotify, FSEvents
  and the like). It watches each folder that the tree scans on its own, not recursively, so
  build output, dependencies, version control and hidden folders cost no watches. A lone file
  without a project is watched through its folder, which catches editors that save by replacing
  the file. The system's man pages aren't watched.
- Reader waits until things have been quiet for about 120 ms, then takes changes in, so a save in
  several writes is read once and whole. When a folder gains, loses or renames an entry, its
  project's tree is scanned again, along with its list of folders to watch. When the file on
  screen changes it is read again in place: scroll, history and notice stay, and a search
  stays at the match it was on.
- If the system can't watch, Reader says so on stderr and works as before, with Reload.

## Consequences

- Saved edits, and files added or removed, show without Reload.
- A new dependency (`notify`), and one inotify watch per folder in each open project. A very
  large tree can reach the system's watch limit; folders past it go unwatched without a word.
- Any structural change scans the whole project again, not only the folder that changed. That's
  cheap for the projects Reader is for, but it would need narrowing for huge trees.
- A file written in many bursts more than 120 ms apart is read once per burst.

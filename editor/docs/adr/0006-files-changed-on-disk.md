# 0006. A file changed on disk is read again, unless the tab has changes of its own

Date: 2026-10-25

## Status

Accepted. The user asked for reloading when a file changes on disk.

## Context

Files open in the editor are also changed by other programs: `git checkout`, formatters, other
editors. GNOME's reads them again when nothing would be lost, and asks otherwise.

## Decision

The folders of open files are watched (notify, not recursive; `watch.rs`), and a file that changed
is read on a thread. If it reads as the tab last saved or read it, nothing happens (that includes
our own saves). If the tab has no changes, its text is replaced, the cursor kept and the old text
kept as an undo step. If it has changes, a strip above the text says so, with Read Again and Keep
Mine. A file that disappears marks its tab as unsaved, with Save and Close.

## Consequences

- An unchanged tab always shows what's on disk.
- Watching folders means changes to other files in them wake the window, briefly.

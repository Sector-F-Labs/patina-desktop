# 0007. Saving replaces the file whole, keeping its line ends, encoding mark and permissions

Date: 2026-10-25

## Status

Proposed

## Context

A save that's interrupted part way must not leave a truncated file. Files also carry things the
editor shouldn't change: Windows line ends, a UTF-8 byte-order mark, an executable bit, a symlink
pointing at them, hard links, an owner other than the user.

## Decision

- Text is read as UTF-8; a byte-order mark is noted and dropped, and `\r\n` line ends are made `\n`
  for editing if most lines have them. Saving puts both back. The line ends can be changed from the
  status bar.
- A file that isn't UTF-8 opens read-only, with a note, so saving can't destroy the bytes shown as
  �. Other encodings are left for later.
- Saving writes a new file beside the old one, copies the old one's permissions, syncs it and
  renames it over the old one. A symlink is followed, so the link stays a link. A file with other
  hard links, owned by someone else, or in a folder that can't be written is written in place
  instead.
- Reading and writing happen on threads; the window never waits on the disk.

## Consequences

- A crash mid-save leaves the old file or the new one, never half.
- Replacing the file gives it a new inode; programs holding it open keep the old one.
- Non-UTF-8 files can be read but not edited.

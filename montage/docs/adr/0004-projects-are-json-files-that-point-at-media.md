# 0004. Projects are JSON files that point at the media

Date: 2026-10-26

## Status

Proposed. This design is mine; the user hasn't signed it off yet.

## Context

A project has to be saved and opened again. Home videos are big, and people keep them in their
own folders (`~/Videos`, a phone's import folder), so copying them into a project would double
the space they take. iMovie copies media into its library by default; Windows Movie Maker's
`.wlmp` files pointed at the originals.

## Decision

- A project is one `.montage` file of pretty-printed JSON (`Project` in `project.rs`): a
  `format` number (1), the library as absolute paths with ids, and the three tracks.
- What ffprobe found about each file is saved with it, so an opened project lays out at once;
  every file is read again on opening, for thumbnails, waveforms and any change.
- Saving writes a temporary file and renames it over the old one.
- A project from a newer format is refused rather than half-read.
- The project open on quitting is opened again at start, as iMovie does. Voiceovers are recorded
  into `~/Videos/Montage Voiceovers/`, and the project points at them like any other file.

## Consequences

- Moving or renaming a file breaks the project's link to it: the app says how many can't be
  found. There's no relinking yet.
- A project can't be moved to another machine with its media in one piece.
- The file is readable and diffable, and easy to write by hand (`scripts/demo.sh` does).

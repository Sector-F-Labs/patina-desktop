# 0002. The launcher lists folder apps in ~/Apps

Date: 2026-10-16

## Status

Accepted

## Context

ADR-0001 left "a folder of apps, as macOS has" for later. Patina OS has since settled what that
folder is: an app is a folder, `~/Apps/Name.app`, with an `app.toml` saying how to run it
(Patina OS ADR-0002), in a format with rules a launcher must follow (Patina OS ADR-0004,
format 1). Patina OS runs the desktop, so the desktop's launcher is where those apps appear; the
principle of not repeating the UI (patina-desktop ADR-0027) says they join the same list, not a second one.

The runner that will give each app the base image it names (Patina OS ADR-0003) doesn't exist
yet.

## Decision

- patina-launcher reads `~/Apps/*.app/app.toml` (`launcher/src/folders.rs`) and lists those apps
  beside the .desktop entries, searched and launched the same way: name, summary as the generic
  name, comment, keywords, icon from the folder, `args` after the program, `terminal`.
- It follows format 1's rules: only `*.app` folders, not hidden; `exec` and `icon` inside the
  folder; `exec` there and executable, picked by `{os}-{arch}` from a table; formats it doesn't
  know skipped with the reason on stderr; keys it doesn't know ignored.
- Each app runs straight on the running system, from the user's home folder; `base` is read past.
  Nothing here is particular to Patina OS: on Ubuntu or FreeBSD the folder works the same.

## Consequences

- Copying a folder into `~/Apps` adds an app to the launcher, and deleting it removes it, with
  nothing installed anywhere else. The list is read when the launcher opens.
- Until the runner exists, a folder app is as safe as any program the user runs, and depends on
  the libraries of the system it's on.
- Opening files and links with folder apps waits for format 1's later keys.

> Recorded first as patina-desktop ADR-0035, when the launcher lived there.

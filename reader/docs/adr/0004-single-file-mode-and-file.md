# 0004. A single-file mode without the tree, and --file

Date: 2026-10-16

## Status

Accepted

## Context

Reader opens a project with its tree. The user asked for a way to open just a file, without the
tree, by a flag, and for a `--file` option to always be there. Help is the case: an app's Help
should open its document alone, not a project browser, and shouldn't disturb the project the user
was last reading.

## Decision

- `--file FILE` names the file to open. With a folder too (`reader DIR --file FILE`), the file
  opens in that project; alone, in its repository, as a bare file argument does.
- `-s`/`--single` opens single-file mode: the file alone in a smaller window, no tree. Links and
  Back and Forward work as usual; File → Project Tree shows the tree. Nothing it opens is
  remembered as the last project or file. Given a folder, it opens the folder's README.

## Consequences

- Help can run `reader --single --file <doc>` and get a plain document window.
- Opening a document this way never changes what plain `reader` opens next.

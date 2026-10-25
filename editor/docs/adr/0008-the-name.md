# 0008. It's called Text Editor, `text-editor`, `com.sectorflabs.TextEditor`

Date: 2026-10-25

## Status

Proposed

## Context

The repository is `editor`, as the user named it. The command can't be `editor`: Debian and Ubuntu
already have `/usr/bin/editor` (the alternatives system's default editor, used by `git` and
others), and installing ours into `~/.local/bin` would shadow it with a window. GNOME's is "Text
Editor", run as `gnome-text-editor`.

## Decision

The app is **Text Editor**: the binary `text-editor`, the app id `com.sectorflabs.TextEditor`, its
settings in `~/.config/text-editor` and its session in `~/.local/share/text-editor`.

## Consequences

- No clash with `editor`, and the name says what it is, as GNOME's does.
- If another `text-editor` command turns up on a system, a rename means moving the config and data
  folders.

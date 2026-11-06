# 0008. Tabs, each a project or the manual, remembered

Date: 2026-10-16

## Status

Accepted. Supersedes remembering a single last project (ADR-0001).

## Context

The user asked for tabs outside single-file mode, as Notebook has, to read several projects at
once.

## Decision

- Each tab is a project, or the system's manual, with its own tree, filter, history, file and
  search marks; Patina's TabStrip shows them in the title bar after the menus, titled by the
  project's folder or "Manual".
- A new tab (Ctrl+T, `+`) offers Open Project and System Manual. Opening a project or the manual
  fills a new tab, or the tab in front when it's empty; a project or manual already open in a tab
  brings that tab forward instead. `reader DIR` does the same with the remembered tabs.
- The open tabs, what each shows, and the one in front are remembered in `config.toml`; a config
  from before tabs opens its last project as the one tab. Windows opened with `--single` or
  `--man` have no tabs remembered, and single-file mode shows no tabs.

## Consequences

- Several projects and the manual can be read side by side, and come back next time.

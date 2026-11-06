# 0011. Tabs, each a workspace of its own, in Patina's tab strip

Date: 2026-10-13

## Status

Proposed. The user asked for tabs that hold different root folders, and for a new tab, like a first
run, to show an Open Folder prompt in the sidebar. The details below are my design.

## Context

A window showed one workspace folder (ADR-0007). With no folder open, the sidebar was hidden and
the Open Folder button sat in the middle of the window. The user wants several folders open at once,
switching between them, and wants the sidebar itself to offer to open a folder when there's none.
Patina 0.1.5 added `patina::tabs::TabStrip` (Patina ADR-0009): tabs in the title bar in place of the
title, owned by the app.

We considered two other options:

- **Tabs for open notes within one workspace, like an editor.** This answers a different need; the
  sidebar already switches notes.
- **A window per workspace.** Without single-instance launching, each window would remember its
  own folders and the windows would overwrite each other's config.

## Decision

- **Each tab is a workspace.** Each tab has its own folder, tree, open note, `Editor` (with an id of
  its own), and Markdown view. The window keeps the config, skins, dialogs and messages. A tab can
  also hold a note open on its own (ADR-0007) or nothing yet.
- **The strip is Patina's `TabStrip`.** It goes after the menus, in place of the window title. A
  tab is named after its folder, or its note when the note is open on its own. A new tab is "New
  Tab". Keys: Ctrl+Shift+T and Ctrl+PageUp/PageDown from Patina, plus Ctrl+W to close. File has New
  Tab and Close Tab.
- **The sidebar is always shown.** Without a folder, it shows "No folder open" and an Open Folder…
  button. The middle of the window just points at the sidebar.
- **Leaving a tab saves its note, and so does closing one.** Only the shown tab can be edited, so
  autosave only watches that tab. If the save fails, the tab stays open.
- **Closing the last tab leaves a new, empty one.** The window stays open; Ctrl+Q quits.
- **The tabs' folders are remembered** in `config.toml` (`tabs` and `tab`) and restored on the next
  plain launch. A launch with a path (from the file manager, say) opens just that path and doesn't
  overwrite the saved tabs. `folder` still means the last folder opened, as in ADR-0007.

## Consequences

- Several notebooks can be open side by side in one window, and a first run or a new tab says
  plainly what to do.
- Tabs with no folder aren't remembered, and neither is each tab's open note: a restored tab opens
  its first note, as opening a folder always has.
- Two windows started without a path both write `tabs`, and the last one to change wins.
- The note's name is no longer in the title bar. The sidebar highlights the open note, and the tab
  shows a • while the note has unsaved changes.
- Many tabs shrink to Patina's 48 px minimum. Patina doesn't scroll the strip yet.

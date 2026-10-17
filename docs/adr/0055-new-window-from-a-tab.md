# 0055. A window's tab opens a new window of its app

Date: 2026-10-17

## Status

Accepted. The user asked that right-clicking an app on the bar offer a new window.

## Context

The bar's tabs are the window list (ADR-0012, ADR-0027). A tab's right-click menu had only Close
Window, and opening another window of an app already open meant going back to the launcher. Docks
on macOS, GNOME and Windows put New Window on a running app's menu.

Running an app's command again doesn't always give a new window. Firefox and Files see they're
already running and only raise their window. A `.desktop` entry can name a `new-window` action
for that (`firefox --new-window`, `nautilus --new-window`), and those apps do. The bar already
matches a window's app id to its entry for the tab's icon (ADR-0050).

The menu is drawn by Patina's `TabStrip`. Its `TabAction` enum is matched exhaustively by
Teleprinter, Notebook, Reader and Files, so a new variant would break them when they next update
Patina.

## Decision

- **Patina's `TabStrip` gains `.tab_menu(&mut dyn FnMut(&mut Ui, usize))`.** It adds items to each
  tab's menu, before Close, with the tab's index, and leaves `TabAction` as it was.
- **The bar's tab menu has New Window.** It runs, through the compositor's `Spawn` like
  everything else the bar opens (ADR-0045):
  - the window's entry's `new-window` action, if it has one;
  - else the entry's Exec, in `$TERMINAL` (or foot) for a terminal app;
  - else, with no entry, a program named like the app id, if one is installed.

  The lookup reads the installed entries on a thread. The matching moved from `icons.rs` to
  `apps.rs` for both uses.

## Consequences

- Another window of an open app is a right click away, for apps that open one when run again or
  name a `new-window` action.
- An app that only raises its window when run again, and names no action, still only raises it.
- An app with no entry and no program named after its app id (some X11 and Flatpak apps) gets
  nothing. The bar logs it.
- Patina apps' own tab strips can add menu items the same way.

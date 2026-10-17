# 0061. The window tabs say the app's name, then the title

Date: 2026-10-17

## Status

Accepted. The user asked for the window list to read "Appname - Title", or just the app's name
when the title repeats it.

## Context

The bar's tabs showed each window's title (ADR-0012), which tells windows of one app apart but
often not which app a window is: "~", "Home", "main.rs". Icons (ADR-0050) helped, but not with
`window_tabs = "titles"`, or at a glance. Many titles also already end in their app's name
("Patina — Mozilla Firefox"), so putting the name in front naively would say it twice.

The bar already reads each window's `.desktop` entry on its icon thread (ADR-0050, ADR-0060),
and an entry's Name is what the launcher calls the app.

## Decision

- **A tab says "Name - Title"** (`tab_title`):
  - With no title, or a title that only names the app ("Mozilla Firefox", "Teleprinter"),
    it's the name alone.
  - A last part of the title, after " — ", " – ", " - " or " | ", that names the app again
    is dropped: "Patina — Mozilla Firefox" reads "Firefox - Patina".
- **The name** is the `.desktop` entry's Name, else the last part of the app id. The icon thread
  answers names as well as icons, so no file is read while drawing. Until the name comes back,
  the tab shows the title as before.
- The tooltip on an icon-only tab says the same.

## Consequences

- Every tab says which app it is, and the title still tells its windows apart.
- Tabs are longer, so titles are cut off sooner on a crowded bar.
- Knowing the name is guesswork when a title merely contains it: a document called "Firefox
  notes" in Firefox, with no separator, shows as just "Firefox".

# 0004. The tabs and their unsaved text are kept across restarts; quitting never asks

Date: 2026-10-25

## Status

Accepted. The user asked for session restore and autosave as GNOME Text Editor does them.

## Context

GNOME Text Editor reopens the tabs open when it closed, with their unsaved changes, including
untitled documents that were never saved; quitting doesn't ask about changes. It keeps the changes
as drafts beside the session, not in the files.

## Decision

- The tabs are kept in `~/.local/share/text-editor/session.toml`: each tab's id, file (or its
  Untitled number), cursor and hand-chosen language, and which tab was in front.
- A tab's text, while it differs from its file, is written to `drafts/<id>.txt` a second after
  typing stops, on a thread. A tab back as saved has its draft removed.
- Quitting writes the outstanding drafts and the session, and asks nothing.
- Starting again reopens the tabs, each with its draft's text if it has one; its file is still read,
  so the tab knows what was saved and shows whether it differs. Drafts no tab names are removed.
- Closing a tab with changes does ask: Save, Close Without Saving, or Cancel.

## Consequences

- Nothing typed is lost to quitting, a crash (within a second), or a logout.
- Files on disk only ever change when the user saves.
- A draft can hold sensitive text in `~/.local/share` until it's saved or its tab is closed.
- There's no setting to turn this off yet; GNOME's "Restore session" switch, which makes quitting
  ask instead, would be a new decision.

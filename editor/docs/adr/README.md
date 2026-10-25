# Architecture Decision Records

This directory holds ADRs for Text Editor — short records of significant architecture decisions,
written at the time they're made, following Michael Nygard's format as popularized by Martin
Fowler: <https://martinfowler.com/bliki/ArchitectureDecisionRecord.html>.
## When to write one

Write an ADR when a decision is:

- Hard, or expensive, to reverse once other code depends on it.
- Not obvious from reading the code — a "why", not a "what".
- The kind of thing that'll get re-litigated later without a record ("didn't we already decide
  this?").

Small, reversible choices don't need one. Rule of thumb: if you'd want to explain the choice to a
new contributor in a paragraph beyond what the code itself shows, it's ADR-sized.

An ADR is a point-in-time record of *a decision as it was made*, including the options considered
and rejected. It does not get rewritten later — a decision that reverses or replaces an earlier one
gets its own new ADR that supersedes the old one, which stays in place with its status updated.

## Convention

- One file per decision: `NNNN-short-kebab-title.md`, numbered sequentially, zero-padded to 4
  digits. Numbers are never reused, even for a decision that gets reversed.
- Start from [`0000-template.md`](0000-template.md) — copy it, don't write from scratch.
- `Status` is one of: `Proposed`, `Accepted`, `Rejected`, `Deprecated`, or `Superseded by
  ADR-NNNN` (linked to the superseding record).
- Every new ADR gets a row in the index below.
- Keep records short — a paragraph or two per section is normal. An ADR documents a decision and
  its reasoning, not a design document.

Text Editor is a [Patina](https://github.com/Sector-F-Labs/patina) app. Decisions about the shared
frame, skins and components are recorded in Patina's own ADRs; record here only what this app
decides.

If you're working with Claude Code in this repo, `.claude/skills/adr/SKILL.md` automates the
mechanics of adding one.

## Index

| # | Title | Status |
|---|-------|--------|
| [0001](0001-a-text-editor-on-patina.md) | Build a text editor as a native egui app on Patina | Accepted |
| [0002](0002-egui-textedit-as-the-editing-surface.md) | egui's TextEdit is the editing surface | Proposed |
| [0003](0003-highlighting-a-line-at-a-time.md) | Syntax highlighting with syntect, a line at a time and kept between edits | Proposed |
| [0004](0004-sessions-and-drafts.md) | The tabs and their unsaved text are kept across restarts; quitting never asks | Accepted |
| [0005](0005-one-window.md) | One window: a second start hands its files to the first | Proposed |
| [0006](0006-files-changed-on-disk.md) | A file changed on disk is read again, unless the tab has changes of its own | Accepted |
| [0007](0007-saving.md) | Saving replaces the file whole, keeping its line ends, encoding mark and permissions | Proposed |
| [0008](0008-the-name.md) | It's called Text Editor, `text-editor`, `com.sectorflabs.TextEditor` | Proposed |
| [0009](0009-languages-from-bats-syntax-set.md) | The languages are bat's syntax set, through two-face | Accepted |

## Decision Relationship

```mermaid
flowchart TD
    A[0001: a text editor on Patina] --> B[0002: egui's TextEdit]
    B --> C[0003: highlighting a line at a time]
    A --> D[0004: sessions and drafts]
    D --> E[0005: one window]
    D --> F[0006: files changed on disk]
    A --> G[0007: saving]
    A --> H[0008: the name]
    C --> I[0009: bat's syntax set]
```

## Revisit Triggers

- Editing files of several megabytes matters: a widget of our own over a rope ([ADR-0002](0002-egui-textedit-as-the-editing-surface.md)).
- Skins should colour code: scopes mapped to colours in `[app.text-editor]` ([ADR-0003](0003-highlighting-a-line-at-a-time.md)).
- Someone wants quitting to ask instead: a Restore Session setting ([ADR-0004](0004-sessions-and-drafts.md)).
- Files in other encodings need editing ([ADR-0007](0007-saving.md)).

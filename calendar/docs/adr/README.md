# Architecture Decision Records

This directory holds ADRs for Calendar — short records of significant architecture decisions,
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

Calendar is a [Patina](https://github.com/Sector-F-Labs/patina) app. Decisions about the shared
frame, skins and components are recorded in Patina's own ADRs; record here only what this app
decides.

If you're working with Claude Code in this repo, `.claude/skills/adr/SKILL.md` automates the
mechanics of adding one.

## Index

| # | Title | Status |
|---|-------|--------|
| [0001](0001-build-on-patina.md) | Build Calendar as a native egui app on Patina, for plan on urd | Accepted |
| [0002](0002-colours-and-visibility.md) | Keep calendar colours on the server and which calendars show on this machine | Proposed |
| [0003](0003-sync-off-the-ui-thread-with-a-cache.md) | Sync on a worker thread, refetching only changed calendars, with a disk cache | Proposed |
| [0004](0004-icalendar-subset-and-chrono-tz.md) | Read a subset of iCalendar ourselves, resolve zones with chrono-tz, expand rules with rrule | Proposed |
| [0005](0005-account-in-a-private-file.md) | Keep the server login in a file only the user can read | Proposed |
| [0006](0006-own-calendar-skin-as-default.md) | Ship a built-in Calendar skin, red bar on paper, and start in it | Accepted (skin); Proposed (default, bar colouring); colour changed by [ADR-0007](0007-freebsd-red.md) |
| [0007](0007-freebsd-red.md) | Use freebsd.org's deep red for the Calendar skin and the icon | Accepted |

## Decision Relationship

```mermaid
flowchart TD
    A[0001: egui app on Patina, for plan] --> B[0002: Colours on the server, visibility here]
    A --> C[0003: Sync off the UI thread, cache]
    A --> D[0004: iCalendar subset, chrono-tz, rrule]
    C --> E[0005: Login in a private file]
    A --> F[0006: Calendar skin, the default]
    F --> G[0007: freebsd.org's red]
```

## Revisit Triggers

- Events show at the wrong time for some zone, or single occurrences need editing. See
  [ADR-0004](0004-icalendar-subset-and-chrono-tz.md).
- The app is used on a shared machine, or another app needs credentials. See
  [ADR-0005](0005-account-in-a-private-file.md).
- Big calendars make refreshes slow. Switch to `sync-collection` or etag-based fetching. See
  [ADR-0003](0003-sync-off-the-ui-thread-with-a-cache.md).

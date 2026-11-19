# 0002. A ledger file is a SQLite database

Date: 2026-11-05

## Status

Accepted

## Context

The user chose SQLite databases as the file format. A bookkeeping file should outlive the app:
readable and queryable by anything, with no format to reverse-engineer, and safe against a crash
in the middle of a save. SQLite gives atomic transactions and is one file that can be copied,
backed up and opened with `sqlite3`. A zip of XML (as .xlsx is) or CSV files were not wanted.

## Decision

We will store each ledger as one SQLite file:

- `PRAGMA application_id` is `0x4C444752` ("LDGR") and `PRAGMA user_version` is the schema
  version, 1 for now. Opening a database with another application id is refused, and the file is
  left untouched. A file with a newer `user_version` is refused too, so an old Ledger can't
  damage a newer file.
- The default extension is `.sqlite3`, so file managers and tools recognise it. `.ledger` was
  rejected because ledger-cli's plain-text journals already use it. The open dialog also
  shows `.sqlite` and `.db`.
- The journal mode stays at SQLite's default (rollback journal), so a ledger is always a single
  file, never a file plus `-wal` and `-shm` files to lose when it's moved.

## Consequences

- Other tools can read the books with plain SQL. Nothing hides in the file.
- Schema changes need a migration keyed on `user_version`; we have no migrations yet.
- There is no single-writer lock beyond SQLite's own. Two Ledgers on one file will each see the
  other's writes only after reopening (ADR-0004).
- A rollback journal fsyncs on each commit; fine for hand-typed cells, and edits are batched
  (ADR-0004).

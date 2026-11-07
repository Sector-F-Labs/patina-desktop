# 0115. Dbug watches the session bus as a monitor, and sends from a connection of its own

Date: 2026-11-07

## Status

Proposed. The user wanted an app to view D-Bus messages, in the way AKHQ shows a Kafka topic, and
asked whether it was possible, and whether a tool of ours could also send messages.

## Context

The desktop's pieces talk over the session bus (`Session`, `Keyboard`, `Processes`, the clipboard
planned in ADR-0114), and there was no way to see that traffic short of `dbus-monitor` in a
terminal.

The bus daemon has a monitoring mode: `org.freedesktop.DBus.Monitoring.BecomeMonitor` turns a
connection into one that gets a copy of every message matching the rules it gives. `dbus-monitor`,
`busctl monitor` and Bustle use it. A monitor can't send or reply, and the bus never waits for it.
On the session bus it works for the same user with no special rights. The system bus needs a
policy that allows it, which usually means root.

Options considered:

- **Eavesdropping match rules** (`eavesdrop=true`), the old way. Daemons have refused them since
  dbus 1.9.
- **`dbus-monitor` as a child process, parsing its text.** It's lossy, and replies would have to be
  matched to calls from text.
- **`BecomeMonitor` through zbus,** which is already the desktop's D-Bus library.

## Decision

- **Dbug is `dbug/` (patina-dbug)**, a Patina app in this workspace.
- **It watches with `BecomeMonitor` through zbus** on a thread of its own, with match rules from
  `--match` (none: everything). Each message becomes a `Record` (`monitor.rs`). A reply or error is
  matched to its call by the caller's name and serial, with the time between them.
- **Sending uses a second, ordinary connection.** The monitor is listen-only because the bus
  makes it so, so a call or a signal Dbug sends goes out on a connection of its own, and shows up
  in the list as any message does, with its reply.
- **Session bus first.** The system bus comes later, with a way to be allowed (a policy file or
  `pkexec`).
- **Arguments are a form built from their type,** not a copy kept of each message type
  (`value.rs`). A D-Bus signature says everything a form needs: booleans, numbers, strings,
  object paths, lists, dictionaries, structs and variants (which say their own type). Two things
  give a signature: a captured message, whose values fill the form (Resend…), and, later, an
  introspected method's argument types, which give a blank form. Nothing is stored per interface,
  so nothing goes stale when a program changes its API.
- **Each view says what it shows,** as the process manager does: a paragraph at the top, hovers on
  the columns and controls, and a line for the bus's own messages and the standard interfaces
  (`explain.rs`), with man pages opened in Reader.
- **The window:** a filterable table (every word must match), pause, follow, a detail pane, and
  a ring of the last 20,000 messages.

## Consequences

- Everything on the session bus is readable by Dbug, passwords and clipboard previews included
  (ADR-0114). The session bus never kept those from the user's own programs, and Dbug shows them
  only to the user who ran it.
- Arguments are shown by zvariant's text form. zvariant parses a body's signature as one struct, so
  two arguments `su` and a lone struct `(su)` look the same, and Dbug shows `su` for both.
- Unique names (`:1.42`) aren't yet shown as the programs behind them. `GetConnectionCredentials`
  would give that.
- Sending takes the user's word for the destination, path, interface and member, and checks only
  the types of the arguments. A call does whatever the program does with it, including things
  that can't be undone, so the sender says so.
- Browsing what's on the bus (names, objects, methods from `Introspect`) isn't built. Until it is,
  a method to call has to be known, or found by resending a message that was captured.
- A byte array (`ay`) is edited a byte at a time, which is slow for anything large.
- A busy bus can produce thousands of messages a second. The match rules and the ring keep the
  window usable. A monitor that falls behind makes the daemon drop it, so the reading thread does
  nothing but make records.

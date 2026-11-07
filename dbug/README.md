# patina-dbug

Dbug (ADR-0115): a live viewer for the messages on the session bus, a Patina app. It asks the bus
daemon to make it a monitor (`BecomeMonitor`), so it reads every message and sends none.

## On the desktop

- A table of calls, replies, errors and signals as they pass: time (UTC), type, from, to,
  interface, member and arguments. A filter above it keeps the rows where every word matches.
- A detail pane for the one picked: serial, reply serial, path, signature, the arguments, and for
  a reply the call it answers and how long it took.
- Pause (Space), Follow, Clear. The last 20,000 messages are kept.
- **Message ▸ Send Message…** opens a sender on the right: a call or a signal, its destination,
  path, interface and member, and its arguments as a form that follows their types (strings,
  numbers, booleans, object paths, lists, dictionaries, structs, variants). It sends from a
  connection of its own, and the answer, or the error, is listed under it; the monitor shows the
  message passing like any other. **Resend…** (the detail pane, a right click on a call or
  signal, or Message ▸ Resend Selected…) fills the sender from a message that went by.
- Each view has a short explanation, and hovers on the columns, the controls and the fields; the
  bus's own messages (`NameOwnerChanged`, `AddMatch`, `PropertiesChanged`…) say what they mean.
- `patina-dbug --match RULE` narrows what the bus sends, with a D-Bus match rule such as
  `type='signal',interface='com.sectorflabs.Patina.Keyboard'`.

Not built yet: browsing the bus's names, objects and methods to pick one to call, names shown
as programs, the system bus, saving a capture, an icon and a launcher entry.

## Tests

`cargo test -p patina-dbug` checks how a message becomes a record and how the argument forms
turn into values. `dbus-run-session -- cargo test
-p patina-dbug -- --ignored` runs the monitor and the sender against a bus of its own, with `dbus-send`.

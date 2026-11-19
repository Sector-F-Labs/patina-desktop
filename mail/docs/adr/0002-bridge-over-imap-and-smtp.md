# 0002. Talk to Proton Mail Bridge over IMAP and SMTP, on worker threads

Date: 2026-10-18

## Status

Proposed

## Context

Proton Mail's own API is end-to-end encrypted and not meant for third-party clients. Proton Mail
Bridge runs locally, decrypts mail, and serves it as ordinary IMAP (127.0.0.1:1143) and SMTP
(127.0.0.1:1025), with STARTTLS by default (SSL can be chosen in Bridge's settings). It uses its
own generated password, and a self-signed certificate made for 127.0.0.1, so normal certificate
checks fail.

Patina's rules say network I/O never runs on the UI thread. IMAP is a stateful, blocking protocol;
async IMAP crates would need a runtime the rest of the app doesn't use.

Considered: `async-imap` with tokio (rejected: a runtime for one connection), pinning Bridge's
exported certificate (rejected for now: the user would have to export and point the app at it,
and Bridge regenerates it).

## Decision

We will:

- Use the blocking `imap` crate (3.0 alpha, native-tls) for reading and `lettre` for sending, and
  `mail-parser` to parse messages.
- Run one IMAP session on a worker thread that takes `Command`s and sends back `Update`s
  (`src/sync.rs`), in the same shape as Calendar's sync. A dropped connection is reopened once and
  the command retried. Each send runs on a short-lived thread of its own.
- Skip certificate verification only when the server is this machine (`127.0.0.1`, `localhost`,
  `::1`), where Bridge runs; any other host must present a valid certificate.
- Show a folder's newest 200 messages, fetched by sequence number with their headers, and fetch a
  message's body when it's opened (`BODY.PEEK[]`, then `\Seen` is set). Nothing is cached on disk.
- Leave saving to Sent to Bridge, which files sent mail itself.

## Consequences

- Simple code with no async runtime; the UI never waits on Bridge.
- Mail keeps no copy of your mail on disk, but it also can't show anything offline, and opening a
  folder always waits for Bridge.
- There is no IDLE: new mail shows on a two-minute refresh, F5, or opening a folder.
- Older mail than the newest 200 in a folder isn't reachable yet (no paging or search).
- On localhost the TLS session isn't authenticated, which is only acceptable because the traffic
  never leaves the machine.
- The `imap` crate is an alpha release; its API may change under us.

# Mail

A Patina (egui) mail client for Proton Mail through Proton Mail Bridge (IMAP 127.0.0.1:1143, SMTP
127.0.0.1:1025, STARTTLS, self-signed certificate). Folders sidebar, message list, reading pane,
compose window.

Significant decisions are ADRs in `docs/adr/`; read them before changing sync, TLS or where the
login is kept. Use the `adr` skill to add one.

## Commands

```bash
make check         # tests + clippy -D warnings
make demo          # made-up mail (MAIL_DEMO=1), no connection, scratch config
make screenshot    # docs/screenshot-*.png from the demo mail (MAIL_DEMO=reply opens a reply)
make live-test     # ignored IMAP test against Bridge: MAIL_LIVE_ACCOUNT=<account.toml>
make install       # per-user install; test with PREFIX=<scratch dir>
```

Never run test instances against the user's config. The Makefile's `SCRATCH_ENV` points XDG
config, data and cache at `target/scratch`. Don't read the user's real `~/.config/mail/account.toml`
or Bridge's vault: the user enters the Bridge password in File → Account…. eframe persists window
state (including the compose window's position) in `XDG_DATA_HOME`, so clear `target/scratch`
between screenshot runs if a window turns up in the wrong place.

Blitz (pinned `=0.3.0-beta.4`) quirks: a refused resource must still be answered (empty bytes),
or it stays a pending critical resource and holds back layout; it doesn't pass overflow up
through tables, so the page's extent is measured over every element (`Page::measure`), not the
root's scroll size; inside a horizontally scrolling `ScrollArea`, egui offers more width than the
pane, so the pane width is taken outside it. `spikes/blitz` is the original experiment, with a
hostile test corpus and a standalone viewer.

Bridge quirks: "Folders" and "Labels" are `\Noselect` parents (the sidebar shows them as headings);
Bridge files sent mail in Sent itself, so Mail doesn't append it.

## Layout

- `src/config.rs`: `config.toml` and the private `account.toml` (`Account`, `Security`).
- `src/message.rs`: `Summary` (list rows, from headers) and `Body` (whole message) via mail-parser;
  HTML-only mail through html2text.
- `src/compose.rs`: `Draft`, reply/reply-all/forward, building the lettre `Message`.
- `src/html.rs`: HTML mail through Blitz (ADR-0004, ADR-0005): the `Guard` net provider (only
  `cid:`/`data:`), the `Links` navigation provider, scale-to-fit, painting only the visible part
  into a texture, pointer events to Blitz, the misleading-link check.
- `src/sync.rs`: IMAP worker thread (`Command` → `Update`), SMTP sends, folder ordering.
- `src/app.rs`: window, header and menus, sidebar, list, reader, compose window, account dialog,
  shortcuts, screenshot mode.
- `src/demo.rs`: the made-up mailbox for `MAIL_DEMO`.

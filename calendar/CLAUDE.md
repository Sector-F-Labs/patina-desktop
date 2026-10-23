# Calendar

A Patina (egui) calendar app for a CalDAV server, made for `plan` on urd
(`https://plan.divanv.com`, source in `../plan`). Day, week and month views, coloured calendars
that can be shown and hidden, and event create/edit/delete.

Significant decisions are ADRs in `docs/adr/`; read them before changing sync, storage or the
iCalendar handling. Use the `adr` skill to add one.

## Commands

```bash
make check         # tests + clippy -D warnings
make dev-server    # local plan serve on :5299 with seeded calendars (user test / password test)
make live-test     # CalDAV client tests against it
make dev           # run the app against it, in a scratch config
make screenshot    # docs/screenshot-*.png, against it
make install       # per-user install; test with PREFIX=<scratch dir>
```

Never run test instances against the user's config or the real server. The Makefile's
`SCRATCH_ENV` points XDG config, data and cache at `target/scratch`. Don't read or copy the real
server password (`~/.config/plan/config.toml` on urd): the user enters it in File → Account….

`plan` quirks: its `default` calendar is labelled "Tasks" but holds events (the real task list,
`tasks`, is VTODO-only and is filtered out), and it ignores `If-None-Match`.

On Wayland the window icon comes from the installed launcher (`com.sectorflabs.Calendar.desktop`),
matched by app id, so `cargo run` shows it only after `make install`.

## Layout

- `src/ics.rs`: iCalendar read/write (component tree, folding, escaping), times and zones.
- `src/model.rs`: `Calendar`, `Event`, and expanding events into `Occurrence`s (rrule).
- `src/edit.rs`: `Draft` for the event dialog; create, update, exclude one occurrence, remove.
- `src/caldav.rs`: blocking CalDAV client (ureq, roxmltree), plus a live test.
- `src/sync.rs`: worker thread, commands and updates, disk cache.
- `src/views.rs`: month, week/day time grid, mini month; return `Action`s.
- `src/app.rs`: window, header and menus, sidebar, dialogs, shortcuts, screenshot mode;
  `Opening`, where `--open` points it.
- `src/agenda.rs`: `--agenda`, the coming days as JSON from the cache, for the Patina desktop's
  clock (ADR-0009). Its fields are an interface: add, don't rename.
- `src/main.rs`: the command line (`parse`), then `--agenda` or the window.
- `src/config.rs`: `config.toml` and the private `account.toml`.
- Skins: the Calendar skin lives in Patina (`skins/calendar.toml` there, ADR-0008); this app only
  makes it the starting one (`HOME_SKIN` in `app.rs`). Patina's `TitleBar` colours the bar's
  controls when a skin colours the bar.

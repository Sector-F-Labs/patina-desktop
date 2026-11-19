# Podcasts

A Patina (egui) podcast player: subscriptions from RSS feeds, search through Apple's iTunes
Search API (no key), episodes played with rodio while they download.

Podcasts is a member of the patina-desktop workspace (one `Cargo.lock` and `target/` for the
repository; build it with `-p podcasts`, which the Makefile does), and still its own app. It used to
be its own repository, Sector-F-Labs/podcasts; the history up to the move is kept here.

Significant decisions are ADRs in `docs/adr/`; read them before changing where feeds come from or
how playback and downloads work. Use the `adr` skill to add one.

Patina comes from its `develop` branch (see `../AGENTS.md`). What the app needs from Patina is
changed there, not worked around here.

## Commands

```bash
make check         # tests + clippy -D warnings
make dev           # run in a scratch config seeded from packaging/dev-config.toml
make screenshot    # docs/screenshot*.png, in a scratch config
make icons         # assets/podcasts-icon-128.png from packaging/linux/podcasts.svg
make install       # per-user install; test with PREFIX=<scratch dir>
```

Never run test instances against the user's config, downloads or listening progress: the
Makefile's `SCRATCH_ENV` points XDG config, data and cache at `target/scratch`. Tests must not
call anything that saves (`Config::save`, `Listening::save`, `Downloads`), since those write to
the real XDG dirs; `App::with` builds the app without loading or fetching. In zsh, an unquoted
`$VAR` holding several `A=b` words doesn't split, so run such commands through `bash -c` or make.

Screenshot mode: `PODCASTS_SCREENSHOT=path.png`, and `PODCASTS_SKIN=<name>`.

## Layout

- `src/feed.rs`: `Show` and `Episode`, RSS parsing (`rss` crate), show notes as text
  (`html2text`), Apple search answers and Apple Podcasts links.
- `src/fetch.rs`: feeds, search, lookup and artwork on short-lived threads, replies over a
  channel; the feed cache and artwork thumbnails.
- `src/download.rs`: `Downloads`, and `GrowingFile`, which reads a file still downloading.
- `src/player.rs`: `Engine`: a decoding thread per episode feeding chunks to the `Feed` source
  rodio plays; seeks by generation (ADR-0003).
- `src/config.rs`: `~/.config/podcasts/config.toml`, and `Listening` (where you are in each
  episode).
- `src/app.rs`: window, header and menus, sidebar and search, episode list, player bar, status
  bar, shortcuts, screenshot mode; a test that draws it with a real feed.

`testdata/coffee-break-swedish.xml` is a real feed for the tests.

## Gotchas

- rodio pulls samples on the audio callback thread: nothing there may wait on I/O. The `Feed`
  plays silence when the decoding thread is behind.
- MP3s seek coarsely; an accurate Symphonia MP3 seek parses from the file's start, which would
  wait for the whole download.
- Inside Claude Code's Bash sandbox the window never gets a frame, so the app (and
  `make screenshot`) sits there without fetching anything. Run it outside the sandbox.
- A clickable row must sense clicks through `UiBuilder::sense` on its own `Ui`, beneath its
  contents. A `ui.interact` over the row's rect after drawing it sits on top and takes the
  clicks of the buttons inside (▶ once only opened the notes). Test clicks with pointer events.

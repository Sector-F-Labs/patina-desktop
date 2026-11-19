# Home

A private Patina (egui) interface for Home Assistant. Sector F Labs; no GitHub repo. Decisions are
ADRs in `docs/adr/` (use the `adr` skill). Sector F's `AGENTS.md` applies: `patina-ui` comes from
Patina's `develop` branch.

Home is a member of the patina-desktop workspace (one `Cargo.lock` and `target/` for the
repository; build it with `-p home@<version>`, which the Makefile does: a crate named `home` is on crates.io too, so `-p home` is ambiguous), and still its own app. It used to
be its own repository, Sector-F-Labs/home; the history up to the move is kept here.

## Commands

```bash
make check        # tests + clippy -D warnings
make dev          # live reload, scratch config
make screenshot   # docs/screenshot-*.png from demo entities (HOME_DEMO=1)
make install      # per-user install; test with PREFIX=<scratch dir>
```

Never run test instances against the user's config or real Home Assistant. Use `HOME_DEMO=1` (made-up
entities) with the Makefile's scratch config. Don't read `~/.config/home/account.toml`: the user
enters the token in File → Account….

## Layout

- `src/ha.rs`: `Account`, `Entity` and its controls, the REST client on a worker thread, the one-shot
  WebSocket fetch of Lovelace dashboards (ADR-0004), `demo()` and `demo_dashboards()`.
- `src/dashboard.rs`: draws a Lovelace view and its cards; add a card type in `card_ui`.
- `src/widgets.rs`: `entity_row`, `Event` and `ServiceCall`, shared by the entity list and the dashboards.
- `src/chart.rs`: sensor graphs with egui_plot, the small one on cards and the `SensorDialog` window (ADR-0006).
- `src/history.rs`: parsing and thinning of the history API, and the `Store` of fetched series.
- `src/palette.rs`: the graphs' colours, from defaults and a skin's `[app.home]`.
- `src/light.rs`: the light settings window (ADR-0005); `HOME_OPEN=light.x` or `sensor.x` opens a window for screenshots.
- `src/config.rs`: `config.toml` and the private `account.toml`.
- `src/app.rs`: window, header and menus, sidebar (dashboards, then entity domains), account dialog, screenshot mode.

On Wayland the window icon comes from the installed launcher (`com.sectorflabs.Home.desktop`), so
`cargo run` shows it only after `make install`.

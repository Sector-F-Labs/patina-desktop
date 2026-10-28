# Home

A private Patina (egui) interface for Home Assistant. Sector F Labs; no GitHub repo. Decisions are
ADRs in `docs/adr/` (use the `adr` skill). Sector F's `AGENTS.md` applies: `patina-ui` comes from
Patina's `develop` branch.

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
- `src/widgets.rs`: `entity_row`, shared by the entity list and the dashboards.
- `src/config.rs`: `config.toml` and the private `account.toml`.
- `src/app.rs`: window, header and menus, sidebar (dashboards, then entity domains), account dialog, screenshot mode.

On Wayland the window icon comes from the installed launcher (`com.sectorflabs.Home.desktop`), so
`cargo run` shows it only after `make install`.

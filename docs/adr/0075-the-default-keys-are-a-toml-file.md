# 0075. The default keys are a TOML file built in, and Alt+Tab is fixed

Date: 2026-10-21

## Status

Accepted. Refines [ADR-0069](0069-the-desktops-keys-are-config.md). The user asked for the
key mappings to live in a TOML file in the source, built into the binary, so they're edited
without going through Rust, with every key but the escape hatches and Alt+Tab changeable in config.

## Context

ADR-0069 made the desktop's own keys config by action, but their defaults were a Rust table
(`keys::DEFAULTS`), and the config written on first start repeated them by hand in
`config.example.toml`. Moving a default meant editing both. Alt+Tab was one of the changeable
actions (`next_tab`, `previous_tab`), though it's how one gets out of a fullscreen game
(ADR-0041), as Ctrl+Alt+Backspace is how one gets out of the desktop.

## Decision

- **The defaults are `compositor/keys.toml`,** in the same form as `[keys]` in the config,
  read into the binary with `include_str!` and parsed once. `[keys]` replaces an action's keys
  as before; the others keep the file's.
- **Rust keeps what an action does:** `keys::action` maps each name to a `KeyAction`. A new
  key for an existing action is an edit to the TOML; a new action is code too. The tests read the
  file and check every name and key in it.
- **The config written on first start takes its keys from the file:** `config::starter()` puts
  the file's settings, commented out, where `config.example.toml` has
  `# {the defaults from compositor/keys.toml}`. The file's own notes stay in the source.
- **Alt+Tab and Alt+Shift+Tab are fixed in `input::key_action`,** with quitting and restarting
  the desktop, the consoles and the media keys, matched before `[keys]`. `next_tab` and
  `previous_tab` are gone. A key in `[keys]` that one of these already is gets a warning and is
  left out.

## Consequences

- One place to change a default key, readable without Rust; a rebuild still puts it in the
  binary.
- A config that sets `next_tab` or `previous_tab` now gets "no action" warnings, and Alt+Tab
  works as before.
- An action set in `[keys]` but missing from `keys.toml` still works: the two are merged by
  name, so `keys.toml` doesn't have to list every action, though it does.

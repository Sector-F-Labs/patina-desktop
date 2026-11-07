# 0118. Skin Builder makes skins and applies them to the apps in ~/.config

Date: 2026-11-07

## Status

Accepted. Builds on Patina's ADR-0004 (skins in Patina).

## Context

A Patina skin is a TOML file in the suite skin format. Making one meant copying a built-in into
an app's `~/.config/<app>/skins` and editing hex colours by hand with nothing to show the result,
then setting `skin = "…"` in that app's `config.toml`, once for every app. Settings picks a skin
for the shell only.

## Decision

- **A Patina app of its own, `patina-skin-builder`**, installed by `make install` with a launcher
  entry and icon. Settings stays the place to pick a skin; this is where they are made.
- **Your skins are in `~/.config/patina-skin-builder/skins`.** Edit has every colour of the shared
  sections (`[colors]`, `[controls]`, `[window]`) as a picker, the base, corner radius and shadows,
  and a window drawn in the skin beside it. New starts from a fresh skin or a copy of a built-in.
- **The file stays a TOML document** (`toml_edit`), so keys the editor has no control for, such as
  `[window.controls]` and `[app.*]`, and the comments survive an edit. A colour left out is shown
  as egui's own, and can be put back to that.
- **Apply forces a skin into apps.** It lists every folder in `~/.config`, with the Patina apps
  (a `skins` folder, or a `skin` in their config) first and ticked on request. For each ticked one it
  writes the skin file into `<folder>/skins`, replacing one of that name, and sets `skin` in its
  `config.toml`, keeping the rest of that file. A config that won't parse is left alone and reported.
  Folders that aren't known as Patina apps can be ticked too: the user asked to force it.
- **Apps show it when next started.** Most read their skin once at start.

## Consequences

- Making a skin and trying it on every app is one window and one click.
- Forcing can write a `skin` key and a `skins` folder into an app that has no use for them. It's
  the user's choice, per folder, and nothing is deleted.
- Not built yet: editing `[window.controls]`, showing the colours' contrast, importing a skin
  from a file, and telling a running app to reload.

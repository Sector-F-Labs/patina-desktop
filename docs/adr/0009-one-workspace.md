# 0009. Keep the compositor and the bar in one cargo workspace

Date: 2026-10-14

## Status

Accepted

## Context

The compositor and the bar will change together: workspaces, window titles and clicks all need
both sides. The user chose one workspace for them over separate repos.

## Decision

This repo is a cargo workspace: `bar/` (patina-hud) and `compositor/` (patina-compositor). They
stay separate crates and binaries; the compositor starts the bar it finds beside itself. Later
shell pieces (a launcher) join as further members.

## Consequences

- One `make check`, one VM, and a change that spans both lands in one commit.
- The repo is still named patina-hud; renaming it is the user's call.

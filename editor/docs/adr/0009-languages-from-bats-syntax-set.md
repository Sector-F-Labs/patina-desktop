# 0009. The languages are bat's syntax set, through two-face

Date: 2026-10-25

## Status

Superseded by [ADR-0010](0010-the-highlighter-lives-in-patina.md)

## Context

syntect's default syntaxes ([ADR-0003](0003-highlighting-a-line-at-a-time.md)) are the Sublime
Text packages of years ago: 75 languages, without TOML, TypeScript, Dockerfile and other everyday
files. A `Cargo.toml` opened as plain text.

The options: add a TOML `.sublime-syntax` of our own to the defaults, which means building and
linking the whole set at start-up; or take a set someone keeps up, precompiled. The `two-face`
crate packages bat's syntaxes, syntect's own included, as a dump built for syntect 5.3.

## Decision

The syntaxes come from `two_face::syntax::extra_newlines()` (the fancy-regex build, as our syntect
is), in place of syntect's defaults. The themes stay syntect's, as ADR-0003 says.

## Consequences

- TOML, TypeScript, Dockerfile, Nix and the rest of bat's 199 languages are highlighted, and the
  language menu lists them all.
- About a megabyte more in the binary; loading takes about 2 ms, once.
- New languages come with two-face's releases, not ours; one it lacks still needs a syntax of our
  own added to the set.

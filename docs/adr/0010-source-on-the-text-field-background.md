# 0010. Source files on the text-field background

Date: 2026-10-25

## Status

Accepted. The user found source files and code fences "quite awful with their light grey background
on default skin", and asked for Text Editor's look instead. Supersedes
[ADR-0003](0003-source-on-the-code-background.md).

## Context

ADR-0003 put source on `code_bg_color` to match markdown's code blocks. Skins set that and the
text-field colour to the same `surface`, but in Default (stock egui) `code_bg_color` is grey 230 in
light and 64 in dark, a heavy slab under the code. Text Editor draws its text on `extreme_bg_color`,
the text-field background, white or near-black in Default, and Patina's code blocks now do too
(Patina's ADR-0026).

## Decision

A source file is drawn in the same rounded frame as before, filled with `extreme_bg_color`.

## Consequences

- Source, code fences and Text Editor look alike, in Default as in every skin.
- In skins nothing changes: `surface` sets both colours.

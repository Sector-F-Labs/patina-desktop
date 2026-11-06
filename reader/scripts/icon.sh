#!/bin/sh
# Renders the app icon (packaging/linux/reader.svg) to docs/icon.png at
# 120 px, for the README. SIZE and OUT change either.
set -eu
cd "$(dirname "$0")/.."
cargo run --quiet --example render_icon -- "${SIZE:-120}" "${OUT:-docs/icon.png}"

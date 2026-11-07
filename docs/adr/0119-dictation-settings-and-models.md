# 0119. Dictation's model, language and accuracy are settings, and models download from Settings

Date: 2026-11-07

## Status

Accepted. The user's accent (South African English) and mumbling sometimes make dictation type
the wrong words, and they wanted to try things rather than guess. Builds on
[ADR-0070](0070-dictation-with-whisper-on-the-machine.md) and [ADR-0073](0073-dictation-shows-an-overlay.md).

## Context

The daemon was started by the compositor with no arguments, so it always used base.en, with
Whisper's language left to the model and greedy decoding. The only setting was where the overlay
shows. Trying a bigger model meant curl, a file name and a command line the compositor never
passes.

## Decision

- **patina-dictate's config holds them**: `model`, `language`, `beam_size`, `no_speech`, `prompt`,
  `threads` and `fallback`, next to `overlay`. Values are clamped to what whisper.cpp takes. The
  command line's `-m` and `-l` still win.
- **The daemon reads the config at each recording** (as it already did for the overlay). A changed
  model or language is loaded then; the rest is applied to that recording. No restart.
- **Settings' Dictation page** has the model list (a short one, tiny to large-v3-turbo with
  compact q5_0 variants, English-only and multilingual), each with Download, Cancel and Delete,
  the language, and the accuracy controls with a sentence on what each does. Edits keep the rest of
  the file (`toml_edit`).
- **Downloads are `curl`** into `ggml-NAME.bin.part`, renamed when done, with progress from the file's
  size. No HTTP client is added to the workspace; curl is on the machines this runs on.

## Consequences

- Trying a model or a setting is a click and the next press of the dictate key.
- Settings' list of models and patina-dictate's names are kept in step by hand: a name is whisper.cpp's.
- A beam search above 1 and the larger models are slower; the page says so, and the measurements
  of [ADR-0074](0074-whisper-runs-on-the-gpu-through-vulkan.md) still apply.

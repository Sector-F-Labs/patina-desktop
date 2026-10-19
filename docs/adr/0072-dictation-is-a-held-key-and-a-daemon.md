# 0072. Dictation is a held key in the compositor and a daemon that listens for it

Date: 2026-10-19

## Status

Accepted. The user asked for dictation to be wired to a key once Commit worked on nifl.

## Context

patina-dictate transcribes (ADR-0070) and the compositor types text into the focused window
(ADR-0071). What's left is the key: hold it, speak, let go, and the words appear. Key actions
(ADR-0069) only fired on press, and the transcriber needs its model loaded before a press, not
after, or the first words wait on it.

Considered and ruled out:

- **The compositor starting patina-dictate on each press**, stopping it on release. The model
  would load on every press: about 0.1 s for base.en on the CPU, over a second for medium or with
  CUDA.
- **The compositor calling the daemon** (Start, Stop methods). The compositor would wait on the
  bus from its event loop, and own the daemon's interface; tiling already settled the split the
  other way: the compositor signals, a program of its own decides (ADR-0053).

## Decision

- **A `dictate` action in `[keys]`, Alt+Space by default.** The key going down signals
  `DictationStarted` on `com.sectorflabs.Patina.Input`; the same key coming up, whatever else is
  down by then, signals `DictationStopped`. A key repeating doesn't start it twice. The signals
  are emitted from a thread, so the loop never waits on the bus.
- **`patina-dictate --daemon`**, which the compositor keeps running as it does the tiler. It
  records from start to stop, drops presses under 0.3 s as slips, transcribes on a thread of its
  own (so the next press records at once), and calls `Commit`. The model is loaded at start, and
  again on each recording until it loads, so a model downloaded later is used without a restart.
- **Commit waits for modifiers held by hand to be let go**, up to two seconds, so the window
  sees Ctrl+V and not Ctrl+Alt+V when Alt is still down from Alt+Space.

## Consequences

- Dictation works anywhere on the desktop with no setup but a model in
  `~/.local/share/patina-dictate/models` (base.en by default).
- Nothing shows while it records or transcribes yet: an overlay and the bar's tray state are
  next.
- The microphone is opened at each press, which takes a moment; the first syllable may be cut.
  Keeping the stream open would cost the privacy indicator and some power.
- The daemon holds the model in memory all session: about 150 MB for base.en.
- Alt+Space is taken from apps; `dictate = ""` in `[keys]` gives it back.
- Without a model, every press logs that there's none, in the daemon's log in Settings'
  Processes page.

# 0073. Dictation shows an overlay with the voice's waveform, its own process fed on stdin

Date: 2026-10-19

## Status

Accepted. The user asked for an overlay with a waveform that reacts to the voice, placed at the
bottom or top of the screen from Settings.

## Context

Dictation (ADR-0072) showed nothing while a key was held or while the words were transcribed, so
there was no telling whether it was listening, had heard anything, or was still working. The
daemon that records has the sound; it has no Wayland connection and runs all session.

Considered and ruled out:

- **Drawing from the daemon itself.** It would hold a Wayland connection and a layer surface all
  session for something shown a few seconds at a time, and its event loop would have to serve the
  host and the bus together.
- **A long-lived overlay process, shown and hidden over D-Bus**, with the levels as signals: thirty
  signals a second on the session bus, and one more interface, for what one pipe does.
- **egui_plot**, which the weather app uses: built for charts with axes; a few rounded bars drawn
  with egui's painter are lighter, and cheap for the CPU painter the pieces use (ADR-0002).

## Decision

- **`patina-dictate --overlay bottom|top`** is the overlay: a pill on patina-shell in the bar's
  skin, on the overlay layer, taking neither keyboard nor pointer, centred at that edge and clear
  of the bar's exclusive zone.
- **The daemon starts it at each press and feeds it on stdin**, as the bar feeds its tooltips: a
  loudness (the RMS of the last thirtieth of a second) a line, thirty times a second, from a
  `Meter` on the recording; `transcribing` when the key comes up; and stdin closed once the text
  is committed, or there is none, which fades it out. A slip under 0.3 s closes it at once.
- **What it shows:** a red dot that swells with the voice and a waveform of mirrored bars, a bar a
  level, scrolling left; then a spinner, the bars settling and a band sweeping along them.
  Colours come from the skin's visuals only.
- **Its edge** is `overlay` in `~/.config/patina-dictate/config.toml`, read at each press, so a
  change applies to the next one. Settings gets a Dictation page that writes it (with
  `toml_edit`, keeping the rest) and shows the key held to dictate, from `[keys]`.
- **It can be tried alone**: made-up levels piped into `--overlay` show it with no microphone,
  model or key, and an ignored test draws it in software as PNGs.

## Consequences

- Dictation shows that it's listening, how loud it hears you, and that it's working on the words.
- A process and a Wayland connection start at each press, a few tens of milliseconds behind the
  key; the recording has already started, so nothing is lost.
- The overlay is found on PATH as `patina-dictate`, where the compositor puts its own folder
  first, so a binary replaced under the running daemon still starts the new one.
- It follows the bar's skin when it starts, not while it's up; it's up for seconds.
- A config of dictation's own, beside the compositor's and the bar's: the model and language
  will go there too when Settings offers them.

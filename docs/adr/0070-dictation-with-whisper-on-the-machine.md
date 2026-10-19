# 0070. Dictation with Whisper on the machine, the transcriber built first

Date: 2026-10-19

## Status

Accepted. The plan comes from the user's notes (Projects/Patina dictation); they chose to start
with the transcriber.

## Context

The desktop should take speech as text in any field, like Handy: hold a key, speak, and the
words are typed where the focus is, all transcribed locally. It needs four parts: the key held
and released (the compositor's keys, ADR-0069, only fire on press), text delivered to the
focused client (text-input-v3, else the clipboard and Ctrl+V as `cmd.rs` already sends keys),
an overlay and settings, and the transcriber itself: microphone, voice detection and model.

The compositor's parts are familiar Smithay work. The unknowns are all in the transcriber:
whether whisper.cpp builds on both systems, whether it's fast enough on a laptop's CPU and in the
FreeBSD VM, how `/dev/dsp` records, and how silence is told from speech.

Considered and ruled out:

- **Running Handy** behind a `virtual-keyboard` protocol in the compositor. It's a WebKit app on an
  all-egui desktop, and its paste is still poor in terminals and X11 apps.
- **Parakeet and Silero voice detection.** Both need ONNX Runtime, whose prebuilt libraries (the
  `ort` crate) don't cover FreeBSD.
- **cpal on FreeBSD.** It records there through ALSA's port; the kernel's OSS (`/dev/dsp`) is
  always there whatever sound server runs, and shares the device by its virtual channels.

## Decision

- **Whisper everywhere, through whisper.cpp** (the `whisper-rs` crate), in a new crate,
  `dictate/` (patina-dictate). The CPU by default; `--features cuda` for an NVIDIA GPU (Linux
  only), `vulkan` where FreeBSD has a GPU driver.
- **The transcriber first, as a command:** it records until Enter or the end of its input and
  prints the text, or transcribes a WAV file. It's useful on its own (`patina-dictate | wl-copy`)
  and answers the unknowns before the compositor's parts are built. It becomes the daemon the
  compositor keeps running, as it does the tiler, once the compositor's side exists.
- **The microphone is cpal on Linux** (ALSA, which PipeWire and PulseAudio serve) **and
  `/dev/dsp` on FreeBSD**, read directly at 16 kHz mono where the kernel allows. Everything else
  is the same code on both.
- **Silence is told by Whisper itself:** segments it thinks likelier than 0.6 to be no speech
  are dropped, since on silence it otherwise writes "you" or "Thank you.". No separate voice
  detector until this proves not to be enough.
- **Models are whisper.cpp's GGML files**, named as it names them (`base.en`, `small`), in
  `$XDG_DATA_HOME/patina-dictate/models`; base.en is the default.

## Consequences

- On huginn, 11 s of speech takes base.en 1.1 s on the CPU and 0.2 s with CUDA; medium takes
  21 s on the CPU and 0.8 s with CUDA. So larger models want a GPU. The laptops' CPUs and the
  VM's are still to be measured.
- whisper.cpp is built from source with cmake for this crate, which lengthens a clean build and
  adds cmake and a C++ compiler to what the build needs; the CUDA build takes far longer.
- Models (75 MB to 1.5 GB) are downloaded by hand for now. Settings will have to manage them.
- The FreeBSD recording is untested until it's built in the VM.
- What comes next is the compositor's: a `com.sectorflabs.Patina.Input` service with
  `Commit(text)`, keys that act on release as well as press, and then the overlay and Settings.

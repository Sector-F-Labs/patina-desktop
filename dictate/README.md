# patina-dictate

Speech to text on the machine, with Whisper through whisper.cpp (ADR-0070). Dictation on the
desktop: hold Ctrl+Super+Alt, speak, let go, and what was said is typed into the focused window
(ADR-0072). As a command, it records from the microphone until Enter, or the end of its input,
and prints what was said; or it transcribes a WAV file.

## How it fits in

- **Run by the compositor** as `patina-dictate --daemon`, kept running like the tiler. The
  compositor's `dictate` key (`[keys]`, Ctrl+Super+Alt) signals `DictationStarted` and
  `DictationStopped` on `com.sectorflabs.Patina.Input`; this records between them and calls
  `Commit` there, which types the text into the focused window through the clipboard
  (ADR-0071).
- **On its own**, `patina-dictate | wl-copy` puts what you said on the clipboard.
- **Models** are whisper.cpp's GGML files in `$XDG_DATA_HOME/patina-dictate/models`, as
  `ggml-NAME.bin`; base.en is the default. Download them from
  `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-NAME.bin`.

## Code

`main.rs`, the arguments and the run; `daemon.rs`, `--daemon`: the signals, recording, and
transcribing on a thread of its own; `capture.rs`, the microphone: cpal on Linux, `/dev/dsp`
on FreeBSD; `audio.rs`, WAV files, mixing down and resampling to Whisper's 16 kHz, with their
tests; `transcribe.rs`, the model, and dropping what Whisper thinks isn't speech.

## Running

```sh
mkdir -p ~/.local/share/patina-dictate/models
curl -Lo ~/.local/share/patina-dictate/models/ggml-base.en.bin \
    https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
cargo run --release -p patina-dictate            # speak, then Enter
cargo run --release -p patina-dictate -- -v talk.wav
cargo run --release -p patina-dictate --features cuda -- -m medium
```

whisper.cpp is built with cmake, so the build needs cmake, a C++ compiler and libclang; and on
Linux ALSA's headers (`libasound2-dev`) for cpal. `--features cuda` needs the CUDA toolkit.

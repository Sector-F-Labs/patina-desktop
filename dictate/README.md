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
  (ADR-0071). It ends when that name leaves the bus: with no Wayland connection, nothing else
  would end it, and a restarted desktop would have two.
- **On its own**, `patina-dictate | wl-copy` puts what you said on the clipboard.
- **Models** are whisper.cpp's GGML files in `$XDG_DATA_HOME/patina-dictate/models`, as
  `ggml-NAME.bin`; base.en is the default. Download them from
  `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-NAME.bin`.

## Code

`main.rs`, the arguments and the run; `daemon.rs`, `--daemon`: the signals, recording,
transcribing on a thread of its own, and the overlay started and fed levels; `overlay.rs`,
`--overlay` (ADR-0073), the waveform pill on patina-shell, read from stdin; `config.rs`,
`overlay` in `~/.config/patina-dictate/config.toml`; `capture.rs`, the microphone: cpal on Linux, `/dev/dsp`
on FreeBSD; `audio.rs`, WAV files, mixing down and resampling to Whisper's 16 kHz, with their
tests; `transcribe.rs`, the model, and dropping what Whisper thinks isn't speech.

## Running

```sh
mkdir -p ~/.local/share/patina-dictate/models
curl -Lo ~/.local/share/patina-dictate/models/ggml-base.en.bin \
    https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
cargo run --release -p patina-dictate            # speak, then Enter
cargo run --release -p patina-dictate -- -v talk.wav
cargo run --release -p patina-dictate -- -v -m small.en    # -v says which GPU, if any
# the overlay alone, with made-up levels (on a layer-shell compositor)
(for i in $(seq 60); do echo 0.0$((RANDOM % 9)); sleep 0.03; done; echo transcribing; sleep 1) \
    | cargo run --release -p patina-dictate -- --overlay bottom
# drawn in software as PNGs, to look at
PATINA_DICTATE_PREVIEW=/tmp cargo test -p patina-dictate preview -- --ignored
```

whisper.cpp is built with cmake, so the build needs cmake, a C++ compiler and libclang; and on
Linux ALSA's headers (`libasound2-dev`) for cpal. It runs on the GPU through Vulkan by default
(ADR-0074), so it needs `glslc` and the Vulkan headers too; without a Vulkan driver it runs on
the CPU, as fast as a build without Vulkan. `--features cuda` adds CUDA, with the CUDA toolkit.

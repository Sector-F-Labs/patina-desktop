# 0074. Whisper runs on the GPU through Vulkan by default

Date: 2026-10-20

## Status

Accepted. The user asked for dictation to be faster on nifl, and the measurements below chose
this. Supersedes the CPU default of [ADR-0070](0070-dictation-with-whisper-on-the-machine.md).

## Context

On nifl (i7-1185G7, Iris Xe, Ubuntu), dictation's transcription ran on the CPU, the build
deployed from huginn. Each option was measured on nifl in Balanced mode, base.en, with a 4 s
clip (dictation's usual length) and an 11 s one, the median of three runs:

| | 4 s | 11 s | Text |
|---|---|---|---|
| CPU (AVX2, as deployed) | 0.97 s | 1.66 s | right |
| CPU, encoder window cut to the sound | about a fifth of that | about half | "ask not" became "asked not" |
| CPU with AVX-512 | no faster | no faster | |
| Flash attention, CPU or GPU | slower | slower | |
| 4 threads instead of 8 | slower | slower | |
| **Vulkan on the Iris Xe** | **0.38 s** | **0.52 s** | right |
| Vulkan, small.en | 1.17 s | 1.41 s | better punctuation |
| Vulkan build, no Vulkan driver | 1.06 s | 1.66 s | as the CPU build |

Whisper's encoder always covers 30 s; cutting its window (`audio_ctx`) to the sound is the
biggest saving on the CPU, but it costs accuracy. AVX-512 gains nothing on Tiger Lake, which
lowers its clock for it. ggml's Vulkan backend skips software drivers (llvmpipe) on its own.

Considered and ruled out: OpenVINO and SYCL (Intel only, a toolkit of their own, not in
FreeBSD), and a build per machine's CPU.

## Decision

- **The `vulkan` feature is on by default.** whisper.cpp uses the first Vulkan GPU, Intel, AMD
  or NVIDIA, and the CPU where there's none, at a CPU build's speed.
- **CUDA stays a feature to add**, for when it's worth the toolkit.
- **No `audio_ctx` cut, no AVX-512 build, no flash attention**: none is worth what it costs once
  the GPU does the work.
- `-v` reports the device whisper.cpp chose.

## Consequences

- Dictation on nifl is about three times faster, with the same text: half a second for 11 s.
- The build needs `glslc` and the Vulkan headers (`make deps`), and takes longer: whisper.cpp's
  shaders are compiled into the binary, which grows from about 2 MB to 50 MB.
- small.en on the GPU costs what base.en did on the CPU; it's the model to offer when Settings
  chooses models.
- A machine with no GPU driver (the FreeBSD VM) loses nothing; cutting the encoder's window
  could still help there, at a cost in accuracy, if it's ever needed.

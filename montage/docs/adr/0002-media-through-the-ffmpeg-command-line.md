# 0002. Read, play and export media through the ffmpeg command-line tools

Date: 2026-10-26

## Status

Proposed. This design is mine; the user hasn't signed it off yet.

## Context

A video editor needs to read the files people have (phone MP4s, MOV, MKV, MP3, M4A, FLAC…),
show thumbnails and waveforms, play the edit with its sound in step, record a voiceover, and
write a finished movie. The options considered:

- **Linking FFmpeg's libraries** (`ffmpeg-next` / `ffmpeg-sys`): the most control and the
  fastest seeking, but it binds the build to the system's libav* versions and their headers,
  which aren't installed on huginn and differ by distro and on FreeBSD.
- **GStreamer** (`gstreamer-rs`): a full media framework with editing services (GES), but a large
  dependency whose plugins vary between systems, and no dev packages installed here either.
- **Pure-Rust decoders** (symphonia for sound, plus some H.264 decoder): no system
  dependencies, but no complete pure-Rust decoder exists for the video formats phones make,
  and no encoder either.
- **The `ffmpeg` and `ffprobe` programs**, run as subprocesses: installed nearly everywhere
  (huginn has them; FreeBSD has the package), they read every format and write H.264/AAC, and
  the Patina desktop already pipes frames to `ffmpeg` for screen recording.

## Decision

Montage drives the `ffmpeg` and `ffprobe` programs, and links no media libraries:

- **Probing** (`media.rs`): `ffprobe -print_format json` for duration, picture size and sound;
  `ffmpeg` for a thumbnail (one RGBA frame on stdout) and for the sound's loudness (mono 16-bit
  PCM at 4 kHz, reduced to 25 peaks a second). One worker thread does this, in order.
- **One filter graph** (`render::movie`) describes the movie from any time on: each video clip
  read with `-ss`/`-t`, scaled and letterboxed to the output size, padded and cut to its exact
  length, joined with `concat`; music and voiceover clips delayed with `adelay` and mixed with
  `amix` (not normalised, so the clips' own sound doesn't drop as tracks are added).
- **Export** runs that graph into an MP4 (libx264 CRF 20, AAC 192k, faststart), reading
  progress from `-progress pipe:1`.
- **Preview** runs the same graph twice from the playhead, at 640×360: once for the picture as
  raw RGBA frames, once for the sound as 48 kHz float stereo, which rodio plays. The sound is the
  clock, and frames are shown as it reaches them. Stopped, one frame of the clip under the
  playhead is decoded, the newest asked for winning.
- **Recording** a voiceover is `ffmpeg` reading the default microphone (`-f pulse` on Linux,
  `-f oss /dev/dsp` on FreeBSD) into FLAC, stopped with `q` on its stdin.
- rodio is built with only its `playback` feature: ffmpeg decodes everything.

## Consequences

- ffmpeg is a run-time dependency. Without it the app says so in its status bar, and can't
  read, play or export anything.
- What plays is what exports: both are the same graph, so a bug shows up in the preview.
- Starting playback costs one ffmpeg start-up and seek per clip from the playhead on, a fraction
  of a second on huginn. A project with very many clips after the playhead opens that many
  inputs at once.
- Scrubbing decodes a frame per move through a new process: fine for home movies, slower than
  an editor with a decoder in memory.
- The preview is 640×360, whatever the export size.
- Every path is an argument, never part of the filter graph, so file names need no escaping.
- Revisit if scrubbing or start-up latency becomes the complaint, or for effects a filter graph
  can't express: linking libav then becomes worth its build cost.

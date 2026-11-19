# Montage

A Patina (egui) home video editor, like iMovie or Windows Movie Maker: a library of clips, a
timeline with a Video sequence and Music and Voiceover tracks under it, voiceover recording, and
MP4 export. ffmpeg does all the media work as subprocesses (ADR-0002); nothing links libav.

Significant decisions are ADRs in `docs/adr/`; read them before changing the media pipeline,
the timeline's model or the project format. Use the `adr` skill to add one.

## Commands

```bash
make check         # tests + clippy -D warnings (render/preview tests run ffmpeg; skipped without it)
make demo          # scripts/demo.sh makes test-pattern clips and target/demo/Demo.montage, then opens it
make screenshot    # docs/screenshot*.png from the demo project
make install       # per-user install; test with PREFIX=<scratch dir>
make app           # target/Montage.app for the Patina desktop's ~/Apps
```

Never run test instances against the user's config: the Makefile's `SCRATCH_ENV` points XDG
config and data at `target/scratch`. Montage reopens the last project at start, so a run against
the real config opens (and on quit may prompt to save) the user's project.

Patina comes from its `develop` branch (see `../AGENTS.md`): what Montage needs from Patina is
changed there, then `cargo update -p patina-ui`.

## Layout

- `src/project.rs`: the model (`Project`, `Media`, `Clip`, `Placed`, `Track`, `Sel`) and every
  edit as a pure function: drop, move, trim, split, remove. Saved as JSON (ADR-0004).
- `src/render.rs`: `movie()`, the one ffmpeg filter graph for the timeline from a time on, used
  by both export and preview; `still()` for one frame; `Export`, ffmpeg writing an MP4 on a thread.
- `src/preview.rs`: playback. Two ffmpeg pipes (RGBA frames, f32 stereo); the sound, through
  rodio, is the clock, falling back to the wall clock with no output. Stills on a worker where
  the newest ask wins.
- `src/media.rs`: the reader thread: ffprobe JSON, thumbnails, loudness peaks for waveforms.
- `src/record.rs`: voiceover recording, ffmpeg from the microphone into FLAC, stopped with `q`.
- `src/timeline.rs`: painting and pointer handling; returns `Edit`s for the app. Trims and moves
  of placed clips change the project live after an `Edit::Snapshot`. Its tests drive it with
  real pointer events through `Harness`.
- `src/library.rs`: the library tiles; drags carry a `MediaId` through egui's `DragAndDrop`.
- `src/app.rs`: window, menus, shortcuts, viewer and transport, dialogs, undo, the status bar,
  screenshot mode (`MONTAGE_SCREENSHOT`, `MONTAGE_SKIN`).
- `src/testing.rs`: (tests only) a tiny project of ffmpeg test-source files.

## Gotchas

- Each clip's picture is `tpad`ded and `trim`med, and its sound `apad`ded and `atrim`med, to the
  clip's exact length. Without that, `fps` conversion loses a frame here and there and the
  picture drifts from the sound over many clips. `ffmpeg_runs_the_graph` counts the frames.
- `amix` has `normalize=0`: otherwise adding music quietens the video's own sound.
- Library drags: drop on the frame `pointer.any_released()`, not on "button not down", or a
  hover with no button held drops at once.
- Drags measure from `press_origin()`, not the position when egui calls it a drag, which is a
  few pixels later.
- File dialogs (rfd) run on a thread and come back as `Picked`, so the UI never blocks.

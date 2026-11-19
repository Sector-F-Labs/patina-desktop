# 0003. Every episode is downloaded, and plays while it downloads

Date: 2026-10-23

## Status

Proposed

## Context

Episodes are MP3 (sometimes M4A) files of 20–100 MB on CDNs. Some, such as Acast's, have ads
inserted when served, so two downloads of the same episode can differ in length. rodio pulls
samples from its source on the audio callback thread, and its decoders want a `Read + Seek`
source.

Options considered:

- **Download fully, then play.** Simple, and seeking is exact, but a long episode on a slow
  connection takes a minute before anything plays.
- **Stream over HTTP with range requests.** Nothing kept, but seeking means new requests, and an
  ad-inserting CDN may serve a different file each time, so positions drift.
- **Download to disk and play the file while it grows.** Starts in about a second, keeps a file
  for offline listening, and every position refers to the one file that was downloaded.

## Decision

Every episode played is downloaded to `~/.local/share/podcasts/episodes/` (as `<name>.part`, then
renamed once whole), and plays from the partial file:

- A `GrowingFile` reads the download through positional reads, waiting for bytes that haven't
  arrived. Seeking to the end uses Content-Length.
- The decoder runs on a thread of its own, sending chunks of samples through a bounded channel to
  the source rodio plays. Only that thread ever waits on the network. The audio callback plays
  silence when it runs dry and shows "Buffering".
- Seeks go to the decoding thread. Each seek starts a new generation, and samples from before it,
  including what's left of the chunk being played, are dropped.
- MP3s seek coarsely (by byte estimate from the length), because Symphonia's accurate MP3 seek
  parses every frame from the start, which would wait for the whole download.
- Where you are in each episode is saved every few seconds, on pause and on exit, in
  `~/.local/share/podcasts/progress.json`. Within 30 seconds of the end counts as played.

## Consequences

- Playback starts in about a second; seeking ahead of the download waits for it to get there.
- Downloads use disk until deleted (right-click an episode). There's no automatic clean-up yet.
- A coarse seek can land a fraction of a second off, and can play up to one packet from before it.
- M4A files whose index is at the end wait for the whole download before they start.
- `GrowingFile` uses Unix positional reads (`FileExt::read_at`), which suits Linux and FreeBSD.

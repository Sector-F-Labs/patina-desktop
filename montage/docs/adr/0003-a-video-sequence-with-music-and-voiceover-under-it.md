# 0003. A video sequence, with music and voiceover clips placed under it

Date: 2026-10-26

## Status

Proposed. The three tracks are what the user asked for; how they behave is my design.

## Context

The user asked for a library of clips, a timeline they're dragged onto, and an audio track and
a voiceover layered under the video, as in iMovie or Windows Movie Maker. Those editors keep the
picture as one unbroken story: clips play one after another with no gaps, and sound is laid
under them. Pro editors offer free tracks where anything goes anywhere, with gaps and overlaps,
which home editing doesn't need and which make simple jobs (put this clip before that one)
harder.

## Decision

- **Video** is a sequence (`Project::video`): clips play back to back, a clip dropped between
  two goes between them, and dragging one moves it to another place in the order. The movie
  lasts as long as this track.
- **Music** and **Voiceover** hold clips placed at a time (`Placed { at, clip }`), dragged
  freely, snapping to the cuts, the movie's start and end, and the playhead. Sound past the
  movie's end is cut off.
- Every clip is a stretch of one file (`from`, `length`) with a volume (mute, quieter, normal or
  louder from its right-click menu). Clips are trimmed by their edges; a music or voiceover
  clip's start edge moves its start with it, so its sound stays where it was.
- A video clip plays its own sound under the music and voiceover; a clip without sound is
  silence.
- Split at the playhead (Ctrl+B) and Delete work on the selected clip, or on the video clip
  under the playhead.
- Undo keeps whole copies of the project (up to 200), which stays small: it's references to
  files and some numbers.

## Consequences

- No gaps or overlaps on the video track, and so no transitions yet: a later fade or crossfade
  goes between neighbouring clips.
- One music and one voiceover track; two songs overlap only if placed so.
- No ducking yet: music doesn't quieten under the voiceover by itself; its volume is set per
  clip.
- No titles, still photos or effects yet.

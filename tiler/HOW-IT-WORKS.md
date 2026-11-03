# How tiling fits into the desktop

Drag a window to the left edge and it fills the left half; press Super+Up and it fills the
screen. This page explains how that is split between the compositor and a small program of its
own, and why the split is worth having. The README next to it covers running and testing; the
decisions are in ADR-0049, ADR-0053, ADR-0054 and ADR-0065.

## The idea in one paragraph

Tiling is two jobs with different needs. *Mechanism* (seeing the pointer, resizing a window,
drawing a preview) has to live in the compositor, because only it owns those. *Policy* (which
edge means which half, what Super+Down does to a half, where a window opens) is the part that
gets changed all the time, and it doesn't need any of that. So the compositor reports what
happened on the session bus and places windows where it's told, and `patina-tiler` decides. The
policy can be rebuilt and restarted without closing a single window, or replaced by another
program entirely.

## The picture

<img src="images/architecture.svg" alt="The compositor, on the left, sends drags, Super+arrows, resizes, windows opening and closing and work-area changes as signals on the session bus, and answers Windows, Place, Move, Restore and ShowPreview calls. patina-tiler, on the right, decides with pure functions in policy.rs and keeps what it tiled and where apps' windows were. Below, the owner of the Tiler bus name decides; patina-tiler queues for it and another tiler can take it over.">

The only thing the two sides share is the bus name `com.sectorflabs.Patina.Tiling`: signals one
way, method calls back.

## The sequence, in time

<img src="images/sequence.svg" alt="One drag to the left edge: the compositor reports DragStarted and DragMotion, patina-tiler sees the pointer in the left zone and calls ShowPreview, and when the window is dropped the compositor reports DragEnded and the tiler calls Place with the left half.">

### Why the tiler checks twice

`DragStarted` is not taken on trust. Before it follows a drag, the tiler asks whether it still
owns the Tiler name (`tiling::deciding`) and whether the compositor lists the window at all,
since X11 windows can't be placed yet. Both checks happen again on a Super+arrow and on a
window opening. That is what lets another tiler take over mid-session without the two
fighting: the one that doesn't own the name does nothing.

## What decides what

| Event from the compositor | What patina-tiler does |
|---------------------------|------------------------|
| `DragMotion` near an edge (within 4 px) | `zone()`: left or right edge is that half, top fills; sides win in the corners. Shows or hides the preview as the zone changes. |
| `DragEnded` in a zone | `Place` the window in it. Dropped anywhere else, it saves where the window floated. |
| `SnapRequested` (Super+arrow) | `after_snap()`: Up fills, Down restores, Left and Right tile to that half or restore from the other one. |
| `Resized` (the line between tiled windows was dragged) | `split_of()` keeps the new split, so a window tiled to the other side takes the rest. |
| `WindowOpened` | `opening()`: where its app's window was last, if none of its app's is open; fills if it's too big for the work area or closed filled (ADR-0065). |
| `WindowClosed` | Saves where it floated, and whether it was filled, for the app's next window. |
| `WorkAreaChanged` | Tiles its halves again, and forgets windows since closed, maximised or placed by another tiler. |

The rectangles come from the compositor's `WorkArea()` (inside the outer gaps, ADR-0051) and
`Gap()`, so a bar or a gap setting changes what a half is without the tiler knowing why.

## What the compositor keeps

| Job | Why it isn't the tiler's |
|-----|--------------------------|
| Drag the line between two tiled windows (`split.rs`, ADR-0054) | It needs the pointer's cursor, a pointer grab and the windows following every motion; over the bus that is a round trip in each, and a client can't grab the pointer. The tiler is told with `Resized` afterwards. |
| Draw the preview | It is drawn over the desktop with the same renderer as everything else; the tiler only says where. |
| Resize a window | `Place` becomes an xdg configure, and the tiled states come from which sides of the window touch the work area, which is right for halves, thirds and quarters alike. |
| Restore a placed window when it's dragged | Immediate, and in the pointer's hands; the tiler is told with `DragStarted` and forgets it. |
| Keep filled and maximised windows fitting the work area | A work-area change shouldn't wait for a round trip, and still works with no tiler. |
| Number the windows | Each window gets an id that is stable while it's open; the first time windows are named on the bus. |
| Coalesce drag motions | When the bus falls behind, only the latest position goes out. |

## Who decides

The owner of `com.sectorflabs.Patina.Tiler` decides. `patina-tiler` holds the name with
replacement allowed and queues for it (`claim_default`); a tiler that `claim`s it takes over,
and `patina-tiler` decides again when that one leaves. A newer `patina-tiler` also takes over
from an older one still running. While nothing owns the name, the compositor still reports
every drag and arrow but nothing tiles.

`cargo run -p patina-system --example thirds` is a whole alternative tiler in about a hundred
lines: the edges give thirds, and Super+Left/Right step along them. It is the quickest way to
see where the seam is.

## When it's not running

- Windows don't tile. Drags still move them and maximising still works, since that is the
  compositor's.
- The compositor starts it with the bar and the wallpaper and starts it again when it exits.
  Settings' Processes page restarts it, which is how a newly installed build is picked up.
- It exits when the compositor it started with leaves the bus, as nothing else would end it.
- Without a session bus there is no tiling at all.

## The files

```
 patina-desktop/
 ├── compositor/src/
 │   ├── tiling.rs        the mechanism: places windows, shows the preview, forwards events
 │   ├── tiling/dbus.rs   com.sectorflabs.Patina.Tiling: signals, Windows(), Place(), …
 │   ├── split.rs         dragging the line between tiled windows
 │   └── maximize.rs      placements, and keeping filled windows fitting
 ├── system/src/tiling.rs the client side: Tiling, Event, claim, claim_default, deciding
 └── tiler/src/
     ├── main.rs          the bus: events in, Place/Move/Restore/preview out
     ├── policy.rs        what tiles where, as pure functions with their tests
     └── positions.rs     where apps' windows were, in ~/.local/state/patina-tiler/windows.toml
```

The diagrams in `images/` are hand-drawn SVG, and follow the viewer's light or dark theme.

## Limits, as they stand

- X11 windows aren't listed and can't be placed; tiling goes through xdg-toplevel configures.
- Halves and fill only, by default; anything else is another tiler.
- The preview is drawn over the dragged window, not under it.
- A window the app maximises, or another tiler places, is forgotten on the next work-area
  change rather than tiled again.

# 0053. Tiling is decided by patina-tiler, a process the compositor keeps running

Date: 2026-10-17

## Status

Accepted. The user said this was the idea all along: tiling is its own process, worked on and
restarted like the others, and expected always to be there. It supersedes the part of
[ADR-0049](0049-tiling-by-dragging-decided-over-d-bus.md) that kept a built-in policy in the
compositor.

## Context

ADR-0049 put tiling on D-Bus, so any app could decide. But the compositor kept its own policy
as the default (the drop zones, the halves, what Super+arrows do), with an outside tiler only
replacing it. That ADR rejected a policy-only-outside design for two reasons: a tiler that
crashed or was slow to start would leave no tiling, and every drag would need a round trip before
the preview showed.

That left the everyday tiling code in the compositor. Changing it meant rebuilding and restarting
the compositor, which closes every window. Iterating on it was the slowest of anything on the
desktop.

The other pieces already solve those concerns:

- **Crashes:** the compositor starts and supervises the bar and the wallpaper, restarting them
  when they exit (ADR-0045).
- **Restarting:** Settings' Processes page restarts any of them.
- **Latency:** a bus round trip is well under a frame, and drag motions are already coalesced.

Options considered:

- **Keep the built-in policy, with patina-tiler on top.** Tiling would survive the tiler being
  gone, but the same rules would live in two places, and the built-in copy would drift
  (ADR-0027).
- **Only patina-tiler decides.** The compositor keeps the mechanism and none of the policy.

## Decision

- **patina-tiler (`tiler/`) decides tiling**, Ubuntu's way, as the compositor did. The edges tile
  to halves, the top fills (ADR-0052), and Super+Left/Right/Up/Down tile, fill and restore. The
  gaps (ADR-0051) come from `WorkArea()` and `Gap()`. When the work area changes, the halves it
  tiled are tiled again. `policy.rs` holds the rules as pure functions with tests. `main.rs`
  talks to the bus.
- **The compositor starts it** with the wallpaper and the bar (`keep_client`). It's restarted on
  exit, it's listed in Settings' Processes page, and it's installed by `make install`.
- **The compositor holds no tiling policy.** It reports every drag and Super+arrow, and places
  windows and shows the preview where it's told. It also restores a placed window when it's
  dragged, keeps filled and maximised windows fitting the work area, and maximises at the app's
  request. Its zones, halves and snap rules are gone, along with `Placement::Left` and `Right`.
  A tiler's halves are `Rect`s.
- **A new `Screen()`** on the bus gives the whole screen, so a tiler knows where the pointer meets
  its edges.
- **Replacing the tiler still works.** patina-tiler holds the Tiler name with replacement allowed
  and queues for it (`claim_default`). A tiler that `claim`s it takes over, and patina-tiler
  decides again when that one leaves. Before acting, it checks it owns the name (`deciding`).

## Consequences

- Tiling changes are a `make install` and a restart of one process in Settings, with no windows
  closed.
- While patina-tiler is down (crashed and restarting, or no session bus), windows don't tile.
  Drags still move them, and maximising still works.
- Super+arrows and the preview now cross the bus. A drag start costs two calls: the owner check,
  and the window list to skip X11 windows.
- The tiler tracks what it tiled. A window the app maximises, or another tiler places, is
  forgotten on the next work-area change rather than re-tiled.
- Without a session bus there's no tiling at all, where before the compositor's own policy
  still worked.

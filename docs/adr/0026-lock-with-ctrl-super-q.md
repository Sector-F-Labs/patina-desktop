# 0026. Lock with Ctrl+Super+Q, the Mac shortcut; Super+L tiles right again

Date: 2026-10-15

## Status

Accepted

## Context

ADR-0025 made Super+L lock the screen, as on Ubuntu, which took L away from the vim tiling keys
(ADR-0023): H, J and K still tiled, but right was the arrow only. The user found the lock getting in
the way of positioning windows, and asked for the Mac's shortcut instead.

## Decision

- **Ctrl+Super+Q** locks, as Ctrl+Cmd+Q does on a Mac (Super sits where Cmd does). Super+Escape
  still locks too.
- **Super+L** tiles right again, so the vim keys are whole: H, J, K and L, and with Shift, growing.
- Super+Q (no Ctrl) still quits the focused app, as Cmd+Q does on a Mac.

## Consequences

- Locking needs three keys, as on a Mac, so it's hard to hit by accident while moving windows.
- Muscle memory from Ubuntu (Super+L) now tiles instead of locking.

# 0045. The compositor keeps the desktop's processes, and Settings shows them

Date: 2026-10-17

## Status

Accepted

## Context

Trying a change to the bar, the wallpaper or a menu on the laptop meant logging out to GDM and
back in. The compositor starts the bar and wallpaper once (`clients::spawn`) and forgets them: a
thread reaps each one, nothing restarts it, and everything they print goes into the compositor's
stderr, mixed together. Starting a new bar by hand from a terminal needs `WAYLAND_DISPLAY`, `PATH`
and `DISPLAY` set as the compositor sets them.

Two problems were separated:

- **The pieces** (bar, wallpaper, menus, launcher, lock screen) are Wayland clients. Any of them can
  be stopped and started again without touching the session, so long as something starts them
  the way the compositor does.
- **The compositor itself** can't be restarted from inside. When it exits, every client loses its
  connection, and GDM ends the session because the compositor is the process its `Exec=` names. A
  small `patina-session` above it, running it again after a crash or when asked and leaving for GDM
  on logout, keeps the login but not the open windows. Keeping windows across a restart (KWin's
  wrapper holds the socket and Qt 6 apps reconnect) needs toolkits that support it, which GTK
  doesn't. Both are left for later.

For the pieces, the compositor is already their parent, so it's where the restarting goes, much
as MINIX 3's reincarnation server is the parent of the drivers it restarts. The user asked for a
way to watch and steer this by hand: a small task manager for Patina's own processes, in Settings,
with each process's log a click away. Settings already talks to the compositor over the session
bus for Night Light (ADR-0039), so the same path was chosen, rather than a socket of its own or a
separate app (ADR-0027: a feature grows where it already lives).

## Decision

- **The compositor keeps a list of what it starts** (`compositor/src/processes.rs`): each
  process's name, command line, PID, state (running, exited with its status, stopped, given up),
  restarts and start time, and its last 500 lines of output. It reads each child's stdout and
  stderr itself and copies every line to its own stderr, prefixed with the process's name, so the
  session's log still has everything.
- **Each child gets a process group of its own.** Commands run through `sh -c`, so stopping only
  the shell could leave the program running; stopping signals the whole group (SIGTERM).
- **Kept pieces come back.** The wallpaper and the bar are *kept*: when one exits, for any reason
  other than being stopped from Settings, it's started again a second later. If it exits five
  times within thirty seconds, the compositor gives up and says so, rather than looping. Everything
  else it starts (the launcher, the desktop's menu, the volume flash, the lock screen, commands
  bound to keys) runs once. The twenty most recent of these stay in the list after they exit, with
  their logs, so a crash can still be read.
- **On the session bus:** `com.sectorflabs.Patina.Processes` at `/com/sectorflabs/Patina/Processes`,
  with `List()`, `Restart(id)`, `Stop(id)`, `Log(id, from)` and `Spawn(command, env)`. `Log` returns the lines from
  sequence number `from` onwards and the number to ask from next, so a reader polls for only what's
  new. There are no signals: Settings polls, as it already does for the bar's config and for
  power modes. Unlike Night Light, it's served nested too (in the VM, or `make run`), since that's
  where the pieces are worked on most. If the name is taken (another Patina is running), it says
  so and the compositor runs on without it. The client side is `patina_system::processes`.
- **The bar and the desktop's menu start what they open through `Spawn`** (added the same day):
  the tray's Wi-Fi and volume menus, the launcher from the flask button, and Settings. The
  compositor runs the command once, as one of its own clients, with the caller's variables (the
  bar's `PATINA_SKIN` and `PATINA_HUD_EDGE`) besides its own; a caller can't set
  `WAYLAND_DISPLAY`, `DISPLAY`, `PATH` or `XDG_*`. When the call fails (another compositor, or no
  session bus) the caller starts the command itself, as before, so the tray never stops working
  over D-Bus; only the listing is lost. The bar calls on a thread, so the bus never holds it up.
- **Settings has a Processes page:** a table of the processes with their state, PID, uptime and
  restarts, a Restart button and a Stop button on each row, and a log window that opens on a click
  and follows new lines. Settings' own Restart is greyed out, since restarting it would close the
  window it was clicked in.

## Consequences

- Changing the bar or wallpaper is: build, then Restart in Settings, or kill it; it comes back from
  the binary now on disk. No logout.
- Only what the compositor starts is listed. With `Spawn`, that's everything the desktop's own
  pieces open; apps started by patina-launcher are its children and aren't listed, and nor is
  Xwayland, which Smithay starts.
- `Spawn` runs any command line for whoever asks on the session bus. That's no more than any
  process of the user's can already do, but it means a piece's menus now depend on the bus being
  there, which is why the fallback is kept.
- Output from the pieces now passes through the compositor, line by line. A child that writes
  without newlines is held until it does, and a very chatty one costs the compositor a thread's
  reading.
- Anyone on the session bus can stop, restart or start the desktop's pieces. That's the user's own
  session, as with Night Light.
- Restarting the compositor itself, and `patina-session`, are still to come; the page gets a
  "Restart desktop" button then.

# 0134. Game mode is a small program that quits programs and runs commands while a fullscreen window has the keyboard

Date: 2026-11-11

## Status

Accepted. The user suggested it and agreed the shape below. The compositor changes in two small
ways, neither of them game mode's logic: a default key binding, and drawing the Overlay layer above
a fullscreen window, which the layer-shell protocol asks for and which the notice needs to be seen.

## Context

The compositor already tells the bus when a fullscreen window has the keyboard: Night Light's
`com.sectorflabs.Patina.NightLight` interface lists "A fullscreen window" in its `Inhibitors`
property (ADR-0132). The same moment is when background work costs the most: a rust-analyzer
indexing a workspace or a set of Docker containers competes with a game for CPU, memory and the
disk. The user wants to say, in Settings, which processes to deal with and which commands to run
when a game starts, and have everything put back when it ends.

A first attempt put the whole thing in the compositor (process scanning, signals, Docker, commands
and a key). It was dropped: none of it needs the compositor, which already says everything it has
to, and a hung command or a signalled process should not be able to stall the one process the
session depends on. A second attempt copied dictation's structure (a daemon crate with an overlay
and app lists) when only the idea of a bus message and a key binding was meant; that too was more
than was asked for. The examples in the codebase show that a thing can be done, not that it
should be built the same way.

## Decision

We will add `patina-game-mode`, a small program with a config of its own at
`~/.config/patina-game-mode/config.toml`, and nothing else. It does only three things: wait, quit
programs, run commands. It does not pause or restore anything.

- **It watches the bus.** It reads Night Light's `Inhibitors` each second and treats the entry
  "A fullscreen window" as the game being up. It starts after the message has held for a short
  delay and ends after it has been gone for the same delay (three seconds by default), so switching
  away and back does not run everything over and over.
- **On entering** it asks the user's own processes named in a *kill* list to quit (`SIGTERM`) and
  runs the *enter* commands. **On leaving** it runs the *exit* commands. Stopping Docker is an
  enter command (`docker stop $(docker ps -q)`), not an option of its own.
- **Commands** run through sh in the user's session, one at a time, each stopped after thirty
  seconds, off the thread that watches.
- **State** is one empty marker file in `XDG_RUNTIME_DIR` while game mode is on, so Settings can
  say so. There is no record of what was done, since nothing is undone but by the user's commands.
  A marker left by an earlier run is removed at start.
- **By hand**: `patina-game-mode --toggle` asks the running program to switch game mode on or off,
  whatever is on screen. The compositor's default bindings include
  `"Ctrl+Super+G" = "patina-game-mode --toggle"`, a row in its table and no logic in it. A
  `[bindings]` section replaces the defaults, so a user with their own has to add the line, which
  the Settings page says.
- **Started by the compositor**, with `keep_client("patina-game-mode --daemon")` beside dictation's,
  so it is restarted if it dies.
- **Settings drives it.** A "Game mode" page edits the program's config file (as the Dictation page
  edits dictation's, keeping the rest of the file): whether it is on, the delay, the kill list and
  the enter and exit commands, with help text that says how game mode is detected. The page shows
  whether game mode is on, with a button to switch it. Every option is in the UI; the file stays
  plain TOML.
- **A notice** at the top centre says "Game mode on" and "Game mode off", every time and with no
  setting: with no feedback there is no telling whether it worked. It is a pill in the look of
  dictation's overlay, from a `toast` module in `patina-shell` that other pieces can use. The
  daemon runs itself again as `patina-game-mode --toast TEXT` for it, so a surface that goes wrong
  can't take the watching with it.
- **Left out until asked for:** pausing processes or Docker and putting them back, lists of apps
  that do or do not start game mode, and a shortcut row in Settings.
  A generic "when this happens, run that" program may replace this one if a second use appears;
  with no state to recover, this one would carry over to it almost unchanged.

## Consequences

- The compositor is untouched, and so are its risks.
- **It needs Night Light's `pause_in_fullscreen` on**, because the message only appears then. In
  Settings that checkbox is greyed out while Night Light is off, so game mode needs Night Light on
  too, or that page changes. The game mode page should say so when it is off.
- The message carries no app id, so a fullscreen film triggers game mode like a game does; only the
  manual toggle and turning the automatic start off are the answers for now.
- Night Light is only served on the hardware, so game mode cannot be tried in a nested session.
- The program matches the text "A fullscreen window"; a rewording in the compositor breaks it
  silently, and a test should hold the string.
- Killing frees memory at the price of losing what the program hadn't saved. Nothing is restarted;
  an exit command can do that.
- If the program dies between entering and leaving, the exit commands do not run.
- Names are matched against the user's own processes, and the kernel keeps only fifteen characters
  of a name, so `node` matches every Node program.
- Commands run arbitrary shell, so a bad one can hang or do harm.
- We would revisit this if the message proves too narrow, in which case the compositor sending the
  fullscreen window's app id on the bus is the next thing to try.

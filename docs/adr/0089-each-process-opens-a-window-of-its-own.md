# 0089. Each process opens a window of its own, with its graphs and details

Date: 2026-10-24

## Status

Proposed

## Context

The process manager (ADR-0088) is for when something has gone wrong, and the list says only how
much each process uses. Debugging one needs more: what it's doing and waiting in, where its
memory goes, its threads, what it has open and who it talks to. The user asked to drill down
into a process.

A details pane beside the list was considered first. The user chose windows instead: egui opens
real windows (viewports), Patina is built on egui, and a window per process lets several be open
at once and set side by side to compare. Files' Quick Look (its ADR-0008) already opens a
viewport in Patina's frame, so the pattern exists.

Most of what's wanted is in Linux's `/proc/<pid>`, readable for the user's own processes:
`wchan` (the kernel function it's sleeping in, often the answer for a hung process), `status`,
`smaps_rollup`, each thread's `stat`, the `fd` links, with `net/tcp` and its kin to resolve
sockets to connections, and `environ`. Another user's process shows less; the kernel's stack
(`/proc/<pid>/stack`) and attaching a debugger need root, so they're left out.

## Decision

- **A click on a process in the Overview or Performance, or a double click, Enter or Details…
  in Processes, opens its own window**: an immediate viewport in Patina's frame, app id
  `com.sectorflabs.ProcessManager.Process` (apart from the main window's, so Ctrl+Shift+Escape
  still brings that one forward). Opening one already open brings it forward. A click in
  Processes still only chooses the row, for the keyboard and the menus.
- **The window** (`window.rs`): End and Kill (and Switch To, for an app) on the title bar's
  right; its state and what it's waiting in, warned of when it's waiting on a device; command,
  program, folder, control group, CPU time, context switches, priority, out-of-memory score
  and open files against the limit; its own CPU, memory, disk and GPU graphs since the window
  opened, CPU and GPU on a scale that zooms in on a small share; its memory as a bar (its own,
  files, shared, swapped); an app's processes, each a click from its own window; then threads
  with each one's CPU, network connections, open files and environment, folded.
- **The reader thread reads the details** (`details.rs`) of the processes whose windows are
  open, at the same speed as everything else (`Ask::Watch`), so the windows never wait on
  `/proc`.

## Consequences

- Several processes can be compared side by side, each window tiled or floated by the
  compositor and listed in the bar's tabs.
- Linux only, beyond the graphs: FreeBSD has no `/proc` by default, and its windows show the
  graphs and the command line alone.
- The graphs start when the window opens; the process manager doesn't keep every process's
  history.
- Each window's details are read once a reading, so many open windows cost more; a few are
  cheap.
- Next: the kernel's log and pressure stall information for the whole system, and the
  compositor telling which windows don't answer its pings.

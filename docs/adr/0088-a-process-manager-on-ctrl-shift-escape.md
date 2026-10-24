# 0088. A process manager, patina-processes, on Ctrl+Shift+Escape, a key no config can take

Date: 2026-10-24

## Status

Proposed

## Context

The user asked for a process manager: a Patina app, egui used as it's meant to be, with graphs,
"good about task management and really useful", on a key that can't be rebound, like Alt+Tab
(ADR-0075). The desktop had only Settings' Processes page (ADR-0045), which lists the pieces the
compositor started, not the machine's processes, and has no idea what they cost.

A process manager is needed most when something has gone wrong: an app hung, a game fullscreen and
frozen, a config that took the keys. So its key has to be one no config can take, and it has to
work over a fullscreen window. Ctrl+Shift+Escape is the one Windows users already know, and
nothing in Patina used it.

ADR-0027 says a feature grows where it already lives, which argued for growing Settings'
Processes page instead. That page is about the desktop's own pieces and restarting them, and it
sits behind seven settings pages; a process manager wants a window of its own that opens fast, with
a key, and gets out of the way. So it's a new app, and Settings' page stays as it is.

Reading processes: `/proc` alone would be Linux only, and the test VM is FreeBSD. The `sysinfo`
crate reads processes, CPUs, memory, disks and network on both.

The GPU: the user asked for the stats nvtop shows. nvtop reads NVIDIA cards through NVML, the
library nvidia-smi uses, and the DRM drivers (amdgpu, i915, xe and others) through each
process's `/proc/<pid>/fdinfo` for its files on `/dev/dri`, which count the nanoseconds (or, on
xe, cycles) each engine was busy and the memory held, with AMD's sysfs for the card itself.
NVIDIA's driver publishes no such fdinfo. The user also asked that the CPU's and GPU's graphs
wear their maker's colour: NVIDIA green, AMD red, Intel blue.

Which processes are apps: wlr-foreign-toplevel gives app ids but no processes, and guessing from
process names is unreliable. The compositor knows each Wayland client's PID (its socket's
credentials) and each X window's (`_NET_WM_PID`).

## Decision

- **patina-processes** (`processes/`), the Process Manager, app id
  `com.sectorflabs.ProcessManager`: "process", the Unix word, not Windows' "task", as the user
  asked. A Patina app wearing the desktop's skin as Settings does, 1240 by 840 to begin with.
  Three views as Patina's tabs in the title bar, fixed, as Messenger's folders are:
  - **Overview,** where it opens: the machine in a line, a tile per resource with its graph,
    and the ten busiest on the CPU, the GPU and memory, with Processes' right-click menu, so
    something can be ended from there. The user found lists re-sorted every reading hard to
    follow, so they hold still: values averaged over a few readings, a row kept in place until
    an outsider clearly beats it (taking its row), and the list sorted again only when a new
    one clearly leads.
  - **Processes:** the apps first, each the process with windows plus everything it started
    (a browser's content processes), counted together and opened with an arrow; a process with
    windows that another app started is an app of its own. Then the background processes.
    Name, PID, user, CPU (a share of the whole machine), memory and disk columns, sortable, the
    numbers tinted by how much they are. A filter (Ctrl+F). End (SIGTERM, Delete), Kill
    (SIGKILL, Shift+Delete), Pause and Resume, Switch To (Enter), copy PID and command line,
    from a right click, the Process menu or the keyboard. Ending an app ends its own process,
    as closing it would; killing or pausing it signals all of its processes. Ending something
    the process manager itself runs under (the compositor, the session) is asked about first.
  - **Performance:** the CPU, memory, disks and each physical network interface down the side
    with a sparkline each; the one chosen drawn large over the last minute, with what it is and
    the five things using it most, each a click from its row in Processes.
- **GPUs as nvtop reads them** (`gpu.rs`): NVML through `nvml-wrapper`, loaded at run time, so
  nothing breaks without NVIDIA's driver; for each card, busy, memory, temperature, power, clocks,
  fan and video encoding and decoding, and each process's share and memory. The DRM drivers
  through fdinfo and sysfs on Linux; a card without a busy figure of its own (Intel's) is the sum
  of its clients'. Processes gets a GPU column, and Performance a page for each GPU.
- **The CPU's and GPU's graphs wear their maker's colour**, NVIDIA green, AMD red, Intel blue,
  chosen from the driver or the model's name, else the skin's accent; the only colours the task
  manager doesn't take from the skin.
- **One reader thread** (`sample.rs`) reads everything at the update speed (half a second to
  four seconds, or paused) and sends a snapshot; the window never waits on it.
- **The compositor serves the windows' processes** on its Tabs service: `Windows()` (app id,
  title, PID, in the bar's order) and `Activate(pid)`; `patina_system::windows` is the client.
  Without it (another desktop) everything is listed as a background process.
- **Ctrl+Shift+Escape is fixed** in `input::key_action`, beside Alt+Tab, and works over a
  fullscreen window. It brings the process manager's window forward if it's open, else starts it.

## Consequences

- A hung fullscreen game can be dealt with without a console: the key opens the process manager
  over it, and the window taking focus takes the game off the top.
- `sysinfo` and `nvml-wrapper` are new dependencies, for this app alone. Its per-process disk figures need
  permission to read other users' `/proc/<pid>/io`, so others' processes show 0 there.
- Two process managers for processes now: Settings' page for restarting the desktop's pieces and
  reading their logs, and this one for everything. If that overlap confuses, Settings' page
  could move here.
- fdinfo only shows processes the user may read, so an Intel GPU's busy figure leaves out
  other users' work; and the DRM path is tested against the kernel's documented format only,
  since this machine is NVIDIA. FreeBSD has no fdinfo: only NVML there.
- Not yet: "not responding" (the compositor could ping each window), ending a process
  owned by root (through doas or polkit), and per-process network use.

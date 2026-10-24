# patina-processes

The desktop's process manager (ADR-0088): a Patina app on Ctrl+Shift+Escape, a key no config can
take, which the compositor keeps beside Alt+Tab and lets through over a fullscreen game. Pressed
again, it brings the open window forward rather than starting another.

## On the desktop

Three views as Patina's tabs in the title bar (fixed: none opened or closed; Alt+1 to 3), and a
status bar with the totals and what an action did:

- **Overview** (`overview.rs`), where it opens: the machine in a line, a tile per resource with
  its graph (a click opens it in Performance), and the ten busiest on the CPU, the GPU and
  memory, with Processes' right-click menu. Those lists hold still (`ranking.rs`): averaged
  values, rows kept in place, swapped in place only when clearly beaten, re-sorted only for a
  clear new leader.

- **Processes** (`processes.rs`): the apps, each the process with windows and everything it
  started, counted together (`groups.rs`), then the background processes. Sortable columns
  (name, PID, user, CPU, memory, disk), the numbers tinted by how much they are; a filter;
  End, Kill, Pause, Resume and Switch To from a right click, the Process menu or the keyboard.
  Ending what the process manager runs under (the compositor, the session) asks first.
- **Performance** (`performance.rs`): CPU, memory, disk, each GPU and each physical network interface
  down the side with sparklines, the chosen one drawn large over the last minute
  (`graph.rs`), its details, and the five things using it most, a click from their rows.

## How it fits

- `sample.rs` reads the machine on a thread of its own at the update speed, through `sysinfo`
  (Linux and FreeBSD), and sends a snapshot; it also signals processes.
- `gpu.rs` reads the GPUs as nvtop does: NVML for NVIDIA (`nvml-wrapper`, loaded at run time),
  and `/proc/<pid>/fdinfo` with sysfs for the DRM drivers (amdgpu, i915, xe). The CPU's and GPU's
  graphs are drawn in their maker's colour (`performance.rs`, `cpu_colour`/`gpu_colour`).
- Which processes are apps comes from the compositor: `Windows()` and `Activate(pid)` on
  `com.sectorflabs.Patina.Tabs` (`compositor/src/tabs/dbus.rs`), through
  `patina_system::windows`. On another desktop, every process is listed as a background one.
- Apps' names and icons come from their `.desktop` entries and the icon theme (`icons.rs`), as
  the bar's tabs' do.
- It wears the desktop's skin, from the bar's config, as Settings does.

`PATINA_PROCESSES_PREVIEW=dir cargo test -p patina-processes preview -- --ignored` draws both views in
software from this machine's processes.

# 0090. The kernel's log and pressure say what's wrong, on the Overview and a Log tab

Date: 2026-10-24

## Status

Proposed

## Context

The process manager is for when something has gone wrong (ADR-0088), and the user asked for the
system's side of that: the kernel's messages, and how stuck the machine is. On the machine it
was built on, the kernel's log since start held four crashes of `drag-out` in
`libwayland-client`, three of `minecraft-launcher`, Firefox's content processes segfaulting and
nvidia-drm errors, none of which the desktop showed anywhere; and `/proc/pressure/io` said
programs had been waiting on the disks a tenth of the time.

Reading the log: `dmesg` is closed to users on Ubuntu (`kernel.dmesg_restrict=1`), as is
`/dev/kmsg`, but the journal's kernel lines (`journalctl -k`) are open to the `adm` and
`systemd-journal` groups, which the first user is in. FreeBSD's `dmesg` is open to users. So the
journal first, then `/dev/kmsg`, then `dmesg` asked every few seconds.

Pressure stall information (`/proc/pressure/{cpu,memory,io}`, Linux 4.20 on) is the share of
time programs couldn't run for want of the CPU, memory or the disks: unlike how busy the CPU is,
it says whether the machine is holding programs up.

## Decision

- **The kernel's log is read on a thread of its own** (`kernel_log.rs`): the journal's lines
  since start (3000 at most), then followed (`-f` after the last cursor); else `/dev/kmsg`,
  which blocks for each new line; else `dmesg` every five seconds. Each line is looked at for a
  crash (a segfault or trap, naming the program and PID), an out-of-memory kill, a hung task,
  and disk, GPU, heat and other errors.
- **A Log tab**, the fourth: newest at the bottom and followed, filtered by Errors, Warnings or
  Everything (lines with an event always shown), by kind of event, and by words. A line naming
  a process still running opens its window (ADR-0089) from a right click or a double click.
- **Pressure is read with everything else**, and Performance has a Pressure page: the three
  over the last minute, and the 10-second, minute and 5-minute averages, some programs and
  all of them waiting.
- **The Overview shows what looks wrong** (`problems.rs`), under its graphs: pressure over the last minute
  past a threshold (CPU 25%, memory 5%, disks 10%), each kind of event since start once, with
  how often and when last (a crash with the library it was in), and processes exited but not
  cleaned up. Errors first, then the newest. A click goes to the Log tab showing that kind and
  program, or to the Pressure page. With nothing, it says nothing looks wrong. The user asked
  that it be about what to solve now: what's going on, or last happened within the day, is
  listed; older issues are folded under a heading of their own.

## Consequences

- The thresholds are a first guess; a machine that's always a little stuck will keep warning.
- An old crash moves under Older issues after a day, but stays there until the machine
  restarts.
- The events are found by the kernel's wording, which changes between versions and drivers; a
  missed one is still in the log, an "other kernel error" or plain.
- Without the journal's groups, Ubuntu shows nothing but a note how to get them.
- Next: systemd's failed units, crash reports (`/var/crash`, coredumps), and the journal per
  process in its window.

//! The kernel's log since the machine started (ADR-0090), followed as it
//! grows, on a thread of its own: from the journal (`journalctl -k`, which
//! users in `adm` or `systemd-journal` may read), else `/dev/kmsg` (where
//! `kernel.dmesg_restrict` allows), else `dmesg(8)` asked every few seconds
//! (FreeBSD). Each line is looked at for what tends to matter when a
//! machine misbehaves: a program crashing, killed for memory or hung, and
//! errors from the disks, the GPU or the heat.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Lines read from the start: the journal can hold a great many.
const BACKLOG: usize = 3000;
/// How often `dmesg` is asked, without anything that follows.
const DMESG_POLL: Duration = Duration::from_secs(5);

/// A line of the kernel's log.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Seconds since the epoch; 0 where the source doesn't say.
    pub time: f64,
    /// Syslog's, 0 (emergency) to 7 (debug); 3 is an error, 4 a warning.
    pub priority: u8,
    pub message: String,
    pub event: Option<Event>,
}

/// What a line says happened, where it's one of the things looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A program crashed: a segfault or another trap.
    Crash {
        name: String,
        pid: u32,
    },
    /// The kernel ended a program to free memory.
    OutOfMemory {
        name: String,
        pid: u32,
    },
    /// A program waited on the kernel for minutes.
    Hung {
        name: String,
        pid: u32,
    },
    Disk,
    Gpu,
    Heat,
    /// Any other error.
    Error,
}

/// What a kind of event (`Event::label`) means, for the Log tab and the
/// Overview's problems.
pub fn meaning(label: &str) -> &'static str {
    match label {
        "Crash" => {
            "The program did something the processor refused, such as reading memory it \
             doesn't have (a segfault), and the kernel ended it. The library named is the code \
             it was running at the time, often where the fault is. A program that crashes \
             again and again usually has a bug, or a library or driver it doesn't get along with."
        }
        "Out of memory" => {
            "The machine ran out of memory and swap, and the kernel ended this program, the one \
             it judged would free the most, to keep everything else running."
        }
        "Hung" => {
            "The program waited on the kernel for minutes, usually for a disk, a network share \
             or a driver that stopped answering. It can't be ended until the wait is over."
        }
        "Disk" => {
            "The kernel couldn't read or write a disk, or a filesystem found itself damaged. \
             Worth a backup soon: it's often the first sign of a failing disk or cable."
        }
        "GPU" => {
            "The graphics driver reported a fault: it ran out of the GPU's memory or address \
             space, a check inside it failed, or the GPU stopped answering and was reset. Apps \
             drawing at the time may glitch, freeze or crash."
        }
        "Heat" => {
            "A processor got too hot and slowed itself down to cool off, which makes everything \
             slower. Blocked vents, dust or a failing fan are the usual causes."
        }
        _ => {
            "A line the kernel itself rated an error, which isn't one of the kinds above. Many \
             are harmless on a working machine; they matter when they line up with a problem."
        }
    }
}

impl Event {
    pub fn label(&self) -> &'static str {
        match self {
            Event::Crash { .. } => "Crash",
            Event::OutOfMemory { .. } => "Out of memory",
            Event::Hung { .. } => "Hung",
            Event::Disk => "Disk",
            Event::Gpu => "GPU",
            Event::Heat => "Heat",
            Event::Error => "Error",
        }
    }

    /// The process it's about, where it names one.
    pub fn process(&self) -> Option<(&str, u32)> {
        match self {
            Event::Crash { name, pid }
            | Event::OutOfMemory { name, pid }
            | Event::Hung { name, pid } => Some((name, *pid)),
            _ => None,
        }
    }
}

/// Where the lines come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Journal,
    Kmsg,
    Dmesg,
}

impl Source {
    pub fn describe(self) -> &'static str {
        match self {
            Source::Journal => "from the journal",
            Source::Kmsg => "from /dev/kmsg",
            Source::Dmesg => "from dmesg",
        }
    }

    /// Where the lines come from, in a sentence.
    pub fn explain(self) -> &'static str {
        match self {
            Source::Journal => {
                "Read from the system journal (journalctl -k), which keeps the kernel's lines; \
                 you can read it as a member of the adm or systemd-journal group."
            }
            Source::Kmsg => "Read straight from the kernel, through /dev/kmsg.",
            Source::Dmesg => {
                "Read with dmesg every few seconds, which gives no times or levels: errors are \
                 recognised by their words."
            }
        }
    }
}

pub enum Update {
    Source(Source),
    Line(Entry),
    /// Nothing can be read, and why.
    Unreadable(String),
}

/// Starts reading, each line sent as it's read with a repaint of `ctx`.
pub fn start(ctx: egui::Context) -> mpsc::Receiver<Update> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("kernel log".into())
        .spawn(move || {
            let send = |u: Update| {
                let sent = send.send(u).is_ok();
                ctx.request_repaint();
                sent
            };
            let journal = journal(&send);
            if journal.is_ok() {
                return;
            }
            if kmsg(&send).is_ok() {
                return;
            }
            if let Err(e) = dmesg(&send) {
                let why = match journal {
                    Err(j) => format!("{j}; and dmesg: {e}"),
                    Ok(()) => e,
                };
                send(Update::Unreadable(why));
            }
        });
    receive
}

/// From the journal: the backlog, then followed. An error if it has no
/// kernel lines for this user, so the next source is tried.
fn journal(send: &dyn Fn(Update) -> bool) -> Result<(), String> {
    let backlog = Command::new("journalctl")
        .args(["-k", "-b", "-o", "json", "--no-pager", "-n"])
        .arg(BACKLOG.to_string())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("journalctl: {e}"))?;
    let text = String::from_utf8_lossy(&backlog.stdout);
    let mut cursor = None;
    let mut any = false;
    for line in text.lines() {
        if let Some((entry, c)) = parse_json(line) {
            if !any {
                any = true;
                send(Update::Source(Source::Journal));
            }
            cursor = Some(c);
            if !send(Update::Line(entry)) {
                return Ok(());
            }
        }
    }
    if !any {
        return Err("the journal has none of the kernel's lines for you".into());
    }
    let mut follow = Command::new("journalctl");
    follow.args(["-k", "-b", "-o", "json", "--no-pager", "-f"]);
    if let Some(c) = cursor {
        follow.arg(format!("--after-cursor={c}"));
    }
    let Ok(mut child) = follow.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return Ok(());
    };
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some((entry, _)) = parse_json(&line)
                && !send(Update::Line(entry))
            {
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

/// A journal line in JSON, and its cursor.
fn parse_json(line: &str) -> Option<(Entry, String)> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let field = |name: &str| v.get(name).and_then(|f| f.as_str());
    let message = match v.get("MESSAGE")? {
        serde_json::Value::String(s) => s.clone(),
        // A message that isn't UTF-8 comes as its bytes.
        serde_json::Value::Array(bytes) => {
            let bytes: Vec<u8> = bytes
                .iter()
                .filter_map(|b| b.as_u64())
                .map(|b| b as u8)
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => return None,
    };
    let priority = field("PRIORITY").and_then(|p| p.parse().ok()).unwrap_or(6);
    let time = field("__REALTIME_TIMESTAMP")
        .and_then(|t| t.parse::<f64>().ok())
        .map_or(0.0, |us| us / 1e6);
    let cursor = field("__CURSOR")?.to_string();
    Some((entry(time, priority, message), cursor))
}

fn entry(time: f64, priority: u8, message: String) -> Entry {
    let event = classify(&message, priority);
    Entry {
        time,
        priority,
        message,
        event,
    }
}

/// From `/dev/kmsg`, which gives the whole buffer and then blocks for each
/// new line.
fn kmsg(send: &dyn Fn(Update) -> bool) -> Result<(), String> {
    let mut file = std::fs::File::open("/dev/kmsg").map_err(|e| format!("/dev/kmsg: {e}"))?;
    let boot = boot_time();
    // Each read is one record; a record bigger than this is cut short.
    let mut buffer = vec![0u8; 8192];
    let mut announced = false;
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                let text = String::from_utf8_lossy(&buffer[..n]);
                let Some(e) = parse_kmsg(&text, boot) else {
                    continue;
                };
                if !announced {
                    announced = true;
                    send(Update::Source(Source::Kmsg));
                }
                if !send(Update::Line(e)) {
                    return Ok(());
                }
            }
            // Lines overwritten before they were read; carry on.
            Err(e) if e.raw_os_error() == Some(libc::EPIPE) => {}
            Err(e) if !announced => return Err(format!("/dev/kmsg: {e}")),
            Err(_) => return Ok(()),
        }
    }
}

/// A `/dev/kmsg` record: "6,339,5140900,-;message", then key=value lines
/// starting with a space; the time in microseconds since boot.
fn parse_kmsg(text: &str, boot: f64) -> Option<Entry> {
    let (head, rest) = text.split_once(';')?;
    let mut fields = head.split(',');
    let level: u32 = fields.next()?.parse().ok()?;
    let _sequence = fields.next()?;
    let since_boot: f64 = fields.next()?.parse().ok()?;
    let message = rest.lines().next().unwrap_or("").to_string();
    Some(entry(boot + since_boot / 1e6, (level & 7) as u8, message))
}

/// When the machine started, in seconds since the epoch.
fn boot_time() -> f64 {
    std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("btime "))
                .and_then(|t| t.trim().parse::<f64>().ok())
        })
        .unwrap_or(0.0)
}

/// From `dmesg`, asked again every few seconds, sending what's new: the
/// lines after the last one seen before.
fn dmesg(send: &dyn Fn(Update) -> bool) -> Result<(), String> {
    let mut seen: Vec<String> = Vec::new();
    let mut announced = false;
    loop {
        let out = Command::new("dmesg")
            .stderr(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err("not permitted".into());
        }
        let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .collect();
        if !announced {
            announced = true;
            send(Update::Source(Source::Dmesg));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        for line in new_lines(&seen, &lines) {
            // dmesg tells neither when nor how bad; errors are found by
            // their words.
            let first = seen.is_empty();
            if !send(Update::Line(entry(
                if first { 0.0 } else { now },
                6,
                line.clone(),
            ))) {
                return Ok(());
            }
        }
        seen = lines;
        std::thread::sleep(DMESG_POLL);
    }
}

/// The lines of `now` after the last of `before` (all of them if it's not
/// there, the buffer having turned over).
fn new_lines<'a>(before: &[String], now: &'a [String]) -> &'a [String] {
    match before.last() {
        None => now,
        Some(last) => match now.iter().rposition(|l| l == last) {
            Some(i) => &now[i + 1..],
            None => now,
        },
    }
}

/// Whether `text` has any of `wanted` as words, not inside others
/// ("hang" isn't in "changed").
fn words(text: &str, wanted: &[&str]) -> bool {
    wanted.iter().any(|w| {
        text.match_indices(w).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + w.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    })
}

/// What `message` says happened, if it's one of the things looked for.
pub fn classify(message: &str, priority: u8) -> Option<Event> {
    let lower = message.to_lowercase();
    if let Some(rest) = message
        .split_once("Out of memory: Killed process ")
        .map(|(_, r)| r)
    {
        let (pid, rest) = rest.split_once(' ')?;
        let name = rest.strip_prefix('(')?.split_once(')')?.0;
        return Some(Event::OutOfMemory {
            name: name.to_string(),
            pid: pid.parse().ok()?,
        });
    }
    // "Isolated Web Co[741150]: segfault at 0 ip …", and "traps: name[pid] …".
    let named = |text: &str| -> Option<(String, u32)> {
        let open = text.rfind('[')?;
        let close = open + text[open..].find(']')?;
        Some((
            text[..open].trim().to_string(),
            text[open + 1..close].parse().ok()?,
        ))
    };
    if let Some((before, _)) = message.split_once("]: segfault at") {
        let (name, pid) = named(&format!("{before}]"))?;
        return Some(Event::Crash { name, pid });
    }
    if let Some(rest) = message.strip_prefix("traps: ") {
        let upto = rest.find(']')? + 1;
        let (name, pid) = named(&rest[..upto])?;
        return Some(Event::Crash { name, pid });
    }
    // "INFO: task kworker/u8:2:123 blocked for more than 120 seconds."
    if let Some(rest) = message.strip_prefix("INFO: task ")
        && let Some((task, _)) = rest.split_once(" blocked for more than")
    {
        let (name, pid) = task.rsplit_once(':')?;
        return Some(Event::Hung {
            name: name.to_string(),
            pid: pid.parse().ok()?,
        });
    }
    let error = priority <= 3;
    if lower.contains("i/o error")
        || lower.contains("ext4-fs error")
        || lower.contains("btrfs error")
        || lower.contains("xfs") && error
        || lower.contains("nvme") && (lower.contains("timeout") || error)
        || lower.contains("ata") && lower.contains("failed command")
    {
        return Some(Event::Disk);
    }
    let gpu = [
        "drm", "gpu", "amdgpu", "i915", "nouveau", "nvidia", "nvrm", "radeon",
    ]
    .iter()
    .any(|w| lower.contains(w));
    if gpu
        && (error
            || lower.contains("xid")
            || words(
                &lower,
                &["hang", "hung", "reset", "timeout", "timed out", "failed"],
            )
            || lower.contains("can't alloc")
            || lower.contains("no_memory"))
    {
        return Some(Event::Gpu);
    }
    if lower.contains("temperature above threshold")
        || lower.contains("clock throttled")
        || lower.contains("critical temperature")
    {
        return Some(Event::Heat);
    }
    error.then_some(Event::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crashes_name_their_process() {
        let segfault = "Isolated Web Co[741150]: segfault at 0 ip 0000787b7d86be0a sp \
            00007fff3d8cda20 error 6 in libxul.so[606ae0a,787b7ab82000+7a48000]";
        assert_eq!(
            classify(segfault, 6),
            Some(Event::Crash {
                name: "Isolated Web Co".into(),
                pid: 741150
            })
        );
        let trap = "traps: drag-out[539652] general protection fault ip:7f sp:7f error:0 in x.so";
        assert_eq!(
            classify(trap, 6),
            Some(Event::Crash {
                name: "drag-out".into(),
                pid: 539652
            })
        );
    }

    #[test]
    fn out_of_memory_and_hung_tasks() {
        let oom = "Out of memory: Killed process 4242 (firefox) total-vm:1234kB, anon-rss:99kB";
        assert_eq!(
            classify(oom, 3),
            Some(Event::OutOfMemory {
                name: "firefox".into(),
                pid: 4242
            })
        );
        let hung = "INFO: task kworker/u8:2:123 blocked for more than 120 seconds.";
        assert_eq!(
            classify(hung, 3),
            Some(Event::Hung {
                name: "kworker/u8:2".into(),
                pid: 123
            })
        );
    }

    #[test]
    fn disks_gpus_heat_and_other_errors() {
        assert_eq!(
            classify("blk_update_request: I/O error, dev sda, sector 123", 3),
            Some(Event::Disk)
        );
        let nvidia = "[drm:__nv_drm_gem_nvkms_map [nvidia_drm]] *ERROR* [nvidia-drm] Failed to map";
        assert_eq!(classify(nvidia, 3), Some(Event::Gpu));
        assert_eq!(
            classify(
                "NVRM: Xid (PCI:0000:08:00): 79, GPU has fallen off the bus.",
                4
            ),
            Some(Event::Gpu)
        );
        assert_eq!(
            classify(
                "CPU3: Core temperature above threshold, cpu clock throttled",
                4
            ),
            Some(Event::Heat)
        );
        assert_eq!(classify("something broke", 3), Some(Event::Error));
        assert_eq!(classify("usb 1-1: new high-speed USB device", 6), None);
        assert_eq!(classify("[drm] Initialized amdgpu 3.57.0", 6), None);
        let vga = "nvidia 0000:08:00.0: vgaarb: VGA decodes changed: olddecodes=io+mem";
        assert_eq!(classify(vga, 6), None);
        assert_eq!(classify("amdgpu: GPU reset begin!", 4), Some(Event::Gpu));
        let nvrm =
            "NVRM: nvAssertOkFailedNoLog: Assertion failed: Out of memory [NV_ERR_NO_MEMORY]";
        assert_eq!(classify(nvrm, 4), Some(Event::Gpu));
        assert_eq!(
            classify(
                "NVRM: dmaAllocMapping_GM107: can't alloc VA space for mapping.",
                4
            ),
            Some(Event::Gpu)
        );
    }

    #[test]
    fn a_journal_line() {
        let line = r#"{"PRIORITY":"3","__REALTIME_TIMESTAMP":"1792863041392747","MESSAGE":"oops","__CURSOR":"s=1"}"#;
        let (e, cursor) = parse_json(line).unwrap();
        assert_eq!(
            (e.priority, e.message.as_str(), cursor.as_str()),
            (3, "oops", "s=1")
        );
        assert!((e.time - 1792863041.392747).abs() < 1e-3);
        assert_eq!(e.event, Some(Event::Error));
        let bytes = r#"{"MESSAGE":[104,105],"__CURSOR":"c"}"#;
        assert_eq!(parse_json(bytes).unwrap().0.message, "hi");
    }

    #[test]
    fn a_kmsg_record() {
        let e = parse_kmsg(
            "3,339,5140900,-;EXT4-fs error (device sda1): bad\n SUBSYSTEM=block\n",
            1000.0,
        )
        .unwrap();
        assert_eq!(e.priority, 3);
        assert_eq!(e.message, "EXT4-fs error (device sda1): bad");
        assert!((e.time - 1005.1409).abs() < 1e-6);
        assert_eq!(e.event, Some(Event::Disk));
    }

    #[test]
    fn dmesg_sends_only_whats_new() {
        let s = |v: &[&str]| v.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let before = s(&["a", "b"]);
        assert_eq!(new_lines(&before, &s(&["a", "b", "c"])), ["c"]);
        assert_eq!(new_lines(&[], &s(&["a"])), ["a"]);
        assert_eq!(new_lines(&before, &s(&["x", "y"])), ["x", "y"]);
    }

    /// Reads this machine's log and prints what it found: `cargo test -p
    /// patina-processes kernel_log_here -- --ignored --nocapture`.
    #[test]
    #[ignore = "depends on the machine"]
    fn kernel_log_here() {
        let updates = start(egui::Context::default());
        let mut counts = std::collections::BTreeMap::new();
        let mut lines = 0;
        while let Ok(u) = updates.recv_timeout(Duration::from_secs(3)) {
            match u {
                Update::Source(s) => println!("source: {s:?}"),
                Update::Unreadable(e) => println!("unreadable: {e}"),
                Update::Line(e) => {
                    lines += 1;
                    if let Some(event) = &e.event {
                        *counts.entry(event.label()).or_insert(0) += 1;
                        if event.process().is_some() || event.label() == "GPU" {
                            println!(
                                "{:?}: {}",
                                event,
                                e.message.chars().take(90).collect::<String>()
                            );
                        }
                    }
                }
            }
        }
        println!("{lines} lines, {counts:?}");
        assert!(lines > 0);
    }
}

//! What a process's own window shows beyond the list (ADR-0089), read from
//! `/proc/<pid>` for the processes whose windows are open: what it's
//! waiting in, its memory broken down, its threads and how busy each is,
//! its open files and network connections, its environment and the rest.
//! Linux's `/proc` only; elsewhere the window has the graphs alone. A
//! process of another user's shows what the kernel lets anyone read.

use std::collections::HashMap;
use std::path::Path;

/// One process's details, as last read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Details {
    /// The kernel function it's sleeping in, when it is (`wchan`).
    pub waiting_in: Option<String>,
    /// From `status`: "S (sleeping)".
    pub state: Option<String>,
    pub exe: Option<String>,
    pub cwd: Option<String>,
    pub nice: Option<i32>,
    pub oom_score: Option<i32>,
    /// Its cgroup, which names its systemd unit or scope.
    pub cgroup: Option<String>,
    /// Times it gave up the CPU to wait, and was made to.
    pub switches: Option<(u64, u64)>,
    /// CPU time used, user and system, in seconds.
    pub cpu_time: Option<(f64, f64)>,
    pub memory: Option<Memory>,
    pub threads: Vec<Thread>,
    pub files: Vec<String>,
    pub sockets: Vec<Socket>,
    /// Pipes, event fds and the like, counted.
    pub other_fds: usize,
    pub environment: Vec<String>,
    /// The soft limit on open files.
    pub fd_limit: Option<u64>,
}

/// Its memory, from `smaps_rollup`, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Memory {
    pub rss: u64,
    /// Its share of what it shares with others, counted in.
    pub pss: u64,
    pub anonymous: u64,
    pub file: u64,
    pub shared: u64,
    pub swap: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    pub tid: u32,
    pub name: String,
    /// Busy since the last reading, as a share of one CPU, 0 to 100.
    pub cpu: f32,
    pub state: char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socket {
    /// "TCP", "UDP", "TCP6", "UDP6" or "Unix".
    pub kind: &'static str,
    pub local: String,
    pub remote: String,
    /// TCP's state ("Established", "Listening"); empty for the rest.
    pub state: String,
}

/// Reads details, remembering each thread's CPU time to work out how busy
/// it was since.
#[derive(Default)]
pub struct Reader {
    /// Clock ticks used by each thread at the last reading.
    ticks: HashMap<(u32, u32), u64>,
}

impl Reader {
    /// The details of each of `pids`, `seconds` since the last reading.
    pub fn read(&mut self, pids: &[u32], seconds: f64) -> HashMap<u32, Details> {
        let mut ticks = HashMap::new();
        let details = pids
            .iter()
            .map(|&pid| (pid, self.one(pid, seconds, &mut ticks)))
            .collect();
        self.ticks = ticks;
        details
    }

    fn one(&self, pid: u32, seconds: f64, ticks: &mut HashMap<(u32, u32), u64>) -> Details {
        let dir = Path::new("/proc").join(pid.to_string());
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        let link = |name: &str| {
            std::fs::read_link(dir.join(name))
                .ok()
                .map(|p| p.display().to_string())
        };
        let status = read("status").unwrap_or_default();
        let field = |name: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
                .map(|v| v.trim().to_string())
        };
        // A process's switches are its threads' together: its own
        // `status` counts the first thread's alone.
        let mut switches: Option<(u64, u64)> = None;
        let hz = clock_ticks();
        let stat = read("stat").and_then(|s| parse_stat(&s));
        let (sockets_by_inode, mut sockets, mut files, mut other_fds) =
            (net_sockets(&dir), Vec::new(), Vec::new(), 0);
        if let Ok(fds) = std::fs::read_dir(dir.join("fd")) {
            for fd in fds.flatten() {
                let Ok(target) = std::fs::read_link(fd.path()) else {
                    continue;
                };
                let target = target.display().to_string();
                if let Some(inode) = target
                    .strip_prefix("socket:[")
                    .and_then(|t| t.strip_suffix(']'))
                    .and_then(|t| t.parse::<u64>().ok())
                {
                    if let Some(s) = sockets_by_inode.get(&inode) {
                        sockets.push(s.clone());
                    } else {
                        other_fds += 1;
                    }
                } else if target.starts_with('/') {
                    files.push(target);
                } else {
                    other_fds += 1;
                }
            }
        }
        files.sort();
        files.dedup();
        sockets.sort_by(|a, b| (a.kind, &a.local).cmp(&(b.kind, &b.local)));

        let mut threads = Vec::new();
        if let Ok(tasks) = std::fs::read_dir(dir.join("task")) {
            for task in tasks.flatten() {
                let Some(tid) = task
                    .file_name()
                    .to_str()
                    .and_then(|t| t.parse::<u32>().ok())
                else {
                    continue;
                };
                let Some(s) = std::fs::read_to_string(task.path().join("stat"))
                    .ok()
                    .and_then(|s| parse_stat(&s))
                else {
                    continue;
                };
                if let Some((waited, made)) = std::fs::read_to_string(task.path().join("status"))
                    .ok()
                    .and_then(|t| parse_switches(&t))
                {
                    let sum = switches.get_or_insert((0, 0));
                    sum.0 += waited;
                    sum.1 += made;
                }
                let used = s.utime + s.stime;
                let cpu = self.ticks.get(&(pid, tid)).map_or(0.0, |before| {
                    used.saturating_sub(*before) as f64 / hz / seconds * 100.0
                });
                ticks.insert((pid, tid), used);
                threads.push(Thread {
                    tid,
                    name: s.name,
                    cpu: cpu as f32,
                    state: s.state,
                });
            }
        }
        threads.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.tid.cmp(&b.tid)));

        let environment = read("environ")
            .map(|e| {
                let mut vars: Vec<String> = e
                    .split('\0')
                    .filter(|v| !v.is_empty())
                    .map(String::from)
                    .collect();
                vars.sort();
                vars
            })
            .unwrap_or_default();

        Details {
            waiting_in: read("wchan").filter(|w| !w.is_empty() && w != "0"),
            state: field("State"),
            exe: link("exe"),
            cwd: link("cwd"),
            nice: stat.as_ref().map(|s| s.nice),
            oom_score: read("oom_score").and_then(|s| s.trim().parse().ok()),
            cgroup: read("cgroup").and_then(|c| {
                c.lines()
                    .next()
                    .and_then(|l| l.rsplit(':').next())
                    .map(String::from)
            }),
            switches,
            cpu_time: stat
                .as_ref()
                .map(|s| (s.utime as f64 / hz, s.stime as f64 / hz)),
            memory: read("smaps_rollup").map(|s| parse_rollup(&s)),
            threads,
            files,
            sockets,
            other_fds,
            environment,
            fd_limit: read("limits").and_then(|l| parse_fd_limit(&l)),
        }
    }
}

/// The kernel's clock ticks a second, for `stat`'s times.
fn clock_ticks() -> f64 {
    // SAFETY: sysconf(3) reads a constant.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz > 0 { hz as f64 } else { 100.0 }
}

/// What's used here of `stat`.
#[derive(Debug, PartialEq)]
struct Stat {
    name: String,
    state: char,
    utime: u64,
    stime: u64,
    nice: i32,
}

/// `stat`'s line: the name is in parentheses and may hold spaces and
/// parentheses itself, so the fields are counted from the last `)`.
fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    // After the name: state is field 3, utime 14, stime 15, nice 19.
    let at = |field: usize| rest.get(field - 3).copied();
    Some(Stat {
        name,
        state: at(3)?.chars().next()?,
        utime: at(14)?.parse().ok()?,
        stime: at(15)?.parse().ok()?,
        nice: at(19)?.parse().ok()?,
    })
}

/// `status`' context switches: voluntary and not.
fn parse_switches(text: &str) -> Option<(u64, u64)> {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    Some((
        field("voluntary_ctxt_switches")?,
        field("nonvoluntary_ctxt_switches")?,
    ))
}

fn parse_rollup(text: &str) -> Memory {
    let kib = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map_or(0, |k| k * 1024)
    };
    Memory {
        rss: kib("Rss"),
        pss: kib("Pss"),
        anonymous: kib("Pss_Anon"),
        file: kib("Pss_File"),
        shared: kib("Pss_Shmem"),
        swap: kib("Swap"),
    }
}

fn parse_fd_limit(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("Max open files"))?;
    line.split_whitespace().nth(3)?.parse().ok()
}

/// The network sockets in `dir`'s network namespace, by inode.
fn net_sockets(dir: &Path) -> HashMap<u64, Socket> {
    let mut found = HashMap::new();
    for (file, kind) in [
        ("tcp", "TCP"),
        ("tcp6", "TCP6"),
        ("udp", "UDP"),
        ("udp6", "UDP6"),
    ] {
        if let Ok(text) = std::fs::read_to_string(dir.join("net").join(file)) {
            for (inode, socket) in parse_inet(&text, kind) {
                found.insert(inode, socket);
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("net/unix")) {
        found.extend(parse_unix(&text));
    }
    found
}

/// `/proc/net/tcp` and its kin: each socket's addresses, state and inode.
fn parse_inet(text: &str, kind: &'static str) -> Vec<(u64, Socket)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let inode = f.get(9)?.parse::<u64>().ok()?;
            let state = if kind.starts_with("TCP") {
                tcp_state(f.get(3)?).to_string()
            } else {
                String::new()
            };
            Some((
                inode,
                Socket {
                    kind,
                    local: address(f.get(1)?)?,
                    remote: address(f.get(2)?)?,
                    state,
                },
            ))
        })
        .collect()
}

/// `/proc/net/unix`: each socket's path, where it has one.
fn parse_unix(text: &str) -> Vec<(u64, Socket)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let inode = f.get(6)?.parse::<u64>().ok()?;
            let path = f.get(7)?;
            Some((
                inode,
                Socket {
                    kind: "Unix",
                    local: path.to_string(),
                    remote: String::new(),
                    state: String::new(),
                },
            ))
        })
        .collect()
}

/// "0100007F:0035" (little-endian words, as the kernel writes them) as
/// "127.0.0.1:53"; IPv6 in brackets.
fn address(hex: &str) -> Option<String> {
    let (ip, port) = hex.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    // Each word is the address's bytes read as a number in the machine's
    // order, so its own bytes in that order are the address's.
    let words: Vec<[u8; 4]> = (0..ip.len() / 8)
        .map(|i| u32::from_str_radix(&ip[i * 8..i * 8 + 8], 16).map(u32::to_ne_bytes))
        .collect::<Result<_, _>>()
        .ok()?;
    match words.as_slice() {
        [w] => Some(format!("{}:{port}", std::net::Ipv4Addr::from(*w))),
        [_, _, _, _] => {
            let mut bytes = [0u8; 16];
            for (i, w) in words.iter().enumerate() {
                bytes[i * 4..i * 4 + 4].copy_from_slice(w);
            }
            let ip = std::net::Ipv6Addr::from(bytes);
            Some(match ip.to_ipv4_mapped() {
                Some(v4) => format!("{v4}:{port}"),
                None => format!("[{ip}]:{port}"),
            })
        }
        _ => None,
    }
}

fn tcp_state(hex: &str) -> &'static str {
    match hex {
        "01" => "Established",
        "02" => "Opening",
        "03" => "Accepted",
        "04" | "05" | "08" | "09" | "0B" => "Closing",
        "06" => "Time wait",
        "07" => "Closed",
        "0A" => "Listening",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stat_line_with_a_name_full_of_parentheses() {
        let line = "1234 (Web Content (x)) S 1 1234 1234 0 -1 4194560 100 0 0 0 \
            250 50 0 0 20 5 30 0 12345 0 0";
        let s = parse_stat(line).unwrap();
        assert_eq!(s.name, "Web Content (x)");
        assert_eq!((s.state, s.utime, s.stime, s.nice), ('S', 250, 50, 5));
    }

    #[test]
    fn memory_from_smaps_rollup() {
        let text = "40200000-7ffd [rollup]\nRss:  998192 kB\nPss:  937621 kB\n\
            Pss_Anon:  801252 kB\nPss_File:  93145 kB\nPss_Shmem:  43224 kB\nSwap:  10 kB\n";
        let m = parse_rollup(text);
        assert_eq!(m.rss, 998192 * 1024);
        assert_eq!(m.anonymous, 801252 * 1024);
        assert_eq!(m.shared, 43224 * 1024);
        assert_eq!(m.swap, 10 * 1024);
    }

    #[test]
    fn addresses_as_the_kernel_writes_them() {
        assert_eq!(address("0100007F:0035").as_deref(), Some("127.0.0.1:53"));
        assert_eq!(
            address("00000000000000000000000001000000:1F90").as_deref(),
            Some("[::1]:8080")
        );
        assert_eq!(
            address("0000000000000000FFFF00000100007F:0050").as_deref(),
            Some("127.0.0.1:80")
        );
    }

    #[test]
    fn tcp_sockets_with_their_state_and_inode() {
        let text = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
            0: 0100007F:0277 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 25351 1\n";
        let s = parse_inet(text, "TCP");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].0, 25351);
        assert_eq!(s[0].1.local, "127.0.0.1:631");
        assert_eq!(s[0].1.state, "Listening");
    }

    #[test]
    fn context_switches_from_status() {
        let text = "Name:\tx\nvoluntary_ctxt_switches:\t120\nnonvoluntary_ctxt_switches:\t7\n";
        assert_eq!(parse_switches(text), Some((120, 7)));
    }

    #[test]
    fn the_open_files_limit() {
        let text = "Limit                     Soft Limit           Hard Limit           Units\n\
            Max open files            1024                 524288               files\n";
        assert_eq!(parse_fd_limit(text), Some(1024));
    }

    #[test]
    fn this_process_reads_itself() {
        let me = std::process::id();
        let mut reader = Reader::default();
        reader.read(&[me], 1.0);
        let d = &reader.read(&[me], 1.0)[&me];
        assert!(d.state.is_some());
        assert!(d.memory.is_some_and(|m| m.rss > 0));
        assert!(d.threads.iter().any(|t| t.tid == me));
        assert!(d.environment.iter().any(|v| v.starts_with("PATH=")));
        assert!(d.fd_limit.is_some());
    }
}

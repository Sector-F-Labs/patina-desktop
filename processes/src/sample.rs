//! Reading the machine (ADR-0088): every process, the CPUs, memory, disks
//! and network through `sysinfo` (Linux and FreeBSD alike), and the open
//! windows' processes from the compositor. One thread reads, at the update
//! speed chosen, and sends each [`Snapshot`]; the window never waits on it.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use patina_system::windows::{Window, Windows};
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind,
    ProcessStatus, ProcessesToUpdate, RefreshKind, System, ThreadKind, UpdateKind, Users,
};

/// What a process is doing, as far as it matters here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Sleeping,
    /// Paused (SIGSTOP), or stopped by a debugger.
    Stopped,
    /// Exited, waiting for its parent to notice.
    Zombie,
    Other,
}

/// One process.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    /// Its command line, or its name where that can't be read.
    pub command: String,
    pub user: String,
    pub uid: Option<u32>,
    /// Its share of the whole machine's CPU time, 0 to 100.
    pub cpu: f32,
    /// Resident memory, in bytes.
    pub memory: u64,
    /// Read from and written to disk since the last reading, per second.
    pub read: f64,
    pub written: f64,
    pub state: State,
    /// When it started, in seconds since the epoch.
    pub started: u64,
    /// A kernel thread (Linux), which nothing here can end.
    pub kernel: bool,
    /// Its use of the GPUs: busy, 0 to 100 a GPU, and their memory held.
    pub gpu: f32,
    pub gpu_memory: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

/// A mounted filesystem's space.
#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub mount: String,
    pub device: String,
    pub file_system: String,
    pub total: u64,
    pub available: u64,
}

/// A network interface, in bytes per second.
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub name: String,
    pub received: f64,
    pub sent: f64,
    pub total_received: u64,
    pub total_sent: u64,
    pub addresses: Vec<String>,
}

/// What one reading found.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub at: Instant,
    pub processes: Vec<Proc>,
    /// The compositor's windows, or why they can't be had (another
    /// desktop): then every process is a background one.
    pub windows: Result<Vec<Window>, String>,
    /// Across all CPUs, 0 to 100, and each CPU's.
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub cpu_name: String,
    /// The first CPU's clock, in MHz.
    pub frequency: u64,
    pub physical_cores: Option<usize>,
    pub memory: Memory,
    /// All disks' reading and writing, in bytes per second.
    pub disk_read: f64,
    pub disk_written: f64,
    pub volumes: Vec<Volume>,
    pub interfaces: Vec<Interface>,
    pub load: [f64; 3],
    pub uptime: u64,
    pub gpus: Vec<crate::gpu::Gpu>,
    /// The machine's name, its system and kernel.
    pub host: String,
    pub os: String,
    pub kernel: String,
    /// The details of the processes whose windows are open.
    pub details: HashMap<u32, crate::details::Details>,
}

/// What the window asks of the reader.
pub enum Ask {
    /// Read this often; `None` pauses.
    Every(Option<Duration>),
    /// Read now, after ending or pausing something.
    Now,
    /// Read the details of these processes too, now and each time.
    Watch(Vec<u32>),
}

/// Starts the reader, reading `every` so often; each snapshot comes back
/// with a repaint of `ctx`.
pub fn start(
    ctx: egui::Context,
    every: Option<Duration>,
) -> (mpsc::Sender<Ask>, mpsc::Receiver<Snapshot>) {
    let (ask, asked) = mpsc::channel();
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("reader".into())
        .spawn(move || {
            let mut reader = Reader::new();
            let mut every = every;
            loop {
                if send.send(reader.read()).is_err() {
                    return;
                }
                ctx.request_repaint();
                // Paused, wait for the next ask however long it takes.
                let next = match every {
                    Some(d) => asked
                        .recv_timeout(d)
                        .map_err(|e| e == mpsc::RecvTimeoutError::Disconnected),
                    None => asked.recv().map_err(|_| true),
                };
                match next {
                    Ok(Ask::Every(d)) => every = d,
                    Ok(Ask::Now) => {}
                    Ok(Ask::Watch(pids)) => reader.watched = pids,
                    Err(false) => {}
                    Err(true) => return,
                }
                // Asks queued meanwhile, the last speed winning.
                while let Ok(a) = asked.try_recv() {
                    match a {
                        Ask::Every(d) => every = d,
                        Ask::Watch(pids) => reader.watched = pids,
                        Ask::Now => {}
                    }
                }
            }
        });
    (ask, receive)
}

struct Reader {
    system: System,
    disks: Disks,
    networks: Networks,
    users: Users,
    windows: Option<Windows>,
    gpus: crate::gpu::Reader,
    /// The host's name, system and kernel, read once.
    host: (String, String, String),
    /// The processes whose details are read, and their reader.
    watched: Vec<u32>,
    details: crate::details::Reader,
    last: Instant,
}

impl Reader {
    fn new() -> Self {
        let system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            users: Users::new_with_refreshed_list(),
            windows: None,
            gpus: crate::gpu::Reader::new(),
            watched: Vec::new(),
            details: crate::details::Reader::default(),
            host: (
                System::host_name().unwrap_or_default(),
                System::long_os_version().unwrap_or_default(),
                System::kernel_version().unwrap_or_default(),
            ),
            last: Instant::now(),
        }
    }

    fn read(&mut self) -> Snapshot {
        let now = Instant::now();
        let seconds = now.duration_since(self.last).as_secs_f64().max(0.05);
        self.last = now;
        self.system.refresh_cpu_all();
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_disk_usage()
                .with_user(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .without_tasks(),
        );
        self.disks
            .refresh_specifics(true, DiskRefreshKind::everything());
        self.networks.refresh(true);

        let (gpus, gpu_uses) = self.gpus.read();
        let cpus = self.system.cpus().len().max(1) as f32;
        let mut names: HashMap<u32, String> = HashMap::new();
        let mut processes = Vec::with_capacity(self.system.processes().len());
        for p in self.system.processes().values() {
            // Threads, where they're listed as processes (Linux).
            if p.thread_kind() == Some(ThreadKind::Userland) {
                continue;
            }
            let uid = p.user_id().map(|u| **u);
            let user = match uid {
                Some(uid) => names
                    .entry(uid)
                    .or_insert_with(|| {
                        self.users
                            .list()
                            .iter()
                            .find(|u| **u.id() == uid)
                            .map_or_else(|| uid.to_string(), |u| u.name().to_string())
                    })
                    .clone(),
                None => String::new(),
            };
            let name = full_name(
                p.name().to_string_lossy().into_owned(),
                p.cmd().first().map(|a| a.as_os_str()),
            );
            let command = if p.cmd().is_empty() {
                name.clone()
            } else {
                p.cmd()
                    .iter()
                    .map(|a| a.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let io = p.disk_usage();
            processes.push(Proc {
                pid: p.pid().as_u32(),
                parent: p.parent().map(|p| p.as_u32()),
                name,
                command,
                user,
                uid,
                cpu: p.cpu_usage() / cpus,
                memory: p.memory(),
                read: io.read_bytes as f64 / seconds,
                written: io.written_bytes as f64 / seconds,
                state: match p.status() {
                    ProcessStatus::Run => State::Running,
                    ProcessStatus::Sleep | ProcessStatus::Idle => State::Sleeping,
                    ProcessStatus::Stop | ProcessStatus::Tracing => State::Stopped,
                    ProcessStatus::Zombie | ProcessStatus::Dead => State::Zombie,
                    _ => State::Other,
                },
                started: p.start_time(),
                kernel: p.thread_kind() == Some(ThreadKind::Kernel),
                gpu: gpu_uses.get(&p.pid().as_u32()).map_or(0.0, |u| u.busy),
                gpu_memory: gpu_uses.get(&p.pid().as_u32()).map_or(0, |u| u.memory),
            });
        }

        // Each device once: a disk mounted twice (btrfs subvolumes) is
        // still one disk's reading and writing.
        let mut devices: HashMap<String, (u64, u64)> = HashMap::new();
        let mut volumes = Vec::new();
        for d in self.disks.list() {
            let device = d.name().to_string_lossy().into_owned();
            let usage = d.usage();
            devices.insert(device.clone(), (usage.read_bytes, usage.written_bytes));
            volumes.push(Volume {
                mount: d.mount_point().display().to_string(),
                device,
                file_system: d.file_system().to_string_lossy().into_owned(),
                total: d.total_space(),
                available: d.available_space(),
            });
        }
        let (read, written) = devices
            .values()
            .fold((0, 0), |(r, w), (dr, dw)| (r + dr, w + dw));

        let mut interfaces: Vec<Interface> = self
            .networks
            .list()
            .iter()
            .filter(|(name, n)| shown_interface(name, !n.ip_networks().is_empty()))
            .map(|(name, n)| Interface {
                name: name.clone(),
                received: n.received() as f64 / seconds,
                sent: n.transmitted() as f64 / seconds,
                total_received: n.total_received(),
                total_sent: n.total_transmitted(),
                addresses: n
                    .ip_networks()
                    .iter()
                    .map(|ip| format!("{}/{}", ip.addr, ip.prefix))
                    .collect(),
            })
            .collect();
        interfaces.sort_by(|a, b| a.name.cmp(&b.name));

        if self.windows.is_none() {
            self.windows = Windows::connect().ok();
        }
        let windows = match &self.windows {
            Some(w) => w.list(),
            None => Err("no session bus".into()),
        };
        // The compositor may come back (a restart): connect again next time.
        if windows.is_err() {
            self.windows = None;
        }

        let cpu = &self.system.cpus();
        let load = System::load_average();
        Snapshot {
            at: now,
            processes,
            windows,
            cpu: self.system.global_cpu_usage(),
            cores: cpu.iter().map(|c| c.cpu_usage()).collect(),
            cpu_name: cpu
                .first()
                .map(|c| c.brand().trim().to_string())
                .unwrap_or_default(),
            frequency: cpu.first().map_or(0, |c| c.frequency()),
            physical_cores: System::physical_core_count(),
            memory: Memory {
                total: self.system.total_memory(),
                used: self.system.used_memory(),
                available: self.system.available_memory(),
                swap_total: self.system.total_swap(),
                swap_used: self.system.used_swap(),
            },
            disk_read: read as f64 / seconds,
            disk_written: written as f64 / seconds,
            volumes,
            interfaces,
            load: [load.one, load.five, load.fifteen],
            uptime: System::uptime(),
            gpus,
            host: self.host.0.clone(),
            os: self.host.1.clone(),
            kernel: self.host.2.clone(),
            details: self.details.read(&self.watched, seconds),
        }
    }
}

/// Whether interface `name` is worth a graph: on Linux, one with a device
/// behind it (not loopback, a bridge or a container's end); elsewhere, one
/// with an address but loopback.
fn shown_interface(name: &str, addressed: bool) -> bool {
    if cfg!(target_os = "linux") {
        std::path::Path::new("/sys/class/net")
            .join(name)
            .join("device")
            .exists()
    } else {
        addressed && !name.starts_with("lo")
    }
}

/// A process's name: the kernel keeps 15 bytes of it (Linux's comm), so a
/// name that long is taken whole from the program its command line ran,
/// when that begins the same.
fn full_name(name: String, program: Option<&std::ffi::OsStr>) -> String {
    if name.len() < 15 {
        return name;
    }
    program
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|f| f.to_string_lossy().into_owned())
        .filter(|f| f.starts_with(&name))
        .unwrap_or(name)
}

/// The signals the process manager sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Asks it to end (SIGTERM), as closing would.
    End,
    /// Ends it at once (SIGKILL), for what doesn't listen.
    Kill,
    Pause,
    Resume,
}

impl Signal {
    fn number(self) -> libc::c_int {
        match self {
            Signal::End => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Pause => libc::SIGSTOP,
            Signal::Resume => libc::SIGCONT,
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            Signal::End => "end",
            Signal::Kill => "kill",
            Signal::Pause => "pause",
            Signal::Resume => "resume",
        }
    }
}

/// Sends `signal` to process `pid`.
pub fn send(pid: u32, signal: Signal) -> std::io::Result<()> {
    let pid = libc::pid_t::try_from(pid).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    // Never 0 or -1, which would signal a whole group or everything.
    if pid <= 1 {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    // SAFETY: kill(2) takes plain integers and touches no memory of ours.
    if unsafe { libc::kill(pid, signal.number()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_finds_this_process_and_the_machine() {
        let mut reader = Reader::new();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let s = reader.read();
        let me = std::process::id();
        let this = s.processes.iter().find(|p| p.pid == me).expect("listed");
        assert!(this.memory > 0);
        assert!(!this.user.is_empty());
        assert!(
            this.command.contains("patina_processes") || this.command.contains("patina-processes")
        );
        assert!(!s.cores.is_empty());
        assert!(s.memory.total >= s.memory.used && s.memory.total > 0);
        // No process is listed twice.
        let mut pids: Vec<u32> = s.processes.iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        pids.dedup();
        assert_eq!(pids.len(), s.processes.len());
    }

    #[test]
    fn pausing_resuming_and_ending_a_child() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        send(child.id(), Signal::Pause).unwrap();
        send(child.id(), Signal::Resume).unwrap();
        send(child.id(), Signal::End).unwrap();
        let status = child.wait().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }

    #[test]
    fn a_name_the_kernel_cut_short_is_taken_from_the_program() {
        let name =
            |n: &str, program: &str| full_name(n.into(), Some(std::ffi::OsStr::new(program)));
        assert_eq!(
            name("patina-composit", "/usr/local/bin/patina-compositor"),
            "patina-compositor"
        );
        assert_eq!(
            name("Isolated Web Co", "/usr/lib/firefox/firefox"),
            "Isolated Web Co"
        );
        assert_eq!(name("zsh", "-zsh"), "zsh");
    }

    #[test]
    fn init_and_process_groups_are_never_signalled() {
        assert!(send(0, Signal::Kill).is_err());
        assert!(send(1, Signal::Kill).is_err());
    }
}

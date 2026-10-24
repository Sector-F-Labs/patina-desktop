//! The GPUs, read the ways nvtop reads them (ADR-0088): NVIDIA's through
//! NVML, the library nvidia-smi uses, loaded when it's there; the rest
//! (amdgpu, i915, xe and the other DRM drivers) through each process's
//! `/proc/<pid>/fdinfo` for its open GPU files, which count the time spent
//! on each engine and the memory held, and through sysfs for what the card
//! says of itself. Both give each process's share, for the Processes
//! view's GPU column.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::enums::device::UsedGpuMemory;

/// A GPU, and as much as its driver tells.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub driver: String,
    /// Its PCI address.
    pub address: String,
    /// Busy, 0 to 100.
    pub busy: Option<f32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    /// In °C.
    pub temperature: Option<u32>,
    /// Drawn and allowed, in watts.
    pub power: Option<f64>,
    pub power_limit: Option<f64>,
    /// In MHz.
    pub clock: Option<u32>,
    pub memory_clock: Option<u32>,
    /// The first fan, 0 to 100.
    pub fan: Option<u32>,
    /// Video encoding and decoding, 0 to 100.
    pub encoder: Option<f32>,
    pub decoder: Option<f32>,
}

/// A process's use of the GPUs, all of them together.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Use {
    /// Busy, 0 to 100 for each GPU it uses.
    pub busy: f32,
    pub memory: u64,
}

pub struct Reader {
    nvml: Option<Nvml>,
    /// The last NVML per-process sample's time, per GPU.
    seen: HashMap<u32, u64>,
    /// Each DRM client's engine counters at the last reading, by PID and
    /// client id.
    clients: HashMap<(u32, String), Client>,
    last: Instant,
}

impl Reader {
    pub fn new() -> Self {
        Self {
            nvml: open_nvml(),
            seen: HashMap::new(),
            clients: HashMap::new(),
            last: Instant::now(),
        }
    }

    /// The GPUs, and each process's use of them.
    pub fn read(&mut self) -> (Vec<Gpu>, HashMap<u32, Use>) {
        let now = Instant::now();
        let seconds = now.duration_since(self.last).as_secs_f64().max(0.05);
        self.last = now;
        let mut uses: HashMap<u32, Use> = HashMap::new();
        let mut gpus = match &self.nvml {
            Some(nvml) => nvidia(nvml, &mut self.seen, &mut uses),
            None => Vec::new(),
        };
        if cfg!(target_os = "linux") {
            let nvidia_read = self.nvml.is_some();
            gpus.extend(self.drm(seconds, nvidia_read, &mut uses));
        }
        (gpus, uses)
    }

    /// The DRM drivers' GPUs, from sysfs, and their clients from fdinfo.
    fn drm(&mut self, seconds: f64, nvidia_read: bool, uses: &mut HashMap<u32, Use>) -> Vec<Gpu> {
        let mut gpus: Vec<Gpu> = cards()
            .into_iter()
            .filter(|(_, driver, _)| !(driver == "nvidia" && nvidia_read))
            .map(|(device, driver, address)| card(&device, driver, address))
            .collect();
        if gpus.is_empty() {
            self.clients.clear();
            return gpus;
        }
        // Each client's engines since last time; a GPU without a busy figure
        // of its own (Intel's) is the sum of what its clients used.
        let mut busy_by_gpu: HashMap<String, f32> = HashMap::new();
        let mut seen = HashMap::new();
        for (pid, client) in clients() {
            let key = (pid, client.id.clone());
            let busy = self
                .clients
                .get(&key)
                .map_or(0.0, |before| client.busy_since(before, seconds));
            let used = uses.entry(pid).or_default();
            used.busy += busy;
            used.memory += client.memory;
            *busy_by_gpu.entry(client.address.clone()).or_default() += busy;
            seen.insert(key, client);
        }
        self.clients = seen;
        for gpu in &mut gpus {
            if gpu.busy.is_none() {
                gpu.busy = Some(
                    busy_by_gpu
                        .get(&gpu.address)
                        .copied()
                        .unwrap_or(0.0)
                        .min(100.0),
                );
            }
        }
        gpus
    }
}

/// NVML, under the names the driver installs it as.
fn open_nvml() -> Option<Nvml> {
    Nvml::init().ok().or_else(|| {
        Nvml::builder()
            .lib_path(std::ffi::OsStr::new("libnvidia-ml.so.1"))
            .init()
            .ok()
    })
}

fn nvidia(nvml: &Nvml, seen: &mut HashMap<u32, u64>, uses: &mut HashMap<u32, Use>) -> Vec<Gpu> {
    let mut gpus = Vec::new();
    for index in 0..nvml.device_count().unwrap_or(0) {
        let Ok(d) = nvml.device_by_index(index) else {
            continue;
        };
        let memory = d.memory_info().ok();
        gpus.push(Gpu {
            name: d.name().unwrap_or_else(|_| "NVIDIA GPU".into()),
            driver: "nvidia".into(),
            address: d.pci_info().map(|p| p.bus_id).unwrap_or_default(),
            busy: d.utilization_rates().ok().map(|u| u.gpu as f32),
            memory_used: memory.as_ref().map(|m| m.used),
            memory_total: memory.as_ref().map(|m| m.total),
            temperature: d.temperature(TemperatureSensor::Gpu).ok(),
            power: d.power_usage().ok().map(|mw| f64::from(mw) / 1000.0),
            power_limit: d
                .enforced_power_limit()
                .ok()
                .map(|mw| f64::from(mw) / 1000.0),
            clock: d.clock_info(Clock::Graphics).ok(),
            memory_clock: d.clock_info(Clock::Memory).ok(),
            fan: d.fan_speed(0).ok(),
            encoder: d.encoder_utilization().ok().map(|u| u.utilization as f32),
            decoder: d.decoder_utilization().ok().map(|u| u.utilization as f32),
        });
        // Each process's latest share since the last reading.
        let since = seen.get(&index).copied();
        if let Ok(samples) = d.process_utilization_stats(since) {
            let mut latest: HashMap<u32, (u64, u32)> = HashMap::new();
            for s in &samples {
                let entry = latest.entry(s.pid).or_insert((0, 0));
                if s.timestamp >= entry.0 {
                    *entry = (s.timestamp, s.sm_util.max(s.enc_util).max(s.dec_util));
                }
            }
            if let Some(newest) = samples.iter().map(|s| s.timestamp).max() {
                seen.insert(index, newest);
            }
            for (pid, (_, busy)) in latest {
                uses.entry(pid).or_default().busy += busy as f32;
            }
        }
        // Memory, by graphics and compute processes alike, once each.
        let mut held: HashMap<u32, u64> = HashMap::new();
        let processes = d
            .running_graphics_processes()
            .unwrap_or_default()
            .into_iter()
            .chain(d.running_compute_processes().unwrap_or_default());
        for p in processes {
            if let UsedGpuMemory::Used(bytes) = p.used_gpu_memory {
                let h = held.entry(p.pid).or_default();
                *h = (*h).max(bytes);
            }
        }
        for (pid, bytes) in held {
            uses.entry(pid).or_default().memory += bytes;
        }
    }
    gpus
}

/// The DRM cards: each one's device folder in sysfs, driver and PCI
/// address.
fn cards() -> Vec<(PathBuf, String, String)> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut cards: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|e| {
            let device = e.path().join("device");
            let link_name = |p: &Path| {
                std::fs::read_link(p)
                    .ok()?
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            };
            let driver = link_name(&device.join("driver"))?;
            let address = link_name(&device)?;
            Some((device, driver, address))
        })
        .collect();
    cards.sort_by(|a, b| a.2.cmp(&b.2));
    cards.dedup_by(|a, b| a.2 == b.2);
    cards
}

/// What a DRM card says of itself in sysfs: AMD's busy and memory, and
/// any driver's sensors.
fn card(device: &Path, driver: String, address: String) -> Gpu {
    let read = |name: &str| -> Option<u64> {
        std::fs::read_to_string(device.join(name))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    let hex = |name: &str| {
        std::fs::read_to_string(device.join(name))
            .ok()
            .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
    };
    let name = match (hex("vendor"), hex("device")) {
        (Some(v), Some(d)) => pci_name(v, d),
        _ => None,
    }
    .unwrap_or_else(|| format!("GPU ({driver})"));
    let hwmon = std::fs::read_dir(device.join("hwmon"))
        .ok()
        .and_then(|mut d| d.next())
        .and_then(|e| e.ok())
        .map(|e| e.path());
    let sensor = |name: &str| -> Option<u64> {
        std::fs::read_to_string(hwmon.as_ref()?.join(name))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    Gpu {
        name,
        driver,
        address,
        busy: read("gpu_busy_percent").map(|b| b as f32),
        memory_used: read("mem_info_vram_used"),
        memory_total: read("mem_info_vram_total"),
        temperature: sensor("temp1_input").map(|m| (m / 1000) as u32),
        power: sensor("power1_average")
            .or_else(|| sensor("power1_input"))
            .map(|uw| uw as f64 / 1e6),
        power_limit: sensor("power1_cap").map(|uw| uw as f64 / 1e6),
        clock: sensor("freq1_input").map(|hz| (hz / 1_000_000) as u32),
        memory_clock: sensor("freq2_input").map(|hz| (hz / 1_000_000) as u32),
        fan: None,
        encoder: None,
        decoder: None,
    }
}

/// A PCI device's name from the system's pci.ids, with its maker's.
fn pci_name(vendor: u16, device: u16) -> Option<String> {
    let text = [
        "/usr/share/hwdata/pci.ids",
        "/usr/share/misc/pci.ids",
        "/usr/share/pci.ids",
        "/usr/local/share/pciids/pci.ids",
    ]
    .iter()
    .find_map(|p| std::fs::read_to_string(p).ok())?;
    lookup_pci(&text, vendor, device)
}

fn lookup_pci(text: &str, vendor: u16, device: u16) -> Option<String> {
    let maker = match vendor {
        0x1002 => "AMD",
        0x8086 => "Intel",
        0x10de => "NVIDIA",
        _ => "",
    };
    let vendor = format!("{vendor:04x}  ");
    let device = format!("\t{device:04x}  ");
    let mut lines = text.lines().skip_while(|l| !l.starts_with(&vendor)).skip(1);
    let name = lines
        .by_ref()
        .take_while(|l| l.starts_with('\t') || l.starts_with('#'))
        .find_map(|l| l.strip_prefix(&device))?;
    Some(if maker.is_empty() {
        name.to_string()
    } else {
        format!("{maker} {name}")
    })
}

/// A DRM client: one process's use of one GPU, as its fdinfo counts it.
#[derive(Debug, Clone, Default, PartialEq)]
struct Client {
    id: String,
    address: String,
    /// Nanoseconds busy on each engine, since it opened the GPU.
    engines: HashMap<String, u64>,
    /// Cycles busy and cycles passed on each engine (xe).
    cycles: HashMap<String, (u64, u64)>,
    memory: u64,
}

impl Client {
    /// Its busiest engine since `before`, `seconds` ago, 0 to 100.
    fn busy_since(&self, before: &Client, seconds: f64) -> f32 {
        let by_time = self.engines.iter().filter_map(|(engine, ns)| {
            let was = before.engines.get(engine)?;
            Some(ns.saturating_sub(*was) as f64 / (seconds * 1e9))
        });
        let by_cycles = self.cycles.iter().filter_map(|(engine, (busy, total))| {
            let (was_busy, was_total) = before.cycles.get(engine)?;
            let passed = total.saturating_sub(*was_total);
            (passed > 0).then(|| busy.saturating_sub(*was_busy) as f64 / passed as f64)
        });
        (by_time.chain(by_cycles).fold(0.0, f64::max) * 100.0).min(100.0) as f32
    }
}

/// `fdinfo`'s text, if it's a DRM client's.
fn parse_client(text: &str) -> Option<Client> {
    let mut client = Client::default();
    let mut drm = false;
    let mut cycles: HashMap<String, u64> = HashMap::new();
    let mut total_cycles: HashMap<String, u64> = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let number = || value.split_whitespace().next()?.parse::<u64>().ok();
        match key {
            "drm-driver" => drm = true,
            "drm-client-id" => client.id = value.to_string(),
            "drm-pdev" => client.address = value.to_string(),
            _ => {
                if let Some(engine) = key.strip_prefix("drm-engine-") {
                    // Not drm-engine-capacity-*, which counts engines.
                    if !engine.starts_with("capacity-")
                        && let Some(ns) = number()
                    {
                        client.engines.insert(engine.to_string(), ns);
                    }
                } else if let Some(engine) = key.strip_prefix("drm-total-cycles-") {
                    if let Some(n) = number() {
                        total_cycles.insert(engine.to_string(), n);
                    }
                } else if let Some(engine) = key.strip_prefix("drm-cycles-") {
                    if let Some(n) = number() {
                        cycles.insert(engine.to_string(), n);
                    }
                } else if is_device_memory(key) {
                    client.memory += memory(value);
                }
            }
        }
    }
    for (engine, busy) in cycles {
        if let Some(total) = total_cycles.get(&engine) {
            client.cycles.insert(engine, (busy, *total));
        }
    }
    drm.then_some(client)
}

/// The memory keys that count the GPU's own memory, held by the client:
/// amdgpu's `drm-memory-vram`, and the common `drm-resident-vram0` or
/// `drm-resident-local0`. Not system memory, shared or merely reserved.
fn is_device_memory(key: &str) -> bool {
    key == "drm-memory-vram"
        || key
            .strip_prefix("drm-resident-")
            .is_some_and(|region| region.starts_with("vram") || region.starts_with("local"))
}

/// "123 KiB", "4 MiB" or bytes, in bytes.
fn memory(value: &str) -> u64 {
    let mut parts = value.split_whitespace();
    let n: u64 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    match parts.next() {
        Some("KiB") => n * 1024,
        Some("MiB") => n * 1024 * 1024,
        Some("GiB") => n * 1024 * 1024 * 1024,
        _ => n,
    }
}

/// Every DRM client of every process that can be read: each process's
/// files open on `/dev/dri`, each client once (a client's file may be
/// open twice).
fn clients() -> Vec<(u32, Client)> {
    let mut found = Vec::new();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return found;
    };
    for entry in procs.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        let mut ids = Vec::new();
        for fd in fds.flatten() {
            let on_gpu = std::fs::read_link(fd.path()).is_ok_and(|t| t.starts_with("/dev/dri"));
            if !on_gpu {
                continue;
            }
            let info = entry.path().join("fdinfo").join(fd.file_name());
            let Some(client) = std::fs::read_to_string(info)
                .ok()
                .and_then(|t| parse_client(&t))
            else {
                continue;
            };
            if !client.id.is_empty() && !ids.contains(&client.id) {
                ids.push(client.id.clone());
                found.push((pid, client));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMDGPU: &str = "pos:\t0\nflags:\t02100002\nmnt_id:\t26\nino:\t1004\n\
        drm-driver:\tamdgpu\ndrm-client-id:\t12\ndrm-pdev:\t0000:03:00.0\n\
        drm-memory-vram:\t204800 KiB\ndrm-memory-gtt:\t4096 KiB\n\
        drm-engine-gfx:\t1000000000 ns\ndrm-engine-compute:\t0 ns\ndrm-engine-dec:\t5 ns\n";
    const XE: &str = "drm-driver:\txe\ndrm-client-id:\t3\ndrm-pdev:\t0000:00:02.0\n\
        drm-resident-vram0:\t8 MiB\ndrm-total-system:\t100 MiB\n\
        drm-cycles-rcs:\t1000\ndrm-total-cycles-rcs:\t10000\n\
        drm-engine-capacity-vcs:\t2\n";

    #[test]
    fn an_amdgpu_client_counts_engine_time_and_vram() {
        let c = parse_client(AMDGPU).unwrap();
        assert_eq!(c.id, "12");
        assert_eq!(c.address, "0000:03:00.0");
        assert_eq!(c.memory, 200 * 1024 * 1024);
        assert_eq!(c.engines["gfx"], 1_000_000_000);
        // Half a second on gfx in one second is half busy.
        let mut later = c.clone();
        later.engines.insert("gfx".into(), 1_500_000_000);
        assert_eq!(later.busy_since(&c, 1.0), 50.0);
        assert_eq!(c.busy_since(&c, 1.0), 0.0);
    }

    #[test]
    fn an_xe_client_counts_cycles_and_local_memory_only() {
        let c = parse_client(XE).unwrap();
        assert_eq!(c.memory, 8 * 1024 * 1024);
        assert!(c.engines.is_empty(), "capacity isn't an engine");
        let mut later = c.clone();
        later.cycles.insert("rcs".into(), (1250, 11000));
        assert_eq!(later.busy_since(&c, 1.0), 25.0);
    }

    #[test]
    fn a_file_that_isnt_the_gpus_is_no_client() {
        assert_eq!(parse_client("pos:\t0\nflags:\t02\n"), None);
    }

    #[test]
    fn a_pci_device_is_named_from_pci_ids() {
        let ids = "# comment\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\
            \t73bf  Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]\n\
            \t\t1002 0e3a  Radeon RX 6900 XT\n\
            1022  Advanced Micro Devices, Inc. [AMD]\n\t73bf  Not this one\n";
        assert_eq!(
            lookup_pci(ids, 0x1002, 0x73bf).as_deref(),
            Some("AMD Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]")
        );
        assert_eq!(lookup_pci(ids, 0x1002, 0x1234), None);
    }

    /// Reads the GPUs here, and prints them: `cargo test -p patina-processes
    /// gpus_here -- --ignored --nocapture`.
    #[test]
    #[ignore = "depends on the machine"]
    fn gpus_here() {
        let mut reader = Reader::new();
        reader.read();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let (gpus, uses) = reader.read();
        println!("{gpus:#?}");
        let mut uses: Vec<_> = uses
            .into_iter()
            .filter(|(_, u)| u.memory > 0 || u.busy > 0.0)
            .collect();
        uses.sort_by_key(|(pid, _)| *pid);
        println!("{uses:?}");
        assert!(!gpus.is_empty());
    }
}

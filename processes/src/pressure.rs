//! Pressure stall information (ADR-0090): how much of the time programs
//! were held up waiting for the CPU, memory or the disks, from Linux's
//! `/proc/pressure`. The surest sign of a machine that feels stuck: a busy
//! CPU is fine, programs waiting on it are not.

/// One resource's pressure: the share of time, 0 to 100, over the last
/// 10 seconds, minute and 5 minutes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stall {
    /// Some programs were waiting.
    pub some: [f64; 3],
    /// Every program that wanted to run was waiting; not for the CPU.
    pub full: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pressure {
    pub cpu: Stall,
    pub memory: Stall,
    pub io: Stall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cpu,
    Memory,
    Io,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Cpu, Kind::Memory, Kind::Io];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Cpu => "CPU",
            Kind::Memory => "Memory",
            Kind::Io => "Disk",
        }
    }

    /// What programs were waiting for, in a sentence.
    pub fn waiting_for(self) -> &'static str {
        match self {
            Kind::Cpu => "the CPU",
            Kind::Memory => "memory",
            Kind::Io => "the disks",
        }
    }

    /// Over the last minute, more than this is worth a warning: a little
    /// CPU waiting is normal on a busy machine, waiting for memory or the
    /// disks much less so.
    pub fn worrying(self) -> f64 {
        match self {
            Kind::Cpu => 25.0,
            Kind::Memory => 5.0,
            Kind::Io => 10.0,
        }
    }
}

impl Pressure {
    pub fn of(&self, kind: Kind) -> &Stall {
        match kind {
            Kind::Cpu => &self.cpu,
            Kind::Memory => &self.memory,
            Kind::Io => &self.io,
        }
    }
}

/// The machine's pressure, where the kernel keeps it.
pub fn read() -> Option<Pressure> {
    let one = |name: &str| {
        std::fs::read_to_string(format!("/proc/pressure/{name}"))
            .ok()
            .and_then(|t| parse(&t))
    };
    Some(Pressure {
        cpu: one("cpu")?,
        memory: one("memory")?,
        io: one("io")?,
    })
}

fn parse(text: &str) -> Option<Stall> {
    let averages = |line: &str| -> Option<[f64; 3]> {
        let value = |key: &str| {
            line.split_whitespace()
                .find_map(|f| f.strip_prefix(key)?.parse::<f64>().ok())
        };
        Some([value("avg10=")?, value("avg60=")?, value("avg300=")?])
    };
    let line = |kind: &str| text.lines().find(|l| l.starts_with(kind));
    Some(Stall {
        some: averages(line("some ")?)?,
        full: line("full ").and_then(averages),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kernels_averages() {
        let text = "some avg10=6.02 avg60=9.81 avg300=12.25 total=2500382968\n\
                    full avg10=5.48 avg60=8.36 avg300=10.33 total=2240713259\n";
        let s = parse(text).unwrap();
        assert_eq!(s.some, [6.02, 9.81, 12.25]);
        assert_eq!(s.full, Some([5.48, 8.36, 10.33]));
        assert_eq!(
            parse("some avg10=1.00 avg60=2.00 avg300=3.00 total=1\n")
                .unwrap()
                .full,
            None
        );
        assert_eq!(parse("nonsense"), None);
    }
}

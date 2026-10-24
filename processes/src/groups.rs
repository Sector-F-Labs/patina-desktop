//! Processes as the Processes view lists them (ADR-0088): each app, the
//! process owning windows with every process it started, counted together;
//! then the rest, in the background. And the order they're sorted in.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use patina_system::windows::Window;

use crate::sample::Proc;

/// An app: a process with windows, and what it started.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub pid: u32,
    pub app_id: String,
    /// Its windows' titles, in the bar's order.
    pub titles: Vec<String>,
    /// Its own process first, then those it started, depth first, each
    /// with how deep it is under the app's (1 for its children).
    pub members: Vec<(u32, usize)>,
    pub totals: Totals,
}

/// A process's or an app's use of the machine.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Totals {
    pub cpu: f32,
    pub memory: u64,
    /// Bytes read and written per second.
    pub disk: f64,
    /// Busy on the GPUs, 0 to 100 a GPU, and their memory held.
    pub gpu: f32,
    pub gpu_memory: u64,
}

impl Totals {
    pub fn of(p: &Proc) -> Self {
        Self {
            cpu: p.cpu,
            memory: p.memory,
            disk: p.read + p.written,
            gpu: p.gpu,
            gpu_memory: p.gpu_memory,
        }
    }

    fn add(&mut self, other: Self) {
        self.cpu += other.cpu;
        self.memory += other.memory;
        self.disk += other.disk;
        self.gpu += other.gpu;
        self.gpu_memory += other.gpu_memory;
    }
}

/// The apps among `processes`, in the windows' order, and the pids of
/// every process in one. A process with windows started by another (a
/// browser from a terminal) is an app of its own, not part of that one.
pub fn apps(processes: &[Proc], windows: &[Window]) -> (Vec<App>, HashSet<u32>) {
    let by_pid: HashMap<u32, &Proc> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in processes {
        if let Some(parent) = p.parent {
            children.entry(parent).or_default().push(p.pid);
        }
    }
    for list in children.values_mut() {
        list.sort_unstable();
    }
    let with_windows: HashSet<u32> = windows
        .iter()
        .map(|w| w.pid)
        .filter(|pid| by_pid.contains_key(pid))
        .collect();

    let mut apps: Vec<App> = Vec::new();
    let mut grouped = HashSet::new();
    for w in windows {
        if !with_windows.contains(&w.pid) {
            continue;
        }
        if let Some(app) = apps.iter_mut().find(|a| a.pid == w.pid) {
            app.titles.push(w.title.clone());
            continue;
        }
        let mut members = Vec::new();
        let mut stack = vec![(w.pid, 0)];
        while let Some((pid, depth)) = stack.pop() {
            if depth > 0 && with_windows.contains(&pid) {
                continue;
            }
            // A loop in the parents (pid reuse in between readings).
            if !grouped.insert(pid) {
                continue;
            }
            members.push((pid, depth));
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().rev().map(|&k| (k, depth + 1)));
            }
        }
        let mut totals = Totals::default();
        for (pid, _) in &members {
            if let Some(p) = by_pid.get(pid) {
                totals.add(Totals::of(p));
            }
        }
        apps.push(App {
            pid: w.pid,
            app_id: w.app_id.clone(),
            titles: vec![w.title.clone()],
            members,
            totals,
        });
    }
    (apps, grouped)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Name,
    Pid,
    User,
    Cpu,
    Memory,
    Disk,
    Gpu,
}

impl Column {
    pub const ALL: [Column; 7] = [
        Column::Name,
        Column::Pid,
        Column::User,
        Column::Cpu,
        Column::Memory,
        Column::Disk,
        Column::Gpu,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Column::Name => "Name",
            Column::Pid => "PID",
            Column::User => "User",
            Column::Cpu => "CPU",
            Column::Memory => "Memory",
            Column::Disk => "Disk",
            Column::Gpu => "GPU",
        }
    }

    /// Numbers start with the most; words from A.
    pub fn descends_first(self) -> bool {
        matches!(
            self,
            Column::Cpu | Column::Memory | Column::Disk | Column::Gpu
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            column: Column::Cpu,
            descending: true,
        }
    }
}

impl Sort {
    /// Sorting by `column`: the other way if it already is, else its
    /// first way.
    pub fn by(self, column: Column) -> Self {
        if self.column == column {
            Self {
                column,
                descending: !self.descending,
            }
        } else {
            Self {
                column,
                descending: column.descends_first(),
            }
        }
    }

    /// `a` against `b`, each with its name and use; ties by PID, so rows
    /// don't swap places between readings.
    pub fn compare(
        self,
        (a, a_name, a_use): (&Proc, &str, Totals),
        (b, b_name, b_use): (&Proc, &str, Totals),
    ) -> Ordering {
        let order = match self.column {
            Column::Name => a_name.to_lowercase().cmp(&b_name.to_lowercase()),
            Column::Pid => a.pid.cmp(&b.pid),
            Column::User => a.user.cmp(&b.user),
            Column::Cpu => a_use.cpu.total_cmp(&b_use.cpu),
            Column::Memory => a_use.memory.cmp(&b_use.memory),
            Column::Disk => a_use.disk.total_cmp(&b_use.disk),
            Column::Gpu => a_use.gpu.total_cmp(&b_use.gpu),
        };
        let order = if self.descending {
            order.reverse()
        } else {
            order
        };
        order.then(a.pid.cmp(&b.pid))
    }
}

/// Whether `p` is what `filter` (lower case) looks for: its name, command
/// line, user or PID.
pub fn matches(p: &Proc, filter: &str) -> bool {
    filter.is_empty()
        || p.name.to_lowercase().contains(filter)
        || p.command.to_lowercase().contains(filter)
        || p.user.to_lowercase() == filter
        || p.pid.to_string() == filter
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::State;

    fn proc(pid: u32, parent: u32, name: &str, cpu: f32, memory: u64) -> Proc {
        Proc {
            pid,
            parent: Some(parent),
            name: name.into(),
            command: format!("/usr/bin/{name} --flag"),
            user: "divan".into(),
            uid: Some(1000),
            cpu,
            memory,
            read: 0.0,
            written: 0.0,
            state: State::Sleeping,
            started: 0,
            kernel: false,
            gpu: 0.0,
            gpu_memory: 0,
        }
    }

    fn window(pid: u32, app_id: &str, title: &str) -> Window {
        Window {
            app_id: app_id.into(),
            title: title.into(),
            pid,
        }
    }

    /// The compositor (10) started a terminal (20) running a shell (21),
    /// which started a browser (30) with two content processes (31, 32);
    /// and a daemon (40) runs beside them.
    fn machine() -> Vec<Proc> {
        vec![
            proc(10, 1, "patina-compositor", 2.0, 100),
            proc(20, 10, "teleprinter", 1.0, 50),
            proc(21, 20, "zsh", 0.0, 5),
            proc(30, 21, "firefox", 5.0, 500),
            proc(31, 30, "Isolated Web Co", 10.0, 300),
            proc(32, 30, "Isolated Web Co", 1.0, 200),
            proc(40, 1, "pipewire", 0.5, 20),
        ]
    }

    #[test]
    fn an_app_is_its_process_and_what_it_started_but_not_other_apps() {
        let windows = [
            window(20, "teleprinter", "~"),
            window(30, "firefox", "Mozilla Firefox"),
            window(30, "firefox", "Private Browsing"),
        ];
        let (apps, grouped) = apps(&machine(), &windows);
        assert_eq!(apps.len(), 2);
        let (term, browser) = (&apps[0], &apps[1]);
        assert_eq!(term.members, [(20, 0), (21, 1)]);
        assert_eq!(term.totals.memory, 55);
        assert_eq!(browser.titles, ["Mozilla Firefox", "Private Browsing"]);
        assert_eq!(browser.members, [(30, 0), (31, 1), (32, 1)]);
        assert_eq!(browser.totals.cpu, 16.0);
        assert_eq!(browser.totals.memory, 1000);
        let mut grouped: Vec<_> = grouped.into_iter().collect();
        grouped.sort_unstable();
        assert_eq!(grouped, [20, 21, 30, 31, 32]);
    }

    #[test]
    fn a_window_whose_process_is_gone_is_no_app() {
        let (apps, grouped) = apps(&machine(), &[window(99, "ghost", "")]);
        assert!(apps.is_empty() && grouped.is_empty());
    }

    #[test]
    fn a_loop_in_the_parents_ends() {
        let mut ps = machine();
        ps[0].parent = Some(32);
        let (apps, _) = apps(&ps, &[window(30, "firefox", "")]);
        assert_eq!(apps[0].members.len(), 6);
    }

    #[test]
    fn sorting_again_by_a_column_turns_it_round() {
        let s = Sort::default().by(Column::Name);
        assert!(!s.descending);
        assert!(s.by(Column::Name).descending);
        assert!(s.by(Column::Memory).descending);
    }

    #[test]
    fn ties_go_by_pid_either_way() {
        let ps = machine();
        let (a, b) = (&ps[4], &ps[5]);
        let same = Totals::default();
        for descending in [false, true] {
            let s = Sort {
                column: Column::Name,
                descending,
            };
            assert_eq!(
                s.compare((a, &a.name, same), (b, &b.name, same)),
                Ordering::Less
            );
        }
    }

    #[test]
    fn the_filter_looks_at_name_command_user_and_pid() {
        let p = &machine()[3];
        assert!(matches(p, "fire"));
        assert!(matches(p, "--flag"));
        assert!(matches(p, "divan"));
        assert!(matches(p, "30"));
        assert!(!matches(p, "3"));
        assert!(!matches(p, "chrome"));
    }
}

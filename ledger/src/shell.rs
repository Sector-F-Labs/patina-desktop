//! Shell formulas: `=SH("df -h", :F20, sep=",", skip=1, rest=drop, arg=A1)`.
//!
//! The command's output fills a block of cells whose top-left cell is the
//! formula's own. This module runs commands and works out where their output
//! lands; `formula.rs` calls it. See `docs/adr/0006`.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::formula::Pos;

/// How long a command may run unless the formula says `timeout=`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// The most output kept from one command.
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
/// The most rows and columns one block may take.
const MAX_BLOCK_ROWS: usize = 100_000;
const MAX_BLOCK_COLS: usize = 1_000;

/// How a run ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Ok,
    Exit(i32),
    Timeout,
    /// The command couldn't be started.
    Failed(String),
}

impl Status {
    /// For the file: `ok`, `exit:2`, `timeout`, `failed:message`.
    pub fn encode(&self) -> String {
        match self {
            Status::Ok => "ok".into(),
            Status::Exit(n) => format!("exit:{n}"),
            Status::Timeout => "timeout".into(),
            Status::Failed(m) => format!("failed:{m}"),
        }
    }

    pub fn decode(text: &str) -> Self {
        match text {
            "ok" => Status::Ok,
            "timeout" => Status::Timeout,
            t if t.starts_with("exit:") => Status::Exit(t[5..].parse().unwrap_or(-1)),
            t => Status::Failed(t.strip_prefix("failed:").unwrap_or(t).to_string()),
        }
    }
}

/// What a command printed, kept in the file so a reload shows it without
/// running the command again.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The formula this is the result of. A result for another formula is stale.
    pub formula: String,
    pub status: Status,
    pub stdout: String,
    pub stderr: String,
    /// Seconds since the Unix epoch.
    pub ran_at: i64,
}

/// What to run, worked out from the formula and the cells it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub formula: String,
    pub command: String,
    pub args: Vec<String>,
    pub timeout: Duration,
}

/// Runs a job with `sh -c`, the arguments as `$1`, `$2`, ..., never inside the
/// command's text. Blocks until it ends or times out: call it from a thread.
pub fn run(job: &Job) -> Run {
    let ran_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let done = |status, stdout, stderr| Run { formula: job.formula.clone(), status, stdout, stderr, ran_at };
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(&job.command)
        .arg("sh")
        .args(&job.args)
        // Its own process group, so a timeout can stop what the command started too.
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return done(Status::Failed(e.to_string()), String::new(), String::new()),
    };
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe {
                let _ = p.take(MAX_OUTPUT as u64).read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break if s.success() { Status::Ok } else { Status::Exit(s.code().unwrap_or(-1)) },
            Ok(None) if start.elapsed() > job.timeout => {
                // SAFETY: kill(2) with a process group id we created.
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.kill();
                let _ = child.wait();
                break Status::Timeout;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => break Status::Failed(e.to_string()),
        }
    };
    done(status, out.join().unwrap_or_default(), err.join().unwrap_or_default())
}

// --- Where the output goes ---------------------------------------------

/// Where the formula says the output goes. The start is always the formula's
/// own cell; an end that's missing is wherever the output ends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Target {
    pub end_col: Option<u32>,
    pub end_row: Option<u32>,
}

/// How lines are split into cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// `None`: runs of whitespace.
    pub sep: Option<String>,
    /// Lines to leave out from the top.
    pub skip: usize,
    /// More fields than columns: keep them in the last column (`true`) or drop them.
    pub keep_rest: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { sep: None, skip: 0, keep_rest: true }
    }
}

/// The block a command's output fills.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub owner: Pos,
    pub rows: usize,
    pub cols: usize,
    /// Output lines that didn't fit the end row.
    pub cut_lines: usize,
    /// The most fields a line had beyond the last column (`rest=drop` only).
    pub cut_fields: usize,
    /// The cells, `rows` by `cols`; short lines are padded with empty text.
    pub cells: Vec<Vec<String>>,
}

impl Block {
    /// The block's last row and column.
    pub fn end(&self) -> Pos {
        (self.owner.0 + self.rows.max(1) as u32 - 1, self.owner.1 + self.cols.max(1) as u32 - 1)
    }

    pub fn contains(&self, (r, c): Pos) -> bool {
        let (er, ec) = self.end();
        (self.owner.0..=er).contains(&r) && (self.owner.1..=ec).contains(&c)
    }

    pub fn get(&self, (r, c): Pos) -> Option<&str> {
        let row = self.cells.get(r.checked_sub(self.owner.0)? as usize)?;
        row.get(c.checked_sub(self.owner.1)? as usize).map(String::as_str)
    }
}

/// Splits one line into at most `cols` fields (`None`: as many as there are).
/// Returns the fields and how many more there were than fit.
fn split_line(line: &str, opts: &Options, cols: Option<usize>) -> (Vec<String>, usize) {
    let mut fields: Vec<(usize, &str)> = Vec::new(); // start offset, text
    match &opts.sep {
        Some(sep) if !sep.is_empty() => {
            let mut at = 0;
            for part in line.split(sep.as_str()) {
                fields.push((at, part));
                at += part.len() + sep.len();
            }
        }
        _ => {
            let mut rest = line;
            let mut at = 0;
            while let Some(start) = rest.find(|c: char| !c.is_whitespace()) {
                let after = &rest[start..];
                let len = after.find(char::is_whitespace).unwrap_or(after.len());
                fields.push((at + start, &after[..len]));
                at += start + len;
                rest = &line[at..];
            }
        }
    }
    let Some(cols) = cols else {
        return (fields.iter().map(|(_, f)| f.trim().to_string()).collect(), 0);
    };
    let extra = fields.len().saturating_sub(cols);
    let mut out: Vec<String> = fields.iter().take(cols).map(|(_, f)| f.trim().to_string()).collect();
    if extra > 0 && opts.keep_rest && cols > 0 {
        // The last column takes the rest of the line, as it was written.
        out[cols - 1] = line[fields[cols - 1].0..].trim().to_string();
        return (out, 0);
    }
    (out, extra)
}

/// Works out the block for some output.
pub fn layout(owner: Pos, target: Target, opts: &Options, stdout: &str) -> Block {
    let lines: Vec<&str> = stdout.lines().skip(opts.skip).collect();
    let fixed_cols = target.end_col.map(|c| (c.saturating_sub(owner.1) as usize + 1).min(MAX_BLOCK_COLS));
    let max_rows = target.end_row.map(|r| (r.saturating_sub(owner.0) as usize + 1).min(MAX_BLOCK_ROWS));
    let shown = max_rows.map_or(lines.len(), |m| lines.len().min(m)).min(MAX_BLOCK_ROWS);
    let mut cut_fields = 0;
    let mut cells: Vec<Vec<String>> = Vec::with_capacity(shown);
    for line in &lines[..shown] {
        let (fields, extra) = split_line(line, opts, fixed_cols.or(Some(MAX_BLOCK_COLS)));
        cut_fields = cut_fields.max(extra);
        cells.push(fields);
    }
    let cols = fixed_cols.unwrap_or_else(|| cells.iter().map(Vec::len).max().unwrap_or(0));
    for row in &mut cells {
        row.resize(cols, String::new());
    }
    Block { owner, rows: shown, cols, cut_lines: lines.len() - shown, cut_fields, cells }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DF: &str = "Filesystem Size Used Mounted on\n/dev/sda1 100G 40G /\ntmpfs 8G 0 /run/user 1000\n";

    fn block(target: Target, opts: &Options) -> Block {
        layout((1, 1), target, opts, DF)
    }

    #[test]
    fn whitespace_columns_default_to_the_widest_line() {
        let b = block(Target::default(), &Options::default());
        assert_eq!((b.rows, b.cols, b.cut_lines, b.cut_fields), (3, 5, 0, 0));
        assert_eq!(b.cells[1], ["/dev/sda1", "100G", "40G", "/", ""]);
        assert_eq!(b.end(), (3, 5));
        assert!(b.contains((2, 3)) && !b.contains((2, 6)) && !b.contains((0, 1)));
        assert_eq!(b.get((2, 1)), Some("/dev/sda1"));
    }

    #[test]
    fn the_last_column_keeps_the_rest_of_the_line() {
        // Columns B to E: the fifth field joins the fourth, as written.
        let b = block(Target { end_col: Some(4), end_row: None }, &Options::default());
        assert_eq!((b.cols, b.cut_fields), (4, 0));
        assert_eq!(b.cells[2], ["tmpfs", "8G", "0", "/run/user 1000"]);
    }

    #[test]
    fn dropping_the_rest_is_counted() {
        let opts = Options { keep_rest: false, ..Default::default() };
        let b = block(Target { end_col: Some(4), end_row: None }, &opts);
        assert_eq!(b.cells[2], ["tmpfs", "8G", "0", "/run/user"]);
        assert_eq!(b.cut_fields, 1);
    }

    #[test]
    fn rows_past_the_end_are_counted() {
        let b = block(Target { end_col: None, end_row: Some(2) }, &Options::default());
        assert_eq!((b.rows, b.cut_lines), (2, 1));
        let b = block(Target { end_col: None, end_row: Some(50) }, &Options::default());
        assert_eq!((b.rows, b.cut_lines), (3, 0));
    }

    #[test]
    fn skip_and_separator() {
        let opts = Options { sep: Some(",".into()), skip: 1, keep_rest: true };
        let b = layout((0, 0), Target::default(), &opts, "a,b\n1,2,3\n4,5\n");
        assert_eq!(b.cells, vec![vec!["1", "2", "3"], vec!["4", "5", ""]]);
    }

    #[test]
    fn empty_output_is_an_empty_block() {
        let b = layout((0, 0), Target::default(), &Options::default(), "");
        assert_eq!((b.rows, b.cols), (0, 0));
    }

    #[test]
    fn status_round_trips() {
        for s in [Status::Ok, Status::Exit(3), Status::Timeout, Status::Failed("no such: file".into())] {
            assert_eq!(Status::decode(&s.encode()), s);
        }
    }

    fn job(command: &str, args: &[&str], timeout: Duration) -> Job {
        Job { formula: String::new(), command: command.into(), args: args.iter().map(|s| s.to_string()).collect(), timeout }
    }

    #[test]
    fn running_a_command() {
        let r = run(&job("printf '%s\\n' \"$1\" \"$2\"", &["a b", "; echo injected"], DEFAULT_TIMEOUT));
        assert_eq!(r.status, Status::Ok);
        assert_eq!(r.stdout, "a b\n; echo injected\n", "arguments are data, not shell text");
        let r = run(&job("echo oops >&2; exit 3", &[], DEFAULT_TIMEOUT));
        assert_eq!((r.status, r.stderr.trim()), (Status::Exit(3), "oops"));
    }

    #[test]
    fn a_slow_command_times_out() {
        let t = Instant::now();
        let r = run(&job("sleep 5", &[], Duration::from_millis(100)));
        assert_eq!(r.status, Status::Timeout);
        assert!(t.elapsed() < Duration::from_secs(3));
    }
}

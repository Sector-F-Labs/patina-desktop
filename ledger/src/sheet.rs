//! One sheet: a tab of cells. Holds the inputs (what's stored) and the values
//! worked out from them (what's shown).

use std::collections::{BTreeMap, HashMap};

use crate::formula::{Evaluator, Pos, ShellState, Value};
use crate::shell::{Block, Job, Run};

pub struct Sheet {
    pub id: i64,
    pub name: String,
    cells: BTreeMap<Pos, String>,
    values: HashMap<Pos, Value>,
    shell: ShellState,
    blocks: Vec<Block>,
    conflicts: std::collections::HashSet<Pos>,
    dirty: bool,
}

impl Sheet {
    pub fn new(id: i64, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            cells: BTreeMap::new(),
            values: HashMap::new(),
            shell: ShellState::default(),
            blocks: Vec::new(),
            conflicts: Default::default(),
            dirty: false,
        }
    }

    /// A sheet read from the file, with its values worked out.
    pub fn with_cells(
        id: i64,
        name: impl Into<String>,
        cells: BTreeMap<Pos, String>,
        runs: HashMap<Pos, Run>,
    ) -> Self {
        let mut s = Self::new(id, name);
        s.cells = cells;
        s.shell.runs = runs;
        s.dirty = true;
        s.recalc();
        s
    }

    /// What was typed in the cell; empty if nothing.
    pub fn input(&self, pos: Pos) -> &str {
        self.cells.get(&pos).map(String::as_str).unwrap_or("")
    }

    pub fn value(&self, pos: Pos) -> Value {
        self.values.get(&pos).cloned().unwrap_or(Value::Empty)
    }

    /// Sets a cell's input; empty clears it. Returns whether anything changed.
    pub fn set(&mut self, pos: Pos, input: &str) -> bool {
        if self.input(pos) == input {
            return false;
        }
        if input.trim().is_empty() {
            self.cells.remove(&pos);
            self.shell.runs.remove(&pos);
        } else {
            self.cells.insert(pos, input.to_string());
        }
        self.dirty = true;
        true
    }

    /// Works the values out again if an input changed since the last time.
    pub fn recalc(&mut self) {
        if self.dirty {
            let done = Evaluator::with_shell(&self.cells, &self.shell).all();
            self.values = done.values;
            self.blocks = done.blocks;
            self.conflicts = done.conflicts;
            self.dirty = false;
        }
    }

    /// The cells that hold shell formulas.
    pub fn shell_cells(&self) -> Vec<Pos> {
        self.cells.iter().filter(|(_, i)| is_shell(i)).map(|(p, _)| *p).collect()
    }

    /// What to run for the shell formula in a cell, as the cells it reads are now.
    pub fn shell_job(&self, pos: Pos) -> Result<Job, String> {
        Evaluator::with_shell(&self.cells, &self.shell).shell_job(pos)
    }

    /// Marks a shell cell as running, or not.
    pub fn set_pending(&mut self, pos: Pos, pending: bool) {
        if pending {
            self.shell.pending.insert(pos);
        } else {
            self.shell.pending.remove(&pos);
        }
        self.dirty = true;
    }

    pub fn is_pending(&self, pos: Pos) -> bool {
        self.shell.pending.contains(&pos)
    }

    /// Keeps the result of a command.
    pub fn set_run(&mut self, pos: Pos, run: Run) {
        self.shell.runs.insert(pos, run);
        self.dirty = true;
    }

    pub fn run(&self, pos: Pos) -> Option<&Run> {
        self.shell.runs.get(&pos)
    }

    /// The shell output blocks, for drawing.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The block a cell belongs to, if it's filled by a command.
    pub fn block_at(&self, pos: Pos) -> Option<&Block> {
        self.blocks.iter().find(|b| b.contains(pos))
    }

    /// The last used row and column, plus one each: how much grid the data needs.
    pub fn extent(&self) -> Pos {
        self.cells.keys().fold((0, 0), |(r, c), &(row, col)| (r.max(row + 1), c.max(col + 1)))
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The numbers in a block of cells, for the status bar's sum.
    pub fn numbers_in(&self, (r0, c0): Pos, (r1, c1): Pos) -> Vec<f64> {
        self.values
            .iter()
            .filter(|((r, c), _)| (r0..=r1).contains(r) && (c0..=c1).contains(c))
            .filter_map(|(_, v)| if let Value::Number(n) = v { Some(*n) } else { None })
            .collect()
    }
}

/// Whether a cell's input is a shell formula.
pub fn is_shell(input: &str) -> bool {
    input.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("=SH("))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_recalc_and_clear() {
        let mut s = Sheet::new(1, "Sheet 1");
        assert!(s.set((0, 0), "5"));
        assert!(!s.set((0, 0), "5"));
        s.set((1, 0), "=A1*2");
        s.recalc();
        assert_eq!(s.value((1, 0)), Value::Number(10.0));
        assert_eq!(s.extent(), (2, 1));
        s.set((0, 0), "7");
        s.recalc();
        assert_eq!(s.value((1, 0)), Value::Number(14.0));
        assert!(s.set((0, 0), ""));
        s.recalc();
        assert_eq!(s.value((1, 0)), Value::Number(0.0));
        assert_eq!(s.numbers_in((0, 0), (5, 5)), vec![0.0]);
    }

    #[test]
    fn a_shell_cell_runs_and_fills_its_block() {
        let mut s = Sheet::new(1, "S");
        s.set((0, 0), "Bread");
        s.set((1, 1), "=SH(\"printf 'a 1\\nb 2\\n'; echo \"\"$1\"\" 9\", :C, arg=A1)");
        s.recalc();
        assert_eq!(s.value((1, 1)), Value::Error("NORUN".into()), "nothing runs until asked");
        let job = s.shell_job((1, 1)).unwrap();
        s.set_pending((1, 1), true);
        s.recalc();
        assert_eq!(s.value((1, 1)), Value::Pending);
        let run = crate::shell::run(&job);
        assert_eq!(run.status, crate::shell::Status::Ok, "{run:?}");
        s.set_pending((1, 1), false);
        s.set_run((1, 1), run);
        s.recalc();
        assert_eq!(s.value((1, 1)), Value::Text("a".into()));
        assert_eq!(s.value((1, 2)), Value::Number(1.0));
        assert_eq!(s.value((3, 1)), Value::Text("Bread".into()), "the argument reached the command as data");
        assert!(s.block_at((3, 2)).is_some() && s.block_at((4, 2)).is_none());
        // Clearing the cell takes its result with it.
        s.set((1, 1), "");
        assert!(s.run((1, 1)).is_none());
    }
}

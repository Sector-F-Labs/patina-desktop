//! One sheet: a tab of cells. Holds the inputs (what's stored) and the values
//! worked out from them (what's shown).

use std::collections::{BTreeMap, HashMap};

use crate::formula::{Evaluator, Pos, Value};

pub struct Sheet {
    pub id: i64,
    pub name: String,
    cells: BTreeMap<Pos, String>,
    values: HashMap<Pos, Value>,
    dirty: bool,
}

impl Sheet {
    pub fn new(id: i64, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), cells: BTreeMap::new(), values: HashMap::new(), dirty: false }
    }

    /// A sheet read from the file, with its values worked out.
    pub fn with_cells(id: i64, name: impl Into<String>, cells: BTreeMap<Pos, String>) -> Self {
        let mut s = Self { id, name: name.into(), cells, values: HashMap::new(), dirty: true };
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
        } else {
            self.cells.insert(pos, input.to_string());
        }
        self.dirty = true;
        true
    }

    /// Works the values out again if an input changed since the last time.
    pub fn recalc(&mut self) {
        if self.dirty {
            self.values = Evaluator::new(&self.cells).all();
            self.dirty = false;
        }
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
}

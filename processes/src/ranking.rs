//! A top few that holds still, for the Overview's busiest lists: each one's
//! value averaged over the last few readings, rows kept in their places,
//! one swapped out in place only when an outsider clearly beats it, and the
//! whole list sorted again only when a new one clearly takes the lead. The
//! order isn't exact, but a row doesn't jump while it's being read.

use std::collections::HashMap;
use std::hash::Hash;

/// How much of each new reading goes into the average.
const NEW: f64 = 0.35;
/// How far ahead an outsider, or a new leader, must be.
const MARGIN: f64 = 1.25;
/// Below this, a value counts as nothing.
const NOTHING: f64 = 0.01;

pub struct Ranking<K> {
    size: usize,
    order: Vec<K>,
    average: HashMap<K, f64>,
}

impl<K: Copy + Eq + Hash> Ranking<K> {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            order: Vec::new(),
            average: HashMap::new(),
        }
    }

    /// The top few, in their rows' order.
    pub fn order(&self) -> &[K] {
        &self.order
    }

    /// Takes a reading: each key's value now. Keys not in it are gone.
    pub fn update(&mut self, values: impl IntoIterator<Item = (K, f64)>) {
        let mut average = HashMap::new();
        for (key, value) in values {
            let a = match self.average.get(&key) {
                Some(old) => old * (1.0 - NEW) + value * NEW,
                None => value,
            };
            average.insert(key, a);
        }
        self.average = average;
        let of = |k: &K, avg: &HashMap<K, f64>| avg.get(k).copied().unwrap_or(0.0);

        // The gone, and those down to nothing, leave.
        let avg = &self.average;
        self.order.retain(|k| of(k, avg) > NOTHING);

        let mut outsiders: Vec<K> = self
            .average
            .iter()
            .filter(|(k, v)| **v > NOTHING && !self.order.contains(k))
            .map(|(k, _)| *k)
            .collect();
        outsiders.sort_by(|a, b| of(b, avg).total_cmp(&of(a, avg)));

        // A new leader clearly ahead: sort again from scratch.
        let leader = self
            .order
            .iter()
            .chain(&outsiders)
            .copied()
            .max_by(|a, b| of(a, avg).total_cmp(&of(b, avg)));
        if let (Some(leader), Some(first)) = (leader, self.order.first().copied())
            && leader != first
            && of(&leader, avg) > of(&first, avg) * MARGIN
        {
            let mut all: Vec<K> = self.order.iter().chain(&outsiders).copied().collect();
            all.sort_by(|a, b| of(b, avg).total_cmp(&of(a, avg)));
            all.truncate(self.size);
            self.order = all;
            return;
        }

        // Empty rows filled, at the end, best first.
        let mut outsiders = outsiders.into_iter().peekable();
        while self.order.len() < self.size {
            match outsiders.next() {
                Some(k) => self.order.push(k),
                None => break,
            }
        }
        // Then each outsider clearly ahead of the weakest row takes its
        // place, in that row.
        for k in outsiders {
            let weakest = self
                .order
                .iter()
                .enumerate()
                .skip(1)
                .min_by(|(_, a), (_, b)| of(a, avg).total_cmp(&of(b, avg)));
            match weakest {
                Some((i, w)) if of(&k, avg) > of(w, avg) * MARGIN => self.order[i] = k,
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked(r: &mut Ranking<u32>, values: &[(u32, f64)]) -> Vec<u32> {
        r.update(values.iter().copied());
        r.order().to_vec()
    }

    #[test]
    fn the_first_reading_is_sorted() {
        let mut r = Ranking::new(3);
        assert_eq!(
            ranked(&mut r, &[(1, 5.0), (2, 9.0), (3, 7.0), (4, 1.0)]),
            [2, 3, 1]
        );
    }

    #[test]
    fn rows_hold_still_while_values_jitter() {
        let mut r = Ranking::new(3);
        ranked(&mut r, &[(1, 10.0), (2, 8.0), (3, 6.0)]);
        // 3 passes 2 for a moment: no row moves.
        assert_eq!(ranked(&mut r, &[(1, 10.0), (2, 6.0), (3, 9.0)]), [1, 2, 3]);
        assert_eq!(ranked(&mut r, &[(1, 10.0), (2, 7.0), (3, 8.0)]), [1, 2, 3]);
    }

    #[test]
    fn a_new_leader_sorts_the_list_again() {
        let mut r = Ranking::new(3);
        ranked(&mut r, &[(1, 10.0), (2, 8.0), (3, 6.0)]);
        let mut order = Vec::new();
        for _ in 0..6 {
            order = ranked(&mut r, &[(1, 2.0), (2, 8.0), (3, 40.0)]);
        }
        assert_eq!(order, [3, 2, 1]);
    }

    #[test]
    fn an_outsider_clearly_ahead_takes_the_weakest_row_in_place() {
        let mut r = Ranking::new(3);
        ranked(&mut r, &[(1, 10.0), (2, 3.0), (3, 6.0), (4, 2.0)]);
        // Just ahead of 2 isn't enough.
        assert_eq!(
            ranked(&mut r, &[(1, 10.0), (2, 3.0), (3, 6.0), (4, 3.3)]),
            [1, 3, 2]
        );
        let mut order = Vec::new();
        for _ in 0..6 {
            order = ranked(&mut r, &[(1, 10.0), (2, 3.0), (3, 6.0), (4, 8.0)]);
        }
        // 4 is in 2's row; 3 and 1 haven't moved.
        assert_eq!(order, [1, 3, 4]);
    }

    #[test]
    fn the_gone_and_the_idle_leave_and_rows_fill() {
        let mut r = Ranking::new(3);
        ranked(&mut r, &[(1, 10.0), (2, 8.0), (3, 6.0), (4, 5.0)]);
        assert_eq!(ranked(&mut r, &[(1, 10.0), (3, 6.0), (4, 5.0)]), [1, 3, 4]);
        assert_eq!(ranked(&mut r, &[(1, 0.0)]).len(), 1);
    }
}

//! BOX addition: the fire escapes (docs/fire-escapes.md), worked out from
//! the tower's cells.  The engine draws them and walks the evacuees down
//! them; under BOX RULES the emergency services climb them where no lift,
//! stairs or escalator goes (emergency.rs, the fire stairs' leg).  Nothing
//! here is saved: the escapes stand wherever the floors' ends are.

use crate::world::{WorldState, GRID_HEIGHT, GROUND_Y};

const GRID_W: usize = 375;
const GROUND: usize = (GROUND_Y + 1) as usize;
const GRID_H: usize = GRID_HEIGHT as usize;

/// One fire escape: a stair two cells wide at `x` (its left cell), beside
/// the floors' ends on `side` (-1 their left, 1 their right), its top
/// landing at row `top` and its foot at row `bottom` (the street's row, or
/// a roof)
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Escape {
    pub side: i64,
    pub x: i64,
    pub top: i64,
    pub bottom: i64,
}

impl Escape {
    /// the landing's middle, in cells: where a person stands on it
    pub fn landing(&self) -> f64 {
        self.x as f64 + 1.0
    }
    /// the outer and inner ends of its flights, in cells
    pub fn outer(&self) -> f64 {
        if self.side < 0 { self.x as f64 + 0.3 } else { self.x as f64 + 1.7 }
    }
    pub fn inner(&self) -> f64 {
        if self.side < 0 { self.x as f64 + 1.7 } else { self.x as f64 + 0.3 }
    }
}

/// Each row's runs of built cells, first and last, from the ground floor up
/// (the rows under the street are left empty: no one walks out of them)
pub fn floor_runs(built: &impl Fn(i64, i64) -> bool) -> Vec<Vec<(i64, i64)>> {
    let mut rows = vec![Vec::new(); GRID_H];
    for (y, runs) in rows.iter_mut().enumerate().take(GROUND) {
        let y = y as i64;
        let mut x = 0i64;
        while x < GRID_W as i64 {
            if built(x, y) {
                let x0 = x;
                while x < GRID_W as i64 && built(x, y) {
                    x += 1;
                }
                runs.push((x0, x - 1));
            } else {
                x += 1;
            }
        }
    }
    rows
}

/// The fire escapes: a stair beside each end of each run, a landing on the
/// roof where the floor above steps back, and on down to what is under it
/// (a roof, or the street); then one stair for each unbroken column of them
pub fn fire_escapes(floors: &[Vec<(i64, i64)>], built: &impl Fn(i64, i64) -> bool) -> Vec<Escape> {
    use std::collections::BTreeSet;
    let street = GROUND as i64 - 1;
    let free = |x: i64, y: i64| x >= 0 && x + 1 < GRID_W as i64 && (0..=street).contains(&y) && !built(x, y) && !built(x + 1, y);
    let mut slots: BTreeSet<(i64, i64, i64)> = BTreeSet::new(); // (side, x, row)
    for (y, runs) in floors.iter().enumerate().take(GROUND) {
        let y = y as i64;
        for &(x0, x1) in runs {
            for (side, sx, end) in [(-1, x0 - 2, x0), (1, x1 + 1, x1)] {
                if !free(sx, y) {
                    continue;
                }
                slots.insert((side, sx, y));
                // a step-back: the floor above is narrower here, so a landing
                // on this run's roof for those who come down onto it
                let above = y > 0 && floors[(y - 1) as usize].iter().any(|&(a, b)| a <= x1 && b >= x0);
                if above && !built(end, y - 1) && free(sx, y - 1) {
                    slots.insert((side, sx, y - 1));
                }
            }
        }
    }
    // a stair over nothing goes on down: to a roof, or the street
    let mut more = Vec::new();
    for &(side, x, y) in &slots {
        let mut b = y;
        while b < street && free(x, b + 1) && !slots.contains(&(side, x, b + 1)) {
            b += 1;
            more.push((side, x, b));
        }
    }
    slots.extend(more);
    let mut out: Vec<Escape> = Vec::new();
    for (side, x, y) in slots {
        match out.last_mut() {
            Some(e) if e.side == side && e.x == x && e.bottom + 1 == y => e.bottom = y,
            _ => out.push(Escape { side, x, top: y, bottom: y }),
        }
    }
    out
}

/// The tower's floors' runs and its fire escapes, from its cells
pub fn of(world: &WorldState) -> (Vec<Vec<(i64, i64)>>, Vec<Escape>) {
    let built = |x: i64, y: i64| world.cells.get(&format!("{x},{y}")).is_some_and(|t| !t.is_empty());
    let floors = floor_runs(&built);
    let escapes = fire_escapes(&floors, &built);
    (floors, escapes)
}

/// The escape a person at cell `x` on row `from` takes to row `to`: one
/// whose column spans both rows, beside the run they are on, the nearest
pub fn for_trip(world: &WorldState, x: f64, from: i64, to: i64) -> Option<Escape> {
    let (floors, escapes) = of(world);
    let run = floors.get(from as usize)?.iter().copied().find(|&(a, b)| a as f64 <= x && x <= (b + 1) as f64);
    escapes
        .into_iter()
        .filter(|e| e.top <= from.min(to) && from.max(to) <= e.bottom)
        .filter(|e| match run {
            Some((a, b)) => (e.side < 0 && e.x + 2 == a) || (e.side > 0 && e.x == b + 1),
            None => true,
        })
        .min_by(|a, b| (a.landing() - x).abs().total_cmp(&(b.landing() - x).abs()))
}

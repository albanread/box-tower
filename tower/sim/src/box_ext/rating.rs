//! BOX addition (BOX RULES): how a room's evaluation is made up, for the
//! Room pane (docs/people.md).
//!
//! sims/scoring.rs rates a room from its people's average trip stress, its
//! rent, any noisy neighbour and the tower's amenities, against the star
//! rating's two thresholds.  This takes the same steps without changing
//! anything and keeps each one, and adds how far the room is from the
//! nearest lift or stairs, the walk that is part of each trip's stress.

use crate::box_ext::{amenities, box_rules, FREE_WALK_CELLS};
use crate::carriers::slot::floor_to_slot;
use crate::js;
use crate::resources::{FAMILY_CONDO, FAMILY_OFFICE, OP_SCORE_THRESHOLDS};
use crate::sims::scoring::has_nearby_noise;
use crate::sims::states::EVALUATABLE_FAMILIES;
use crate::world::{y_to_floor, WorldState};

/// One room's rating, step by step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakdown {
    /// the people's average trip stress, in ticks
    pub trip_stress: i64,
    /// cells to the nearest lift or stairs serving the floor (None: none
    /// does), and what that walk adds to each trip
    pub walk_cells: Option<i64>,
    pub walk_cost: i64,
    /// what the rent did: +30 for a low rent, -30 for a high one (as far as
    /// zero), and a very high rent sets the stress aside (the room is rated
    /// on noise alone)
    pub rent: i64,
    /// +60 for a noisy neighbour
    pub noise: i64,
    /// taken off by the tower's amenities
    pub relief: i64,
    /// the score rated, and the star rating's thresholds: under `fair` is
    /// good, under `bad` fair, otherwise bad
    pub score: i64,
    pub fair: i64,
    pub bad: i64,
}

fn key_xy(key: &str) -> Option<(i64, i64)> {
    let (x, y) = key.split_once(',')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// Cells from `x` to the nearest lift or stairs on `floor` (an express lift
/// counts as no walk: upstream never charges for one).
fn nearest_way(world: &WorldState, x: i64, floor: i64) -> Option<i64> {
    let lifts = world.carriers.iter().filter(|c| floor_to_slot(c, floor) >= 0 && c.cars.iter().any(|car| car.active)).map(|c| {
        if c.carrier_mode == 0 {
            0
        } else {
            (c.column - x).abs()
        }
    });
    let links = world
        .special_links
        .iter()
        .filter(|l| l.active && (l.entry_floor..=l.entry_floor + (l.flags >> 1) + 1).contains(&floor))
        .map(|l| (l.height_metric - x).abs());
    lifts.chain(links).min()
}

/// The breakdown of the room at `key`, if it is one that is rated, has been
/// rated, and BOX RULES are on.
pub fn of(world: &WorldState, key: &str) -> Option<Breakdown> {
    if !box_rules(world) {
        return None;
    }
    let o = world.placed_objects.get(key)?;
    if !EVALUATABLE_FAMILIES.contains(&o.object_type_code) || o.eval_score < 0 {
        return None;
    }
    let (x, y) = key_xy(key)?;
    let floor = y_to_floor(y);
    let mut b = Breakdown { trip_stress: o.eval_score, ..Default::default() };
    b.walk_cells = nearest_way(world, x, floor);
    b.walk_cost = b.walk_cells.map(|c| (c - FREE_WALK_CELLS).max(0)).unwrap_or(0);

    let mut score = o.eval_score;
    match o.rent_level {
        0 => score += 30,
        2 => score = 0.max(score - 30),
        3 => score = 0,
        _ => {}
    }
    b.rent = score - o.eval_score;
    let radius = match o.object_type_code {
        FAMILY_OFFICE => 10,
        FAMILY_CONDO => 30,
        _ => 20,
    };
    if has_nearby_noise(world, key, floor, radius) {
        b.noise = 60;
        score += 60;
    }
    let after = 0.max(score - amenities::relief_seen(world, key));
    b.relief = score - after;
    b.score = after;
    let [fair, bad] = js::record_get(OP_SCORE_THRESHOLDS, world.star_count.min(5)).copied().unwrap_or([80, 200]);
    b.fair = fair;
    b.bad = bad;
    Some(b)
}

//! BOX addition (BOX RULES only): office workers who come by train
//! (docs/metro.md).
//!
//! Upstream's metro station is a picture and a condition for the fifth
//! star: nobody uses it.  Under BOX RULES a third of the office workers who
//! do not drive come by metro, when the tower has a station and a lift,
//! stairs or escalator from its floor to their office: they set off in the
//! morning from the station's floor instead of the lobby, and go home to it
//! in the evening.  So a station takes some of the rush off the lobby and
//! its lifts, and puts it on the lifts that go down to the station.
//!
//! Hooks in sims/office.rs (each `// BOX: metro`): the morning gate's
//! `arrival_floor`; the evening departure's `exit_floor`, and its arrival
//! test `is_exit`.  Each is None or false with BOX RULES off.

use crate::box_ext::{box_rules, fnv};
use crate::resources::FAMILY_OFFICE;
use crate::route_scoring::select_candidate::select_best_route_candidate;
use crate::world::{SimRecord, WorldState};

/// The station's floor (its top, the concourse), if there is one.
pub fn station_floor(world: &WorldState) -> Option<i64> {
    (world.gate_flags.metro_station_floor_index >= 0).then_some(world.gate_flags.metro_station_floor_index)
}

/// This office worker would come by metro: one in three of those who do not
/// drive (sims/office.rs's choice of drivers), by their place.
pub fn rides(world: &WorldState, sim: &SimRecord) -> bool {
    box_rules(world)
        && sim.family_code == FAMILY_OFFICE
        && station_floor(world).is_some()
        && (sim.floor_anchor + sim.home_column) % 4 != 1
        && fnv(&format!("metro/{}/{}/{}", sim.floor_anchor, sim.home_column, sim.base_offset)) % 3 == 0
}

fn way(world: &WorldState, from: i64, to: i64) -> bool {
    from == to || select_best_route_candidate(world, from, to, Some(true), None).is_some()
}

/// Hook in the office morning gate: the floor a metro rider sets off from,
/// if they ride and can get from the station to the office.
pub fn arrival_floor(world: &WorldState, sim_index: i64) -> Option<i64> {
    let sim = world.sims.get(sim_index as usize)?;
    let station = station_floor(world)?;
    (rides(world, sim) && way(world, station, sim.floor_anchor)).then_some(station)
}

/// Hook in the office evening departure: the floor a metro rider goes home
/// by, if they ride and can get to the station.
pub fn exit_floor(world: &WorldState, sim_index: i64) -> Option<i64> {
    let sim = world.sims.get(sim_index as usize)?;
    let station = station_floor(world)?;
    (rides(world, sim) && way(world, sim.floor_anchor, station)).then_some(station)
}

/// Hook in the office arrival: a rider going home has arrived at the
/// station.
pub fn is_exit(world: &WorldState, sim_index: i64, floor: i64) -> bool {
    let Some(sim) = world.sims.get(sim_index as usize) else { return false };
    station_floor(world) == Some(floor) && rides(world, sim)
}

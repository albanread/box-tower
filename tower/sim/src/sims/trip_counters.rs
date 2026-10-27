// ts: sims/trip-counters.ts

// Trip-counter facade. The binary's stress accessors (11e0:*) moved to
// `sim/stress/*.ts` in Phase 8; this module keeps the family-scoped reset
// helpers that have no direct 11e0 counterpart (they live outside the
// stress subsystem) and re-exports the moved accessors so legacy call
// sites compile unchanged.

use crate::resources::{FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN};
use crate::world::{SimRecord, WorldState};

use super::population::find_sibling_sims;

pub use crate::stress::add_delay::add_delay_to_current_sim;
pub use crate::stress::rebase_elapsed::rebase_sim_elapsed_from_clock;
pub use crate::stress::trip_counters::advance_sim_trip_counters;

/// Clear trip counters. Spec: reset_sim_trip_counters.
pub fn reset_sim_trip_counters(sim: &mut SimRecord) {
    sim.trip_count = 0;
    sim.accumulated_ticks = 0;
}

// Binary reset_facility_sim_trip_counters @ 1138:0d07: iterates occupants of a
// facility via compute_object_occupant_runtime_index(floor, slot, startOccupant).
// startOccupant=1 for hotel families (3/4/5) — i.e. the baseOffset=0 occupant
// is intentionally skipped. For all other families startOccupant=0 (full sweep).
const HOTEL_FAMILY_CODES: &[i64] = &[FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_HOTEL_SUITE];

pub fn reset_facility_sim_trip_counters(world: &mut WorldState, sim_index: i64) {
    let skip_primary = HOTEL_FAMILY_CODES.contains(&world.sims[sim_index as usize].family_code);
    for sibling in find_sibling_sims(world, sim_index) {
        if skip_primary && world.sims[sibling as usize].base_offset == 0 {
            continue;
        }
        reset_sim_trip_counters(&mut world.sims[sibling as usize]);
    }
}

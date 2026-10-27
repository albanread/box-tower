// ts: families/restaurant.ts

// 1228:466d gate_object_family_restaurant_fast_food_state_handler (family 6/12)
// 1228:4851 dispatch_object_family_restaurant_fast_food_state_handler (family 6/12)
//
// Restaurant / fast-food state machine (family 6 restaurant, 12 fast food).
// Phase 5b: the gate splits restaurant / fast-food off from the shared
// commercial processor in `sims/commercial.ts`. The dispatch body remains
// in processCommercialSim which internally gates on familyCode to apply
// restaurant/fast-food-specific quirks (silent-park on VENUE_CLOSED,
// different RNG gates per daypart, etc.).

use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::ledger::LedgerState;
use crate::resources::{FAMILY_FAST_FOOD, FAMILY_RESTAURANT};
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::commercial::{
    handle_commercial_sim_arrival as _handle_commercial_sim_arrival,
    process_commercial_sim as _process_commercial_sim,
};
use crate::time::TimeState;
use crate::world::WorldState;

/// 1228:466d gate_object_family_restaurant_fast_food_state_handler. Gates on
/// familyCode ∈ {6, 12} and delegates to the shared commercial processor
/// (which holds the restaurant-specific RNG gates: daypart-4 1/12, daypart-5
/// before dayTick 2199, etc.).
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn gate_object_family_restaurant_fast_food_state_handler(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
) {
    let sim = &world.sims[sim_index as usize];
    if sim.family_code != FAMILY_RESTAURANT && sim.family_code != FAMILY_FAST_FOOD {
        return;
    }
    if is_sim_in_transit(sim.state_code) {
        maybe_dispatch_queued_route_after_wait(world, ledger, time, sim_index);
        return;
    }
    _process_commercial_sim(world, ledger, time, sim_index);
}

/// 1228:4851 dispatch_object_family_restaurant_fast_food_state_handler.
pub fn dispatch_object_family_restaurant_fast_food_state_handler(
    world: &mut WorldState,
    sim_index: i64,
    arrival_floor: i64,
    time: &TimeState,
) {
    _handle_commercial_sim_arrival(world, sim_index, arrival_floor, time);
}

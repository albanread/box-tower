// ts: families/retail.ts

// 1228:3ed9 gate_object_family_retail_state_handler (family 10)
// 1228:40c0 dispatch_object_family_retail_state_handler (family 10)
//
// Retail state machine (family 10). Phase 5b: the gate splits retail off
// from the shared commercial processor in `sims/commercial.ts`. The
// dispatch body (state-0x20 route to home, state-0x05 depart, etc.)
// currently remains in processCommercialSim, which internally gates on
// `sim.familyCode === FAMILY_RETAIL` to apply retail-specific quirks
// (occupiable-flag early exit, no silent-park on VENUE_CLOSED, etc.).
//
// TODO: binary 1228:3ed9 — the full retail gate/dispatch bodies remain
// intertwined with restaurant/fast-food inside processCommercialSim. A
// later phase can fully split them; for Phase 5b we preserve the
// binary-aligned entry-point shape here.

use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::ledger::LedgerState;
use crate::resources::FAMILY_RETAIL;
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::commercial::{
    handle_commercial_sim_arrival as _handle_commercial_sim_arrival,
    process_commercial_sim as _process_commercial_sim,
};
use crate::time::TimeState;
use crate::world::WorldState;

pub use crate::sims::commercial::{handle_commercial_sim_arrival, process_commercial_sim};

/// 1228:3ed9 gate_object_family_retail_state_handler. Gates on
/// familyCode === FAMILY_RETAIL and delegates to the shared commercial
/// processor; retail-specific early exit (DORMANT+occupiedFlag, binary +0x14)
/// is already encoded inside processCommercialSim.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn gate_object_family_retail_state_handler(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
) {
    let sim = &world.sims[sim_index as usize];
    if sim.family_code != FAMILY_RETAIL {
        return;
    }
    if is_sim_in_transit(sim.state_code) {
        // Commercial families (6/10/0xc) do NOT dispatch through
        // dispatch_sim_behavior in the binary; the queued-wait timeout is
        // not wired for these families. Callers that reach here for a
        // transit sim are expected to be the arrival path.
        maybe_dispatch_queued_route_after_wait(world, ledger, time, sim_index);
        return;
    }
    _process_commercial_sim(world, ledger, time, sim_index);
}

/// 1228:40c0 dispatch_object_family_retail_state_handler. Arrival entry
/// point; delegates to handleCommercialSimArrival.
pub fn dispatch_object_family_retail_state_handler(
    world: &mut WorldState,
    sim_index: i64,
    arrival_floor: i64,
    time: &TimeState,
) {
    _handle_commercial_sim_arrival(world, sim_index, arrival_floor, time);
}

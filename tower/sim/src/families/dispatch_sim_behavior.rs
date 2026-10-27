// ts: families/dispatch-sim-behavior.ts

// 1228:186c dispatch_sim_behavior
//
// Two-tier switch: first on sim.familyCode (via cs:1c71 FAMILY_PROLOGUE_TABLE),
// then on sim.stateCode. Runs the family-specific prologue and jumps into
// the per-family gate or dispatch handler.
//
// Phase 5b body: callable by the daily sweep and `forceDispatchSimStateByFamily`
// test paths. The stride refresh at `sim-refresh/refresh-stride.ts`
// (1228:0d64) still owns the top-level loop; dispatchSimBehavior routes a
// single sim through its family refresh handler, using the state_code
// bits (0x40 in-transit, 0x20 waiting) to decide the two-tier branch
// per ROUTING-BINARY-MAP.md §4.2.

use super::condo::refresh_object_family_condo_state_handler;
use super::hotel::refresh_object_family_hotel_state_handler;
use super::housekeeping::update_object_family_housekeeping_connection_state;
use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use super::office::refresh_object_family_office_state_handler;
use super::restaurant::gate_object_family_restaurant_fast_food_state_handler;
use super::retail::gate_object_family_retail_state_handler;
use super::state_tables::family_prologue::FAMILY_PROLOGUE_TABLE;
use crate::js;
use crate::ledger::LedgerState;
use crate::resources::{
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN, FAMILY_HOUSEKEEPING,
    FAMILY_OFFICE, FAMILY_RESTAURANT, FAMILY_RETAIL,
};
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::time::TimeState;
use crate::world::WorldState;

/// Binary 1228:186c. The stride refresh (1228:0d64) indexes
/// FAMILY_PROLOGUE_TABLE (cs:1c71) on `family_code - 3`; if an entry is
/// present, the handler is invoked. In-transit sims (state_code & 0x40)
/// hit a different sub-branch that calls maybe_dispatch_queued_route_after_wait
/// (1228:15a0). Waiting sims (0x20) without the 0x40 bit fall through to
/// the family's refresh handler which handles the re-dispatch.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn dispatch_sim_behavior(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    // Binary cs:1c71 gate: only families with an entry dispatch here.
    let family_index = world.sims[si].family_code - 3;
    if js::record_get(FAMILY_PROLOGUE_TABLE, family_index).is_none() {
        return;
    }

    // Binary: in-transit sims (0x40) route to the wait-timeout path. When
    // the timeout doesn't fire, the sim stays in transit and the family's
    // refresh handler is skipped (that branch runs elsewhere in the binary
    // — specifically the refresh handler's own state_code >= 0x40 arm,
    // which we already wire through the family `refresh*` functions).
    if is_sim_in_transit(world.sims[si].state_code) {
        maybe_dispatch_queued_route_after_wait(world, ledger, time, sim_index);
        return;
    }

    // Waiting / idle: dispatch into the family handler.
    match world.sims[si].family_code {
        FAMILY_HOTEL_SINGLE | FAMILY_HOTEL_TWIN | FAMILY_HOTEL_SUITE => {
            refresh_object_family_hotel_state_handler(world, ledger, time, sim_index);
        }
        FAMILY_OFFICE => {
            refresh_object_family_office_state_handler(world, ledger, time, sim_index);
        }
        FAMILY_CONDO => {
            refresh_object_family_condo_state_handler(world, ledger, time, sim_index);
        }
        FAMILY_RETAIL => {
            gate_object_family_retail_state_handler(world, ledger, time, sim_index);
        }
        FAMILY_RESTAURANT | FAMILY_FAST_FOOD => {
            gate_object_family_restaurant_fast_food_state_handler(world, ledger, time, sim_index);
        }
        FAMILY_HOUSEKEEPING => {
            update_object_family_housekeeping_connection_state(world, time, sim_index);
        }
        _ => {
            // Entertainment (18/29), recycling (33), parking (36) still use
            // legacy TS branches.
        }
    }
}

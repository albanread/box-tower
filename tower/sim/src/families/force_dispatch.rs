// ts: families/force-dispatch.ts

// 1228:1614 force_dispatch_sim_state_by_family
//
// Forces a dispatch into the family handler ignoring the 0x20/0x40 mode
// bits — used by external paths (daily sweep, transfer-floor failure
// re-dispatch) to re-drive state transitions. The binary strips the bits
// and indexes the per-family state table using the low 4 bits of
// state_code.
//
// Phase 5b note: the TS state constants (e.g. STATE_MORNING_GATE = 0x20,
// STATE_NIGHT_B = 0x26) ENCODE the waiting bit as part of the phase byte,
// unlike the binary where 0x20 is strictly the "waiting" mode bit. So
// this function does NOT strip 0x20 from the state byte before
// dispatching — doing so would convert MORNING_GATE into STATE_COMMUTE
// (0x00) and break the trace. Instead we strip only the 0x40 in-transit
// bit, which the TS constants use via `withTransitFlag` (e.g.
// STATE_COMMUTE_TRANSIT = 0x40, STATE_MORNING_TRANSIT = 0x60); stripping
// 0x40 from those yields the un-transit variant (COMMUTE / MORNING_GATE),
// which is the binary's intent.

use super::condo::refresh_object_family_condo_state_handler;
use super::hotel::refresh_object_family_hotel_state_handler;
use super::housekeeping::update_object_family_housekeeping_connection_state;
use super::office::refresh_object_family_office_state_handler;
use super::restaurant::gate_object_family_restaurant_fast_food_state_handler;
use super::retail::gate_object_family_retail_state_handler;
use crate::ledger::LedgerState;
use crate::resources::{
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN, FAMILY_HOUSEKEEPING,
    FAMILY_OFFICE, FAMILY_RESTAURANT, FAMILY_RETAIL,
};
use crate::sim_access::state_bits::{set_sim_in_transit, SIM_STATE_IN_TRANSIT_BIT};
use crate::time::TimeState;
use crate::world::WorldState;

/// 1228:1614 force_dispatch_sim_state_by_family. Strips the in-transit bit
/// (0x40) from state_code and re-enters the family refresh handler. Callers
/// include `assign_request_to_runtime_route` on transfer-floor failure
/// (1218:0d4e) and the daily drain sweep at checkpoint 0x9c4
/// (`dispatch_active_requests_by_family`).
///
/// See file-level comment for why we do NOT strip 0x20 (that bit is part
/// of the TS state byte for phases 0x20..0x27, not a separate mode flag).
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn force_dispatch_sim_state_by_family(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
) {
    let si = sim_index as usize;
    // Strip the 0x40 in-transit bit only. TS transit states use
    // withTransitFlag(base) = 0x40 | base; stripping 0x40 restores the base
    // phase which is what the binary's force-dispatch feeds into the
    // refresh handler.
    if world.sims[si].state_code & SIM_STATE_IN_TRANSIT_BIT != 0 {
        set_sim_in_transit(&mut world.sims[si], false);
    }
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
            // TODO: binary 1228:1614 — entertainment (18/29), recycling (33),
            // parking (36) refresh handlers are TODO stubs.
        }
    }
}

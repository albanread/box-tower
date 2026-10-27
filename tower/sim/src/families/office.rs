// ts: families/office.ts

// 1228:1cb5 refresh_object_family_office_state_handler (family 7)
// 1228:2031 dispatch_object_family_office_state_handler (family 7)
//
// Office-family state machine (family 7). The refresh handler is the binary's
// two-tier entry point (gate + dispatch) and drives the per-state handlers
// through a Map<state_code, HandlerFn> table (see families/state-tables/office.ts).
//
// For in-transit states (0x40+), the binary runs decrementRouteQueueDirectionLoad
// as a prologue then dispatches through the same handler as the base state.
// The 0x00↔0x40 and 0x20↔0x60 aliases in OFFICE_REFRESH_HANDLER_TABLE preserve
// this: both map to the same function reference.

use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::ledger::LedgerState;
use crate::queue::cancel::decrement_route_queue_direction_load;
use crate::sim_access::state_bits::{is_sim_in_transit, sim_base_state};
use crate::sims::office::{
    handle_office_sim_arrival as _handle_office_sim_arrival, process_office_sim as _process_office_sim,
};
use crate::time::TimeState;
use crate::world::{RouteState, WorldState};

pub use crate::sims::office::{
    advance_office_presence_counter, handle_office_sim_arrival, next_office_return_state, process_office_sim,
};

/// 1228:1cb5 refresh_object_family_office_state_handler. Two-tier entry:
///   - `state_code < 0x40`: run the gate + dispatch body (processOfficeSim,
///     which encapsulates the cs:2005 jump table).
///   - `state_code >= 0x40`: call decrementRouteQueueDirectionLoad prologue
///     (1218:0fc4) then dispatch through the same handler as the base state
///     (aliased 0x00↔0x40, 0x20↔0x60 in OFFICE_REFRESH_HANDLER_TABLE).
///     When the wait-timeout fires via maybe_dispatch_queued_route_after_wait
///     (1228:15a0), the sim is evicted and set to STATE_NIGHT_B.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn refresh_object_family_office_state_handler(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
) {
    let si = sim_index as usize;
    if is_sim_in_transit(world.sims[si].state_code) {
        // Binary prologue: decrement_route_queue_direction_load runs for every
        // in-transit state before the handler is dispatched (cs:1c51 family-7
        // prologue table). The carrier/direction args match the route the sim
        // is currently on; the stub is a no-op until the TS scorer reads load
        // counters (see queue/cancel.ts TODO).
        let target = match world.sims[si].route {
            RouteState::Carrier { carrier_id, .. } => Some(carrier_id),
            _ => None,
        };
        // (`c.carrierId === undefined` is never true: a sim not on a carrier
        // takes the first carrier, if there is one.)
        let carrier = world
            .carriers
            .iter()
            .find(|c| Some(c.carrier_id) == target)
            .or(world.carriers.first());
        if let Some(carrier) = carrier {
            decrement_route_queue_direction_load(carrier, world.sims[si].origin_floor, 0);
        }
        maybe_dispatch_queued_route_after_wait(world, ledger, time, sim_index);
        if !is_sim_in_transit(world.sims[si].state_code) {
            return;
        }
    }
    _process_office_sim(world, ledger, time, sim_index);
}

/// 1228:2031 dispatch_object_family_office_state_handler. Binary dispatches
/// through a 16-entry state table (cs:2aac). The TS body lives in
/// processOfficeSim; the aliasing `0x00↔0x40`, `0x20↔0x60` in the binary
/// table is preserved by reading just the base phase via `simBaseState`
/// when strictly needed.
///
/// This function is called by the arrival path — dispatch_destination_queue_entries
/// (1218:0883) — with the carrier's arrival floor. Delegates to the TS
/// handleOfficeSimArrival which covers the full arrival-phase table.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn dispatch_object_family_office_state_handler(
    world: &mut WorldState,
    time: &TimeState,
    sim_index: i64,
    arrival_floor: i64,
) {
    // Binary quirk: state-table aliases 0x00↔0x40 and 0x20↔0x60 route to the
    // same handler; the difference is whether decrement_route_queue_direction_load
    // ran as prologue. We don't split the cases here because the TS arrival
    // handler uses the transit-phase byte (e.g. STATE_MORNING_TRANSIT = 0x60)
    // directly to distinguish arrival types.
    let _ = sim_base_state(world.sims[sim_index as usize].state_code); // reserved for future table-driven dispatch
    _handle_office_sim_arrival(world, time, sim_index, arrival_floor, None);
}

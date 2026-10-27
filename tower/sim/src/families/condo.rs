// ts: families/condo.ts

// 1228:3548 refresh_object_family_condo_state_handler (family 9)
// 1228:3870 dispatch_object_family_condo_state_handler (family 9)
//
// Condo-family state machine (family 9). The refresh handler is the binary's
// two-tier entry point (gate + dispatch), driving the per-state handlers
// through the cs:1c2d family-9 dispatch table (see families/state-tables/condo.ts).
//
// For in-transit states (0x40+), the binary runs decrementRouteQueueDirectionLoad
// as a prologue (cs:1c2d maps {0x40, 0x41, 0x60, 0x61, 0x62} all to handler
// 1228:1aba which calls it), then dispatches through the same handler as the base state.

use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::ledger::LedgerState;
use crate::queue::cancel::decrement_route_queue_direction_load;
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::condo::{
    handle_condo_sim_arrival as _handle_condo_sim_arrival, process_condo_sim as _process_condo_sim,
};
use crate::time::TimeState;
use crate::world::{RouteState, SimRecord, WorldState};

pub use crate::sims::condo::{handle_condo_sim_arrival, process_condo_sim};

/// 1228:3548 refresh_object_family_condo_state_handler — two-tier entry:
///   - `state_code < 0x40`: run the gate + dispatch body (processCondoSim).
///   - `state_code >= 0x40`: call decrementRouteQueueDirectionLoad prologue
///     (1218:0fc4) then dispatch through the same handler as the base state.
///     Binary quirk: family-9 cs:1c2d table maps {0x40, 0x41, 0x60, 0x61, 0x62}
///     all to handler 1228:1aba — same aliasing mechanism as office/hotel.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn refresh_object_family_condo_state_handler(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
) {
    let si = sim_index as usize;
    if is_sim_in_transit(world.sims[si].state_code) {
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
    _process_condo_sim(world, ledger, time, sim_index);
}

/// 1228:3870 dispatch_object_family_condo_state_handler — arrival dispatch.
pub fn dispatch_object_family_condo_state_handler(
    sim: &mut SimRecord,
    arrival_floor: i64,
    time: &TimeState,
    arrived_via_carrier: bool,
) {
    _handle_condo_sim_arrival(sim, arrival_floor, time, arrived_via_carrier);
}

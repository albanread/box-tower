// ts: families/hotel.ts

// 1228:2aec refresh_object_family_hotel_state_handler (family 3/4/5)
// 1228:2dae dispatch_object_family_hotel_state_handler (family 3/4/5)
//
// Hotel-family state machine (family 3 single, 4 twin, 5 suite). The refresh
// handler is the binary's two-tier entry point (gate + dispatch), driving the
// per-state handlers through the cs:1c41 dispatch table (see
// families/state-tables/hotel.ts).
//
// For in-transit states (0x40+), the binary runs decrementRouteQueueDirectionLoad
// as a prologue (cs:1c41 table entries 0x41, 0x45, 0x60, 0x62 all point to
// handlers that call it), then dispatches through the same handler as the base state.

use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::ledger::LedgerState;
use crate::queue::cancel::decrement_route_queue_direction_load;
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::hotel::{
    handle_hotel_sim_arrival as _handle_hotel_sim_arrival, process_hotel_sim as _process_hotel_sim,
};
use crate::time::TimeState;
use crate::world::{RouteState, WorldState};

pub use crate::sims::hotel::{checkout_hotel_stay, handle_hotel_sim_arrival, process_hotel_sim};

/// 1228:2aec refresh_object_family_hotel_state_handler — two-tier entry:
///   - `state_code < 0x40`: run the gate + dispatch body (processHotelSim).
///   - `state_code >= 0x40`: call decrementRouteQueueDirectionLoad prologue
///     (1218:0fc4) then dispatch through the same handler as the base state.
///     The binary's family-3/4/5 dispatch table (cs:1c41) maps waiting/in-transit
///     states {0x41, 0x45, 0x60, 0x62} to in-place handlers; the TS body
///     already distinguishes these via the TRANSIT-suffixed state constants.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn refresh_object_family_hotel_state_handler(
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
    _process_hotel_sim(world, ledger, time, sim_index);
}

/// 1228:2dae dispatch_object_family_hotel_state_handler — arrival dispatch.
/// Called by dispatch_destination_queue_entries (1218:0883). TS body lives
/// in handleHotelSimArrival.
pub fn dispatch_object_family_hotel_state_handler(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
    arrival_floor: i64,
) {
    _handle_hotel_sim_arrival(world, ledger, time, sim_index, arrival_floor);
}

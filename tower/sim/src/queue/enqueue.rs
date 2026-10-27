// ts: queue/enqueue.ts

// 1218:1002 enqueue_request_into_route_queue
//
// Append a route request to the (carrier, floor, direction) ring. The first
// enqueue onto a previously-empty direction queue triggers
// `assign_car_to_floor_request` to pick a serving car; subsequent enqueues
// onto the same ring ride along that assignment until the car services it
// and `clear_floor_requests_on_arrival` zeroes the route-status byte.

use super::route_record::RouteRequestRing;
use crate::carriers::assign::assign_car_to_floor_request;
use crate::carriers::slot::{floor_to_slot, is_express_stop_floor};
use crate::carriers::sync::sync_assignment_status;
use crate::world::{CarrierFloorQueue, CarrierPendingRoute, CarrierRecord, LobbyMode};

/// The index in `carrier.floor_queues` of the floor's queue (upstream
/// returns the queue itself, or null).
fn get_queue_state(carrier: &CarrierRecord, floor: i64) -> Option<usize> {
    let slot = floor_to_slot(carrier, floor);
    if slot < 0 || slot >= carrier.floor_queues.len() as i64 {
        return None;
    }
    Some(slot as usize)
}

fn get_direction_queue(queue: &mut CarrierFloorQueue, direction_flag: i64) -> &mut RouteRequestRing {
    if direction_flag == 1 {
        &mut queue.up
    } else {
        &mut queue.down
    }
}

/// Binary `enqueue_request_into_route_queue` (1218:1002).
///
/// Pushes `simId` onto the direction ring for `(carrier, sourceFloor,
/// directionFlag)`. If the ring was previously empty, triggers
/// `assign_car_to_floor_request` so a car is dispatched to service the new
/// demand. Also adds a matching `CarrierPendingRoute` entry (TS-native
/// bookkeeping; the binary stores this inside the carrier's
/// `active_request_refs` table lazily via `assign_request_to_runtime_route`).
///
/// Deduplicates on `simId` so a sim that calls resolve twice does not
/// double-enqueue.
///
/// Binary quirk: size-40 ring silently overwrites head on 41st enqueue —
/// see `RouteRequestRing.push`. This function always returns true (the
/// binary has no full flag).
pub fn enqueue_request_into_route_queue(
    carrier: &mut CarrierRecord,
    sim_id: &str,
    source_floor: i64,
    destination_floor: i64,
    direction_flag: i64,
    lobby_mode: LobbyMode,
) -> bool {
    // (Upstream's __DRAIN_TRACE__ logging is left out: TRANSLATION.md.)
    let dup = carrier.pending_routes.iter().any(|route| route.sim_id == sim_id);
    if dup {
        return true;
    }
    // Binary 1218:1002: enqueue's body is gated on floor_to_carrier_slot_index
    // returning a valid slot — express (mode=0) at intermediate (non-lobby)
    // floors silently no-ops while `resolve_sim_route_between_floors` still
    // returns 2 and writes sim+8. Mirror that here so the sim lands in a
    // logical-only "queued on express" state without actually occupying a
    // ring or triggering assign_car_to_floor_request.
    if carrier.carrier_mode == 0 && !is_express_stop_floor(source_floor, lobby_mode) {
        return true;
    }
    let Some(floor_queue) = get_queue_state(carrier, source_floor) else {
        return false;
    };
    let direction_queue = get_direction_queue(&mut carrier.floor_queues[floor_queue], direction_flag);
    // Binary: resolve_sim_route_between_floors checks count==40 upstream and
    // returns 0 (queue-full) without invoking enqueue. Mirror that by failing
    // the push here so the caller takes the queue-full path.
    if direction_queue.is_full() {
        return false;
    }
    let was_direction_queue_empty = direction_queue.is_empty();
    direction_queue.push(sim_id);
    carrier.pending_routes.push(CarrierPendingRoute {
        sim_id: sim_id.to_string(),
        source_floor,
        destination_floor,
        boarded: false,
        direction_flag,
        assigned_car_index: -1,
    });
    if was_direction_queue_empty {
        assign_car_to_floor_request(carrier, source_floor, direction_flag);
    }
    sync_assignment_status(carrier);
    true
}

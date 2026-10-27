// ts: queue/scan.ts

// 1218:142a remove_request_from_unit_queue
// 1218:173a remove_request_from_active_route_slots
// 1218:187b store_request_in_active_route_slot
// 1218:1905 pop_active_route_slot_request
//
// Per-car active-slot ring ops + ring-scan removal. The active-slot ring
// is the 42-entry `active_request_refs` table on each TowerUnitRouteRecord;
// it holds currently-assigned passengers that the car is either en route
// to pick up or carrying toward a destination.

use super::route_record::RouteRequestRing;
use crate::carriers::slot::floor_to_slot;
use crate::carriers::sync::{active_slot_limit_for, has_active_slot, normalize_inactive_slots, sync_pending_route_ids};
use crate::carriers::target::recompute_car_target_and_direction;
use crate::world::{CarrierCar, CarrierRecord};

fn get_direction_ring<'a>(
    carrier: &'a mut CarrierRecord,
    source_floor: i64,
    direction_flag: i64,
) -> Option<&'a mut RouteRequestRing> {
    let slot = floor_to_slot(carrier, source_floor);
    if slot < 0 || slot >= carrier.floor_queues.len() as i64 {
        return None;
    }
    let queue = &mut carrier.floor_queues[slot as usize];
    Some(if direction_flag == 1 { &mut queue.up } else { &mut queue.down })
}

/// Binary `remove_request_from_unit_queue` (1218:142a). Scans a floor's
/// direction ring and compacts out the entry whose request id matches
/// `simId`, preserving the relative order of the remaining entries. Returns
/// true on hit, false otherwise.
pub fn remove_request_from_unit_queue(
    carrier: &mut CarrierRecord,
    sim_id: &str,
    source_floor: i64,
    direction_flag: i64,
) -> bool {
    let Some(ring) = get_direction_ring(carrier, source_floor, direction_flag) else {
        return false;
    };
    ring.remove_first(sim_id)
}

/// Binary `store_request_in_active_route_slot` (1218:187b). Writes the
/// request id + source/destination floors into the first free slot of the
/// car's `active_request_refs` ring. Returns true when stored, false when
/// every slot up to `assignment_capacity` is occupied.
///
/// Re-uses a slot the caller already owns (via `hasActiveSlot`) to avoid
/// duplicate slot inserts.
///
/// (`car` is `carrier.cars[car_index]`, `route` is
/// `carrier.pending_routes[route_index]`.)
pub fn store_request_in_active_route_slot(carrier: &mut CarrierRecord, car_index: i64, route_index: i64) -> bool {
    let route = carrier.pending_routes[route_index as usize].clone();
    if has_active_slot(&carrier.cars[car_index as usize], &route.sim_id) {
        return true;
    }
    let limit = active_slot_limit_for(carrier);
    let car = &mut carrier.cars[car_index as usize];
    for index in 0..limit.max(0) as usize {
        let Some(slot) = car.active_route_slots.get_mut(index) else {
            continue;
        };
        if slot.active {
            continue;
        }
        slot.route_id = route.sim_id.clone();
        slot.source_floor = route.source_floor;
        slot.destination_floor = route.destination_floor;
        slot.boarded = route.boarded;
        slot.active = true;
        sync_pending_route_ids(car);
        return true;
    }
    false
}

/// Binary `pop_active_route_slot_request` (1218:1905). Clears the active
/// slot identified by its slot index and returns the request id that was
/// there. This is the consumer the car's arrival path uses to mark a
/// rider as disembarked before clearing `assigned_count`.
pub fn pop_active_route_slot_request(car: &mut CarrierCar, slot_index: i64) -> Option<String> {
    if slot_index < 0 {
        return None;
    }
    let slot = car.active_route_slots.get_mut(slot_index as usize)?;
    if !slot.active {
        return None;
    }
    let id = std::mem::take(&mut slot.route_id);
    slot.active = false;
    slot.route_id = String::new();
    slot.source_floor = 0xff;
    slot.destination_floor = 0xff;
    slot.boarded = false;
    sync_pending_route_ids(car);
    Some(id)
}

/// Binary `remove_request_from_active_route_slots` (1218:173a). Scans cars
/// in index order; on the FIRST car/slot whose routeId matches, clears the
/// slot, then invokes `recompute_car_target_and_direction` for THAT car and
/// returns 1. Returns 0 if no slot matched.
///
/// The recompute call is how the binary re-evaluates a car's sweep direction
/// the moment a rider/route is pulled out of its active set — most notably,
/// it lets `update_car_direction_flag` (1098:1d2f) flip an express car at
/// the top served floor (arrivalSeen==1, dir==1) to dir==0 when a cancel
/// runs during the dwell-5 arrival path.
///
/// Note on counter decrements: the binary also decrements `assignedCount`
/// (-0x5b) and `destinationCountByFloor[+0xc]` for the slot's destination
/// floor inline here. The TS counter model treats `assignedCount` as
/// boarded-riders-only (incremented in `boardWaitingRoutes`), so the cancel
/// path leaves `assignedCount` alone — counter resync runs through
/// `syncAssignmentStatus` after the cancel completes. Mirroring the binary's
/// unconditional decrement here would mis-balance non-boarded slot removals,
/// so we keep that out of this routine.
pub fn remove_request_from_active_route_slots(carrier: &mut CarrierRecord, sim_id: &str) -> i64 {
    for car_index in 0..carrier.cars.len() {
        let limit = active_slot_limit_for(carrier);
        for slot_index in 0..limit.max(0) as usize {
            let car = &mut carrier.cars[car_index];
            let Some(slot) = car.active_route_slots.get_mut(slot_index) else {
                continue;
            };
            if !slot.active || slot.route_id != sim_id {
                continue;
            }
            slot.active = false;
            normalize_inactive_slots(car);
            sync_pending_route_ids(car);
            recompute_car_target_and_direction(carrier, car_index as i64);
            return 1;
        }
    }
    0
}

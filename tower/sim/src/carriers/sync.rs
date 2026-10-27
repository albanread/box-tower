// ts: carriers/sync.ts

// Carrier-side derived-state sync helpers.
//
// The binary maintains per-car counters (waiting_count[], destination_count[],
// pending_route_ids, slot_destination_floors) incrementally as requests move
// through the queue. The current TS model keeps `pendingRoutes` as the source
// of truth and resyncs the derived counters after each mutation through
// `syncAssignmentStatus`. These helpers live here (not in carriers.ts) so the
// queue/ modules can call them without an import cycle.

use crate::world::{CarrierCar, CarrierPendingRoute, CarrierRecord, CarrierRouteSlot};

use super::slot::floor_to_slot;

const ACTIVE_SLOT_CAPACITY: i64 = 42;

fn active_slot_limit(carrier: &CarrierRecord) -> i64 {
    ACTIVE_SLOT_CAPACITY.min(carrier.assignment_capacity)
}

fn find_route<'a>(carrier: &'a CarrierRecord, route_id: &str) -> Option<&'a CarrierPendingRoute> {
    carrier.pending_routes.iter().find(|route| route.sim_id == route_id)
}

pub fn sync_pending_route_ids(car: &mut CarrierCar) {
    car.pending_route_ids = car
        .active_route_slots
        .iter()
        .filter(|slot| slot.active)
        .map(|slot| slot.route_id.clone())
        .collect();
}

fn empty_slot() -> CarrierRouteSlot {
    CarrierRouteSlot {
        route_id: String::new(),
        source_floor: 0xff,
        destination_floor: 0xff,
        boarded: false,
        active: false,
    }
}

// Binary preserves slot indices across syncs: `pop_active_route_slot_request`
// (1218:1905) clears in place, and `store_request_in_active_route_slot`
// (1218:187b) scans from index 0 and reuses the first freed slot. Compacting
// here would shift later slots forward and reorder boarding in
// `dispatchDestinationQueueEntries`, so we walk in place instead — clearing
// inactive/orphaned slots to the sentinel, refreshing live slot fields from
// the route table, and only padding when the array is shorter than capacity
// (snapshot hydration), never by appending after a compaction.
pub fn sync_route_slots(carrier: &mut CarrierRecord, car_index: i64) {
    let ci = car_index as usize;
    for i in 0..carrier.cars[ci].active_route_slots.len() {
        let slot = &carrier.cars[ci].active_route_slots[i];
        if !slot.active {
            let slot = &mut carrier.cars[ci].active_route_slots[i];
            slot.route_id = String::new();
            slot.source_floor = 0xff;
            slot.destination_floor = 0xff;
            slot.boarded = false;
            continue;
        }
        let found = find_route(carrier, &slot.route_id)
            .map(|route| (route.source_floor, route.destination_floor, route.boarded));
        let slot = &mut carrier.cars[ci].active_route_slots[i];
        let Some((source_floor, destination_floor, boarded)) = found else {
            slot.active = false;
            slot.route_id = String::new();
            slot.source_floor = 0xff;
            slot.destination_floor = 0xff;
            slot.boarded = false;
            continue;
        };
        slot.source_floor = source_floor;
        slot.destination_floor = destination_floor;
        slot.boarded = boarded;
    }
    let car = &mut carrier.cars[ci];
    while (car.active_route_slots.len() as i64) < ACTIVE_SLOT_CAPACITY {
        car.active_route_slots.push(empty_slot());
    }
    sync_pending_route_ids(car);
}

pub fn has_active_slot(car: &CarrierCar, route_id: &str) -> bool {
    car.active_route_slots
        .iter()
        .any(|slot| slot.active && slot.route_id == route_id)
}

/// Upstream passes the route object (`carrier.pendingRoutes[...]`, always one
/// of the carrier's own routes); here its index in `carrier.pending_routes`
/// (TRANSLATION.md: container + index).
pub fn add_route_slot(carrier: &mut CarrierRecord, car_index: i64, route_index: i64) -> bool {
    let ci = car_index as usize;
    let route = carrier.pending_routes[route_index as usize].clone();
    if has_active_slot(&carrier.cars[ci], &route.sim_id) {
        return true;
    }
    let limit = active_slot_limit(carrier);
    let car = &mut carrier.cars[ci];
    let mut index = 0;
    while index < limit {
        let Some(slot) = car.active_route_slots.get_mut(index as usize) else {
            index += 1;
            continue;
        };
        if slot.active {
            index += 1;
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

fn sync_waiting_count(carrier: &mut CarrierRecord, car_index: i64) {
    let ci = car_index as usize;
    {
        let car = &mut carrier.cars[ci];
        car.waiting_count.fill(0);
        car.destination_count_by_floor.fill(0);
        car.nonempty_destination_count = 0;
    }

    for slot in 0..carrier.floor_queues.len() {
        let queue = &carrier.floor_queues[slot];
        let waiting = queue.up.size() + queue.down.size();
        // JavaScript grows the array on a write past its end; the writes go
        // in slot order, so a short array grows by one with no holes.
        let wc = &mut carrier.cars[ci].waiting_count;
        if slot < wc.len() {
            wc[slot] = waiting;
        } else {
            debug_assert_eq!(slot, wc.len());
            wc.push(waiting);
        }
    }

    let limit = active_slot_limit(carrier);
    for index in 0..limit.max(0) {
        let Some(slot_ref) = carrier.cars[ci].active_route_slots.get(index as usize) else {
            continue;
        };
        if !slot_ref.active || !slot_ref.boarded {
            continue;
        }
        let slot = floor_to_slot(carrier, slot_ref.destination_floor);
        let car = &mut carrier.cars[ci];
        if slot < 0 || slot >= car.destination_count_by_floor.len() as i64 {
            continue;
        }
        let prev = car.destination_count_by_floor[slot as usize];
        car.destination_count_by_floor[slot as usize] = prev + 1;
        if prev == 0 {
            car.nonempty_destination_count += 1;
        }
    }

    for r in 0..carrier.pending_routes.len() {
        let route = &carrier.pending_routes[r];
        if !route.boarded || route.assigned_car_index != car_index {
            continue;
        }
        let slot = floor_to_slot(carrier, route.destination_floor);
        let car = &mut carrier.cars[ci];
        if slot < 0 || slot >= car.destination_count_by_floor.len() as i64 {
            continue;
        }
        let prev = car.destination_count_by_floor[slot as usize];
        if prev == 0 {
            car.nonempty_destination_count += 1;
        }
        car.destination_count_by_floor[slot as usize] = prev + 1;
    }
}

// Rebuilds per-car derived state from the authoritative route/status tables.
// The binary mutates pendingAssignmentCount incrementally with the route-slot
// ownership tables, so if a TS path misses one of those inline decrements the
// count drifts. Recompute it here from primary/secondary ownership so target
// selection and dwell gates observe the same ownership state as the tables.
pub fn sync_assignment_status(carrier: &mut CarrierRecord) {
    for car in carrier.cars.iter_mut() {
        car.pending_assignment_count = 0;
    }
    for i in 0..carrier.primary_route_status_by_floor.len() {
        let owner = carrier.primary_route_status_by_floor[i];
        if owner > 0 {
            if let Some(car) = carrier.cars.get_mut((owner - 1) as usize) {
                car.pending_assignment_count += 1;
            }
        }
    }
    for i in 0..carrier.secondary_route_status_by_floor.len() {
        let owner = carrier.secondary_route_status_by_floor[i];
        if owner > 0 {
            if let Some(car) = carrier.cars.get_mut((owner - 1) as usize) {
                car.pending_assignment_count += 1;
            }
        }
    }
    for car_index in 0..carrier.cars.len() {
        sync_route_slots(carrier, car_index as i64);
        sync_waiting_count(carrier, car_index as i64);
    }
}

pub fn normalize_inactive_slots(car: &mut CarrierCar) {
    for slot in car.active_route_slots.iter_mut() {
        if !slot.active {
            slot.route_id = String::new();
            slot.source_floor = 0xff;
            slot.destination_floor = 0xff;
            slot.boarded = false;
        }
    }
}

pub const ACTIVE_SLOT_CAPACITY_CONST: i64 = ACTIVE_SLOT_CAPACITY;

pub fn active_slot_limit_for(carrier: &CarrierRecord) -> i64 {
    active_slot_limit(carrier)
}

/// `findRouteById`: the route itself, as `Array.prototype.find` gives it.
/// A caller that must write to it finds its index in `carrier.pending_routes`.
pub fn find_route_by_id<'a>(carrier: &'a CarrierRecord, route_id: &str) -> Option<&'a CarrierPendingRoute> {
    find_route(carrier, route_id)
}

/// find_route_by_id for a caller that goes on to write the route: its index in
/// `carrier.pending_routes` (the first with this id, as `find` gives).
pub fn find_route_index_by_id(carrier: &CarrierRecord, route_id: &str) -> Option<i64> {
    carrier.pending_routes.iter().position(|route| route.sim_id == route_id).map(|i| i as i64)
}

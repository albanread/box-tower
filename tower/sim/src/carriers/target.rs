// ts: carriers/target.ts

// 1098:0bcf recompute_car_target_and_direction
// 1098:1553 select_next_target_floor
// 1098:1d2f update_car_direction_flag
// 1098:1f4c find_nearest_work_floor
//
// Target/direction recomputation. `recomputeCarTargetAndDirection` calls
// `selectNextTargetFloor` for the next stop, then `updateCarDirectionFlag`
// to adjust the sweep direction at endpoints or on bidirectional flips.
// `findNearestWorkFloor` is the binary fallback when no pending assignments
// exist; current TS uses an inline homeFloor return in selectNextTargetFloor.

use crate::js;
use crate::world::CarrierRecord;

use super::arrival::{clear_floor_requests_on_arrival, reset_out_of_range_car};
use super::slot::floor_to_slot;

fn get_car_capacity(carrier: &CarrierRecord) -> i64 {
    carrier.assignment_capacity
}

pub fn recompute_car_target_and_direction(carrier: &mut CarrierRecord, car_index: i64) {
    let ci = car_index as usize;
    let schedule_flag = carrier.cars[ci].schedule_flag;
    let next = select_next_target_floor(carrier, car_index, schedule_flag);
    if next < carrier.bottom_served_floor || next > carrier.top_served_floor {
        reset_out_of_range_car(carrier, car_index);
        return;
    }
    carrier.cars[ci].target_floor = next;
    update_car_direction_flag(carrier, car_index);
    // Binary 1098:0bcf tail: refresh nearest_work_floor (car[-0x51]) for use
    // as the wrap turn-floor by find_best_available_car_for_floor and as the
    // idle-home test target (current == nearest_work_floor) in the same.
    carrier.cars[ci].nearest_work_floor = find_nearest_work_floor(carrier, car_index);
}

pub fn update_car_direction_flag(carrier: &mut CarrierRecord, car_index: i64) {
    let ci = car_index as usize;
    let floor = carrier.cars[ci].current_floor;
    let prev_dir = carrier.cars[ci].direction_flag;

    if floor != carrier.cars[ci].target_floor {
        // Binary 1098:1d2f: floor != target branch sets direction from relation
        // and goto-s past the clear-on-flip check, so no clear fires here.
        let car = &mut carrier.cars[ci];
        car.direction_flag = if floor < car.target_floor { 1 } else { 0 };
        return;
    }
    if carrier.cars[ci].arrival_seen == 0 {
        return;
    }

    if floor == carrier.top_served_floor && carrier.cars[ci].direction_flag == 1 {
        carrier.cars[ci].direction_flag = 0;
    } else if floor == carrier.bottom_served_floor && carrier.cars[ci].direction_flag == 0 {
        carrier.cars[ci].direction_flag = 1;
    } else if carrier.cars[ci].schedule_flag == 0 {
        // Bidirectional flip gated on schedF == 0. Express modes skip.
        let slot = floor_to_slot(carrier, floor);
        if slot >= 0 {
            let up_calls = js::at_or(&carrier.primary_route_status_by_floor, slot, 0) != 0;
            let down_calls = js::at_or(&carrier.secondary_route_status_by_floor, slot, 0) != 0;
            let car = &mut carrier.cars[ci];
            if car.direction_flag == 0 && !down_calls && up_calls {
                car.direction_flag = 1;
            } else if car.direction_flag == 1 && !up_calls && down_calls {
                car.direction_flag = 0;
            }
        }
    }

    // Binary 1098:1d2f tail: if direction changed during this call,
    // clear_floor_requests_on_arrival fires at the current floor with the
    // NEW direction. This clears the opposite-direction slot (since clause 2
    // of clear fires when dir==0 for example).
    if carrier.cars[ci].direction_flag != prev_dir {
        clear_floor_requests_on_arrival(carrier, car_index, floor);
    }
}

/// Binary `select_next_target_floor` @ 1098:1553. Multi-phase directional
/// scan that distinguishes queued riders (destination queue) from assignment
/// slots (up/down call pickups) and gates assignment targets on capacity.
///
/// Upstream takes `(car, carrier, carIndex, expressDirectionFlag)`; the car is
/// `carrier.cars[carIndex]`, so it is dropped (TRANSLATION.md).
pub fn select_next_target_floor(carrier: &CarrierRecord, car_index: i64, express_direction_flag: i64) -> i64 {
    let car = &carrier.cars[car_index as usize];
    // Binary idle check: arrivalTick == 0 && pendingDestCount == 0.
    // Also gate on pendingAssignmentCount because dayTick can be 0 at the
    // start of a day, making arrivalTick indistinguishable from "never set".
    if car.arrival_tick == 0 && car.nonempty_destination_count == 0 && car.pending_assignment_count == 0 {
        return car.home_floor;
    }

    // When a normal-mode car has finished all work, the binary keeps it parked
    // at home so the A1/B dwell cycle can continue without resetting the
    // arrival latch on every 5→0 transition.
    if express_direction_flag == 0 && car.nonempty_destination_count == 0 && car.pending_assignment_count == 0 {
        return car.home_floor;
    }

    let under_capacity = car.assigned_count != get_car_capacity(carrier);

    let has_queued_rider = |floor: i64| -> bool {
        let slot = floor_to_slot(carrier, floor);
        slot >= 0 && js::at_or(&car.destination_count_by_floor, slot, 0) > 0
    };
    // `table[slot] === carIndex + 1`: undefined (past the end) is never equal.
    let has_up_slot = |floor: i64| -> bool {
        let slot = floor_to_slot(carrier, floor);
        slot >= 0 && carrier.primary_route_status_by_floor.get(slot as usize) == Some(&(car_index + 1))
    };
    let has_down_slot = |floor: i64| -> bool {
        let slot = floor_to_slot(carrier, floor);
        slot >= 0 && carrier.secondary_route_status_by_floor.get(slot as usize) == Some(&(car_index + 1))
    };

    // schedMode 1: express-up. Scan downward for work, fallback = top.
    if express_direction_flag == 1 {
        let gate = (car.direction_flag != 0
            && (car.current_floor != carrier.top_served_floor || car.settle_counter != 0))
            || (car.direction_flag == 0
                && car.current_floor == carrier.bottom_served_floor
                && car.settle_counter == 0);
        if gate {
            let mut f = car.current_floor;
            while f >= carrier.bottom_served_floor {
                if has_queued_rider(f) {
                    return f;
                }
                if under_capacity && (has_down_slot(f) || has_up_slot(f)) {
                    return f;
                }
                f -= 1;
            }
        }
        return carrier.top_served_floor;
    }

    // schedMode 2: express-down. Scan upward for work, fallback = bottom.
    if express_direction_flag == 2 {
        let gate = (car.direction_flag == 0
            && (car.current_floor != carrier.bottom_served_floor || car.settle_counter != 0))
            || (car.direction_flag != 0
                && car.current_floor == carrier.top_served_floor
                && car.settle_counter == 0);
        if gate {
            let mut f = car.current_floor;
            while f <= carrier.top_served_floor {
                if has_queued_rider(f) {
                    return f;
                }
                if under_capacity && (has_up_slot(f) || has_down_slot(f)) {
                    return f;
                }
                f += 1;
            }
        }
        return carrier.bottom_served_floor;
    }

    // schedMode 0: bidirectional sweep
    if car.direction_flag == 0 {
        // Going down — phase 1: cur→bottom for queued OR downSlot
        let mut f = car.current_floor;
        while f >= carrier.bottom_served_floor {
            if has_queued_rider(f) {
                return f;
            }
            if under_capacity && has_down_slot(f) {
                return f;
            }
            f -= 1;
        }
        // Phase 2: bottom→cur for upSlot (capacity-gated)
        if under_capacity {
            let mut f = carrier.bottom_served_floor;
            while f <= car.current_floor {
                if has_up_slot(f) {
                    return f;
                }
                f += 1;
            }
        }
        // Phase 3: cur+1→top for upSlot (capacity-gated) OR queued
        let mut f = car.current_floor + 1;
        while f <= carrier.top_served_floor {
            if under_capacity && has_up_slot(f) {
                return f;
            }
            if has_queued_rider(f) {
                return f;
            }
            f += 1;
        }
        // Phase 4: top→cur+1 for downSlot (capacity-gated)
        if under_capacity {
            let mut f = carrier.top_served_floor;
            while f > car.current_floor {
                if has_down_slot(f) {
                    return f;
                }
                f -= 1;
            }
        }
    } else {
        // Going up — phase 1: cur→top for queued OR upSlot
        let mut f = car.current_floor;
        while f <= carrier.top_served_floor {
            if has_queued_rider(f) {
                return f;
            }
            if under_capacity && has_up_slot(f) {
                return f;
            }
            f += 1;
        }
        // Phase 2: top→cur for downSlot (capacity-gated)
        if under_capacity {
            let mut f = carrier.top_served_floor;
            while f >= car.current_floor {
                if has_down_slot(f) {
                    return f;
                }
                f -= 1;
            }
        }
        // Phase 3: cur-1→bottom for downSlot (capacity-gated) OR queued
        let mut f = car.current_floor - 1;
        while f >= carrier.bottom_served_floor {
            if under_capacity && has_down_slot(f) {
                return f;
            }
            if has_queued_rider(f) {
                return f;
            }
            f -= 1;
        }
        // Phase 4: bottom→cur-1 for upSlot (capacity-gated)
        if under_capacity {
            let mut f = carrier.bottom_served_floor;
            while f < car.current_floor {
                if has_up_slot(f) {
                    return f;
                }
                f += 1;
            }
        }
    }

    -1
}

/// Binary `find_nearest_work_floor` @ 1098:1f4c. Scans the current direction
/// for the nearest floor with pending work for THIS car: a queued rider
/// (destinationCountByFloor[floor] != 0) OR a primary/secondary route-status
/// slot tagged with this car (== carIndex+1). Scan range/direction follow
/// `directionFlag`: dir=0 (down) scans bottom..current; dir=1 (up) scans
/// top..current. If no work floor is found, falls back to the per-car home
/// floor stored in the carrier's reachability_masks_by_floor[carIndex-8] —
/// we keep that as `car.homeFloor`.
///
/// Result is cached as `car.nearestWorkFloor` and used by
/// `findBestAvailableCarForFloor` for both the wrap-cost "turn floor" and
/// the idle-home test (current == nearest_work).
pub fn find_nearest_work_floor(carrier: &CarrierRecord, car_index: i64) -> i64 {
    let car = &carrier.cars[car_index as usize];
    let dir = car.direction_flag;
    let cur = car.current_floor;
    let bottom = carrier.bottom_served_floor;
    let top = carrier.top_served_floor;

    let has_work = |floor: i64| -> bool {
        let slot = floor_to_slot(carrier, floor);
        if slot < 0 {
            return false;
        }
        if js::at_or(&car.destination_count_by_floor, slot, 0) != 0 {
            return true;
        }
        if carrier.primary_route_status_by_floor.get(slot as usize) == Some(&(car_index + 1)) {
            return true;
        }
        if carrier.secondary_route_status_by_floor.get(slot as usize) == Some(&(car_index + 1)) {
            return true;
        }
        false
    };

    if dir == 0 {
        let mut floor = bottom;
        while floor <= cur {
            if has_work(floor) {
                return floor;
            }
            floor += 1;
        }
    } else {
        let mut floor = top;
        while floor >= cur {
            if has_work(floor) {
                return floor;
            }
            floor -= 1;
        }
    }
    car.home_floor
}

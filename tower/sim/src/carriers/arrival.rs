// ts: carriers/arrival.ts

// 1098:13cc clear_floor_requests_on_arrival
// 1098:12c9 cancel_stale_floor_assignment
// 1098:0192 reset_out_of_range_car
//
// Per-floor arrival bookkeeping: clears primary/secondary route-status slots
// that match the arriving car's direction, decrements the owning car's
// pendingAssignmentCount inline, and handles out-of-range / stale-assignment
// housekeeping.

use crate::js;
use crate::world::CarrierRecord;

use super::slot::floor_to_slot;
use super::target::recompute_car_target_and_direction;

// Mirrors binary `decrement_car_pending_assignment_count` (1098:151c).
// Decrements the owner's pendingAssignmentCount and re-runs target/direction
// recompute so the owner re-evaluates its sweep — important because the
// recompute may flip the owner's direction at an endpoint, which then chains
// into another `clearFloorRequestsOnArrival` for the owner's current floor
// (binary chain observed at d3 t428 dense_office: car 7 arrives at floor 11,
// cross-decrements car 2's pac, car 2's recompute flips dir 0→1 at the bottom
// served floor, and that flip clears `primary[10]=2` for car 1).
fn decrement_owner_pac_and_recompute(carrier: &mut CarrierRecord, owner_index: i64) {
    let Some(owner) = (if owner_index >= 0 { carrier.cars.get_mut(owner_index as usize) } else { None }) else {
        return;
    };
    if owner.pending_assignment_count > 0 {
        owner.pending_assignment_count -= 1;
    }
    recompute_car_target_and_direction(carrier, owner_index);
}

// Mirrors binary clear_floor_requests_on_arrival (1098:13cc). Called when a
// car arrives at a floor. Each clause gates on (schedF != 0 || dir matches)
// && table[floor] != 0, then zeros the slot and decrements the owning car's
// pendingAssignmentCount. Same-car decrement is inline; cross-car goes
// through decrement_car_pending_assignment_count (1098:151c) which ALSO
// re-runs the owner's target/direction recompute (and may flip its direction
// at an endpoint, chaining into another clear at the owner's current floor).
//
// Upstream takes the car and finds its index with `carrier.cars.indexOf(car)`;
// here the index is the parameter.  The car's fields are read afresh for the
// second clause: the first clause's cross-car recompute can chain back into
// this car and change its direction, as it does in upstream.
pub fn clear_floor_requests_on_arrival(carrier: &mut CarrierRecord, car_index: i64, floor: i64) {
    let slot = floor_to_slot(carrier, floor);
    if slot < 0 {
        return;
    }
    let ci = car_index as usize;

    if carrier.cars[ci].schedule_flag != 0 || carrier.cars[ci].direction_flag != 0 {
        let pri = js::at_or(&carrier.primary_route_status_by_floor, slot, 0);
        if pri != 0 {
            carrier.primary_route_status_by_floor[slot as usize] = 0;
            let owner_index = pri - 1;
            if owner_index == car_index {
                let car = &mut carrier.cars[ci];
                if car.pending_assignment_count > 0 {
                    car.pending_assignment_count -= 1;
                }
            } else {
                decrement_owner_pac_and_recompute(carrier, owner_index);
            }
        }
    }

    if carrier.cars[ci].schedule_flag != 0 || carrier.cars[ci].direction_flag == 0 {
        let sec = js::at_or(&carrier.secondary_route_status_by_floor, slot, 0);
        if sec != 0 {
            carrier.secondary_route_status_by_floor[slot as usize] = 0;
            let owner_index = sec - 1;
            if owner_index == car_index {
                let car = &mut carrier.cars[ci];
                if car.pending_assignment_count > 0 {
                    car.pending_assignment_count -= 1;
                }
            } else {
                decrement_owner_pac_and_recompute(carrier, owner_index);
            }
        }
    }
}

// Mirrors binary reset_out_of_range_car (1098:0192). Forces a car home when
// its position or target falls outside the served-floor range. All counters
// and active route slots reset to idle.
pub fn reset_out_of_range_car(carrier: &mut CarrierRecord, car_index: i64) {
    let top = carrier.top_served_floor;
    let bottom = carrier.bottom_served_floor;
    let car = &mut carrier.cars[car_index as usize];
    let home_floor = top.min(bottom.max(car.home_floor));
    car.current_floor = home_floor;
    car.target_floor = home_floor;
    car.prev_floor = home_floor;
    car.settle_counter = 0;
    car.dwell_counter = 0;
    car.assigned_count = 0;
    car.pending_assignment_count = 0;
    car.dwell_start_pending_assignment_count = 0;
    car.direction_flag = 1;
    car.arrival_seen = 0;
    car.arrival_tick = 0;
    car.arrival_dispatch_this_tick = false;
    car.arrival_dispatch_starting_assigned_count = 0;
    car.suppress_dwell_opposite_direction_flip = false;
    // Binary 1098:0192 sets car[-0x51] (nearestWorkFloor) to homeFloor.
    car.nearest_work_floor = home_floor;
    car.destination_count_by_floor.fill(0);
    car.nonempty_destination_count = 0;
    for slot in car.active_route_slots.iter_mut() {
        slot.route_id = String::new();
        slot.source_floor = 0xff;
        slot.destination_floor = 0xff;
        slot.boarded = false;
        slot.active = false;
    }
    car.pending_route_ids = Vec::new();
}

// Mirrors binary cancel_stale_floor_assignment (1098:12c9). Invoked from
// advance_carrier_car_state's A2 (motion) branch when a car LEAVES a floor.
// Clears the per-direction route-status slot if it was assigned to THIS car
// (carIndex+1) and the gate (scheduleFlag != 0 || direction match) holds,
// then decrements this car's pendingAssignmentCount.
//
// (Note: an earlier same-name function at binary 1098:0d15 is a separate
// visual-grid housekeeping helper — that one does NOT touch route status.)
//
// Upstream takes `(carrier, car, floor, carIndex)`; the car is dropped.
pub fn cancel_stale_floor_assignment(carrier: &mut CarrierRecord, floor: i64, car_index: i64) {
    let slot = floor_to_slot(carrier, floor);
    if slot < 0 {
        return;
    }
    let car_tag = car_index + 1;
    let ci = car_index as usize;

    // Clause 1: clear primary[floor] if (sched != 0 || dir != 0) AND primary[floor] == carIndex+1
    if carrier.cars[ci].schedule_flag != 0 || carrier.cars[ci].direction_flag != 0 {
        if js::at_or(&carrier.primary_route_status_by_floor, slot, 0) == car_tag {
            carrier.primary_route_status_by_floor[slot as usize] = 0;
            let car = &mut carrier.cars[ci];
            if car.pending_assignment_count > 0 {
                car.pending_assignment_count -= 1;
            }
        }
    }

    // Clause 2: clear secondary[floor] if (sched != 0 || dir == 0) AND secondary[floor] == carIndex+1
    if carrier.cars[ci].schedule_flag != 0 || carrier.cars[ci].direction_flag == 0 {
        if js::at_or(&carrier.secondary_route_status_by_floor, slot, 0) == car_tag {
            carrier.secondary_route_status_by_floor[slot as usize] = 0;
            let car = &mut carrier.cars[ci];
            if car.pending_assignment_count > 0 {
                car.pending_assignment_count -= 1;
            }
        }
    }
}

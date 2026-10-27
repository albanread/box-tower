// ts: carriers/advance.ts

// 1098:06fb advance_carrier_car_state
//
// Pass 1 of the per-tick carrier update, invoked from carrier_tick
// (1098:03ab) before dispatch_carrier_car_arrivals and process_unit_travel_queue.
// Three-way branch on two countdowns (settle / dwell) driving the car state
// machine described in ROUTING-BINARY-MAP.md §4.5.
//
// State counters correspond to the binary's car struct fields:
//   settleCounter ↔ -0x5d (settle after a motion step)
//   dwellCounter  ↔ -0x5c (dwell/boarding at a stop; 5 = just arrived)

use crate::js;
use crate::time::TimeState;
use crate::world::{CarrierRecord, LobbyMode};

use super::arrival::{cancel_stale_floor_assignment, clear_floor_requests_on_arrival, reset_out_of_range_car};
use super::assign::assign_car_to_floor_request;
use super::depart::should_car_depart;
use super::motion::compute_car_motion_mode;
use super::position::advance_car_position_one_step;
use super::slot::floor_to_slot;
use super::target::recompute_car_target_and_direction;

const DEPARTURE_SEQUENCE_TICKS: i64 = 5;

fn get_car_capacity(carrier: &CarrierRecord) -> i64 {
    carrier.assignment_capacity
}

fn get_schedule_index(time: &TimeState) -> i64 {
    time.weekend_flag * 7 + time.daypart_index
}

/// Upstream takes `(car, carrier, carIndex, time, lobbyMode)`; the car is
/// `carrier.cars[carIndex]`, so it is dropped (TRANSLATION.md).
pub fn advance_carrier_car_state(carrier: &mut CarrierRecord, car_index: i64, time: &TimeState, lobby_mode: LobbyMode) {
    let ci = car_index as usize;
    if !carrier.cars[ci].active {
        return;
    }

    if carrier.cars[ci].current_floor < carrier.bottom_served_floor
        || carrier.cars[ci].current_floor > carrier.top_served_floor
    {
        reset_out_of_range_car(carrier, car_index);
        return;
    }

    // Branch C (1098:06fb): stabilize countdown. While nonzero, the car is
    // mid-motion-cycle. Decrement only if recomputed mode is still 0;
    // otherwise snap to 0 (fast-cancel).
    if carrier.cars[ci].settle_counter > 0 {
        if compute_car_motion_mode(carrier, car_index) == 0 {
            carrier.cars[ci].settle_counter -= 1;
        } else {
            carrier.cars[ci].settle_counter = 0;
        }
        return;
    }

    if carrier.cars[ci].dwell_counter == 0 {
        carrier.cars[ci].suppress_dwell_opposite_direction_flip = false;
        // Branch A. A1 (arrival / idle-at-target) fires when target == cur and
        // either the per-car destination queue has riders for this floor, or the
        // car is not full. A1 is level-triggered: as long as the gate holds the
        // next A1 fire (after B's 5-tick countdown) refreshes dwell back to 5.
        let current_slot = floor_to_slot(carrier, carrier.cars[ci].current_floor);
        let has_queued_rider =
            current_slot >= 0 && js::at_or(&carrier.cars[ci].destination_count_by_floor, current_slot, 0) > 0;
        let under_capacity = carrier.cars[ci].assigned_count != get_car_capacity(carrier);

        if carrier.cars[ci].target_floor == carrier.cars[ci].current_floor && (has_queued_rider || under_capacity) {
            // A1
            // Inlined from former loadScheduleFlag helper (binary reloads the
            // scheduling_flag inline within advance_carrier_car_state when a car
            // sits at a terminal floor).
            if carrier.cars[ci].current_floor == carrier.top_served_floor
                || carrier.cars[ci].current_floor == carrier.bottom_served_floor
            {
                carrier.cars[ci].schedule_flag =
                    js::at_or(&carrier.express_direction_flags, get_schedule_index(time), 0);
            }
            let floor = carrier.cars[ci].current_floor;
            clear_floor_requests_on_arrival(carrier, car_index, floor);
            let car = &mut carrier.cars[ci];
            car.dwell_start_pending_assignment_count = car.pending_assignment_count;
            car.dwell_counter = DEPARTURE_SEQUENCE_TICKS;
            if car.arrival_seen == 0 {
                car.arrival_tick = time.day_tick;
            }
            car.arrival_seen = 1;
            return;
        }

        // A2 — motion step. Binary 1098:06fb A2 calls cancel_stale_floor_assignment
        // (1098:12c9) FIRST (which may clear primary/secondary[floor] if assigned
        // to this car), THEN reads the queue + status to decide whether to re-assign.
        // Order matters: cancel runs before the up/down gate check.
        let depart_floor = carrier.cars[ci].current_floor;
        let depart_slot = floor_to_slot(carrier, depart_floor);
        cancel_stale_floor_assignment(carrier, depart_floor, car_index);
        let queue = if depart_slot >= 0 {
            carrier.floor_queues.get(depart_slot as usize)
        } else {
            None
        };
        let has_up_request = queue.is_some_and(|q| !q.up.is_empty())
            && js::at_or(&carrier.primary_route_status_by_floor, depart_slot, 0) == 0;
        let has_down_request = queue.is_some_and(|q| !q.down.is_empty())
            && js::at_or(&carrier.secondary_route_status_by_floor, depart_slot, 0) == 0;
        advance_car_position_one_step(carrier, car_index);
        if has_up_request {
            assign_car_to_floor_request(carrier, depart_floor, 1);
        }
        if has_down_request {
            assign_car_to_floor_request(carrier, depart_floor, 0);
        }
        return;
    }

    // Branch B — dwell countdown. Decrement unconditionally; on 0 transition,
    // latch prevFloor, recompute target+direction, then check departure gate.
    // If the gate says "wait", pin dwell=1 so this path runs again next tick.
    // Binary 1098:06fb: shouldCarDepart runs AFTER recomputeCarTargetAndDirection
    // (see ROUTING-BINARY-MAP.md §4.5).
    carrier.cars[ci].dwell_counter -= 1;
    if carrier.cars[ci].dwell_counter == 0 {
        carrier.cars[ci].suppress_dwell_opposite_direction_flip = false;
        carrier.cars[ci].prev_floor = carrier.cars[ci].current_floor;
        recompute_car_target_and_direction(carrier, car_index);
        if !should_car_depart(carrier, car_index, time, lobby_mode) {
            carrier.cars[ci].dwell_counter = 1;
        }
    }
}

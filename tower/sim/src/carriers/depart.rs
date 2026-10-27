// ts: carriers/depart.ts

// 1098:23a5 should_car_depart
//
// Gate invoked at dwell-expiry inside `advance_carrier_car_state`. Forces
// departure when full, when schedule multiplier is 0, or when stopped at a
// non-home, non-lobby/express floor. Otherwise waits until the arrival-to-now
// delta exceeds multiplier * 30 ticks.

use crate::js;
use crate::time::TimeState;
use crate::world::{CarrierRecord, LobbyMode};

fn get_car_capacity(carrier: &CarrierRecord) -> i64 {
    carrier.assignment_capacity
}

fn get_schedule_index(time: &TimeState) -> i64 {
    time.weekend_flag * 7 + time.daypart_index
}

pub fn should_car_depart(carrier: &CarrierRecord, car_index: i64, time: &TimeState, lobby_mode: LobbyMode) -> bool {
    let car = &carrier.cars[car_index as usize];
    // Binary 1098:23a5.
    if car.assigned_count >= get_car_capacity(carrier) {
        return true;
    }
    let multiplier = js::at_or(&carrier.dwell_delay, get_schedule_index(time), 1);
    if multiplier == 0 {
        return true;
    }
    if car.current_floor != car.home_floor {
        // perfect-parity: dwell at floor 10 (ground) and 25, 40, 55, ...
        // (binary's `(currentFloor-10)%15 == 0` express-stop dwell rule).
        // modern: shift the cadence by +1 to follow lobbies that moved up
        // one floor (15, 30, 45, ... above ground).
        let cycle_offset = if lobby_mode == LobbyMode::Modern { 1 } else { 0 };
        let is_lobby_or_express = car.current_floor == 10
            || (car.current_floor > 10 && (car.current_floor - 10) % 15 == cycle_offset);
        if !is_lobby_or_express {
            return true;
        }
    }
    (time.day_tick - car.arrival_tick).abs() > multiplier * 30
}

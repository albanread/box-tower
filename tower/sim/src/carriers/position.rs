// ts: carriers/position.ts

// 1098:10e4 advance_car_position_one_step
//
// Steps the car one position toward its target. When already at target on
// entry, recomputes target+direction first (with prevFloor = cur latched
// before the recompute). Then computes motion mode, possibly arms the settle
// counter, steps by ±1 or ±3 (the ±3 is express-only — see motion.ts quirk),
// and clears arrivalSeen.

use crate::world::CarrierRecord;

use super::motion::compute_car_motion_mode;
use super::target::recompute_car_target_and_direction;

const DEPARTURE_SEQUENCE_TICKS: i64 = 5;

pub fn advance_car_position_one_step(carrier: &mut CarrierRecord, car_index: i64) {
    let ci = car_index as usize;
    if carrier.cars[ci].current_floor == carrier.cars[ci].target_floor {
        carrier.cars[ci].prev_floor = carrier.cars[ci].current_floor;
        recompute_car_target_and_direction(carrier, car_index);
    }

    let motion_mode = compute_car_motion_mode(carrier, car_index);
    let car = &mut carrier.cars[ci];
    if motion_mode == 0 {
        car.settle_counter = DEPARTURE_SEQUENCE_TICKS;
    } else if motion_mode == 1 {
        car.settle_counter = 2;
    }

    // Binary convention: direction==0 → down (cur -= step); direction!=0 → up.
    // Binary quirk: mode 3 (±3) is express-only — standard/service never hit
    // the 3-floor step path. Preserved in computeCarMotionMode.
    let step_size = if motion_mode == 3 { 3 } else { 1 };
    let step_dir = if car.direction_flag == 0 { -1 } else { 1 };
    car.current_floor += step_dir * step_size;

    if car.arrival_seen != 0 {
        car.arrival_seen = 0;
    }
}

// ts: carriers/motion.ts

// 1098:209f compute_car_motion_mode
//
// Returns 0=stop (seeds settle=5), 1=slow (seeds settle=2), 2=normal, 3=fast
// (±3 floors, express only). Binary quirk: mode 3 (±3) is gated to express
// carriers (mode==0) — standard/service carriers cannot reach it even when
// distances qualify. Do NOT "fix" this.

use crate::world::CarrierRecord;

/// Upstream's return type is the literal union `0 | 1 | 2 | 3`.
pub fn compute_car_motion_mode(carrier: &CarrierRecord, car_index: i64) -> i64 {
    let car = &carrier.cars[car_index as usize];
    let dist_to_target = (car.current_floor - car.target_floor).abs();
    let dist_from_prev = (car.current_floor - car.prev_floor).abs();

    // Binary 1098:209f. No firstLeg override — when distFromPrev < 2
    // (including 0 on departure), binary returns mode 0 (stop, stab=5).
    if carrier.carrier_mode == 0 {
        // Express: stop within 2, fast (±3) when both > 4, normal otherwise.
        // Binary quirk: express-only ±3 mode is not available to standard/service.
        if dist_to_target < 2 || dist_from_prev < 2 {
            return 0;
        }
        if dist_to_target > 4 && dist_from_prev > 4 {
            return 3;
        }
        return 2;
    }

    // Standard (1) and Service (2): stop within 2, slow-stop within 4.
    // Binary quirk: express-only mode 3 (±3 floors) stays disabled here.
    if dist_to_target < 2 || dist_from_prev < 2 {
        return 0;
    }
    if dist_to_target < 4 || dist_from_prev < 4 {
        return 1;
    }
    2
}

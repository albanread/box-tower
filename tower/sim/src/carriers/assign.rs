// ts: carriers/assign.ts

// 1098:0a4c assign_car_to_floor_request
// 1098:0dfc find_best_available_car_for_floor
//
// Floor-call assignment: picks the best car for a floor-call and writes the
// primary (up) / secondary (down) route-status slot with car+1.

use crate::world::CarrierRecord;

use super::slot::floor_to_slot;
use super::target::recompute_car_target_and_direction;

// Mirrors binary find_best_available_car_for_floor (1098:0dfc).
// Binary returns (int return_value, int *param_4). The caller
// assign_car_to_floor_request gates all its writes on `return_value != 0`.
// Cases that return 0 (fullAssign=false): A/B (same-floor, doors closed,
// scheduleFlag||dirMatch), C (idle-home candidate already at floor), D (dead).
// Normal path and degenerate: return 1 with param_4 set (fullAssign=true).
//
// The wrap-cost "turn floor" uses the car's `nearestWorkFloor` field (binary
// -0x51), refreshed at the tail of every `recomputeCarTargetAndDirection`.
// The idle-home test uses the static per-car `homeFloor` field (binary
// reachability_masks_by_floor[carIdx-8]), NOT `nearestWorkFloor` — this is
// what determines whether a parked car at its home is treated as available
// for an idle-home dispatch (binary 1098:0eef..0efe).
//
// The binary has NO assignedCount-vs-capacity early-skip — the per-leg
// capacity gate lives inside the per-direction phase scans of
// `selectNextTargetFloor` instead.
//
// Upstream returns `{ carIndex, fullAssign }`; here the pair
// `(car_index, full_assign)`.  Upstream's `Number.POSITIVE_INFINITY` start
// costs are `i64::MAX`: every real cost is a small integer below it, and the
// only arithmetic on a best cost (`bestMovingCost - bestIdleHomeCost`) is
// reached only when both have been set from real costs.
pub fn find_best_available_car_for_floor(carrier: &CarrierRecord, floor: i64, direction_flag: i64) -> (i64, bool) {
    let mut best_idle_home_cost = i64::MAX;
    let mut best_idle_home_index: i64 = -1;
    let mut best_forward_cost = i64::MAX;
    let mut best_forward_index: i64 = -1;
    let mut best_wrap_cost = i64::MAX;
    let mut best_wrap_index: i64 = -1;

    for (car_index, car) in carrier.cars.iter().enumerate() {
        let car_index = car_index as i64;
        if !car.active {
            continue;
        }

        // Cases A/B: car at floor with doors closed, and either scheduleFlag is
        // set or direction already matches. Binary returns 0 immediately.
        if car.current_floor == floor && car.settle_counter == 0 {
            if car.schedule_flag != 0 || car.direction_flag == direction_flag {
                return (car_index, false);
            }
            // else fall through to idle-home / forward / wrap accumulation
        }

        let turn_floor = car.nearest_work_floor;
        let distance = (floor - car.current_floor).abs();

        // Idle-home candidate: parked at static home floor with no work pending.
        // Binary 1098:0ee9..0efe: reads reachability_masks_by_floor[carIdx-8]
        // (== car.homeFloor) and compares to currentFloor. NOT nearestWorkFloor.
        let is_idle_home = car.pending_assignment_count == 0
            && car.nonempty_destination_count == 0
            && car.settle_counter == 0
            && car.current_floor == car.home_floor;

        if is_idle_home {
            // Case C: idle-home candidate already at floor
            if distance == 0 {
                return (car_index, false);
            }
            if distance < best_idle_home_cost {
                best_idle_home_cost = distance;
                best_idle_home_index = car_index;
            }
            continue;
        }

        if car.direction_flag == direction_flag {
            let forward = if direction_flag == 1 {
                floor - car.current_floor
            } else {
                car.current_floor - floor
            };
            if forward >= 0 {
                if forward == 0 && car.settle_counter == 0 {
                    // Case C alternate: same-floor same-direction with doors open.
                    return (car_index, false);
                }
                if forward < best_forward_cost {
                    best_forward_cost = forward;
                    best_forward_index = car_index;
                }
                continue;
            }
            // Past sweep end: wrap, with nearest_work_floor as turn floor.
            let cost = if direction_flag == 1 {
                turn_floor * 2 - car.current_floor - floor
            } else {
                car.current_floor + floor - turn_floor * 2
            };
            if cost < best_wrap_cost {
                best_wrap_cost = cost;
                best_wrap_index = car_index;
            }
            continue;
        }

        // Opposite direction: wrap via nearest_work as turn floor. Binary 1098:0fe0
        // branches on (request floor vs turn), not (request floor vs current): if
        // the request lies on the car's return leg past its reversal point, cost
        // is the wrap distance; otherwise the car passes the request going the
        // wrong way and the cost is the direct separation.
        let cost = if direction_flag == 1 {
            if turn_floor < floor {
                car.current_floor + floor - 2 * turn_floor
            } else {
                car.current_floor - floor
            }
        } else if floor < turn_floor {
            2 * turn_floor - car.current_floor - floor
        } else {
            floor - car.current_floor
        };
        if cost < best_wrap_cost {
            best_wrap_cost = cost;
            best_wrap_index = car_index;
        }
    }

    let best_moving_cost;
    let best_moving_index;
    if best_forward_index >= 0 {
        best_moving_cost = best_forward_cost;
        best_moving_index = best_forward_index;
    } else {
        best_moving_cost = best_wrap_cost;
        best_moving_index = best_wrap_index;
    }

    if best_idle_home_index >= 0 && best_moving_index >= 0 {
        // Binary quirk: equality breaks toward the idle-home car — the
        // comparator is strict `(moving - idle) < threshold`, so equal costs
        // fall through to the idle-home branch below.
        if best_moving_cost - best_idle_home_cost < carrier.waiting_car_response_threshold {
            return (best_moving_index, true);
        }
        return (best_idle_home_index, true);
    }
    if best_moving_index >= 0 {
        return (best_moving_index, true);
    }

    // Binary quirk (1098:10d0): degenerate fallback writes car index 0 and
    // returns fullAssign=true whenever there is NO forward and NO wrap
    // candidate — **even when** an idle-home candidate is tracked. The
    // binary's tail at 1098:1057 / 1098:108e only consults the idle-home
    // cost when paired with a moving candidate (threshold compare against
    // forward or wrap). Standalone idle-home is never used; the tail falls
    // through to 1098:10d0 which hardcodes index 0. Observed at
    // dense_office tick 1975: all 8 cars are idle-home for a direction-0
    // pickup at floor 18, wrap branch never fires (car.dir was opposite
    // but idle-home short-circuited the loop via JMP 0feb before reaching
    // 0fee), so forward=wrap=9999 and the scorer returns 0 — picking car 0
    // instead of the obvious car 6 at distance 1. Mirror the quirk.
    (0, true)
}

// Mirrors binary assign_car_to_floor_request (1098:0a4c). Top gate: for
// direction=1 (UP) requires primary[floor]==0; for direction=0 (DOWN)
// requires secondary[floor]==0. On fullAssign, writes table slot, increments
// pac, and recomputes target+direction — all unconditionally.
pub fn assign_car_to_floor_request(carrier: &mut CarrierRecord, floor: i64, direction_flag: i64) {
    let slot = floor_to_slot(carrier, floor);
    if slot < 0 {
        return;
    }
    let table = if direction_flag == 1 {
        &carrier.primary_route_status_by_floor
    } else {
        &carrier.secondary_route_status_by_floor
    };
    if crate::js::at_or(table, slot, 0) != 0 {
        return;
    }

    let (result_car_index, full_assign) = find_best_available_car_for_floor(carrier, floor, direction_flag);
    if result_car_index < 0 {
        return;
    }
    if !full_assign {
        return;
    }

    let car_index = result_car_index;
    // Binary 1098:0a4c body is straight-line — no iteration over a per-request
    // "assigned_car" field (there is none; floor-call ownership lives only in
    // `primary/secondary_route_status_by_floor`). TS used to seed the per-route
    // `assignedCarIndex` here so boardWaitingRoutes could gate on it, but that
    // cache goes stale after cancel-then-reassign (e.g., sky_office d3: a car
    // leaves f12 → cancelStaleFloorAssignment clears secondary[2] → next
    // assignCarToFloorRequest picks a new owner, yet pendingRoutes still point
    // at the departed car). The per-route field is now populated lazily by
    // `assignRequestToRuntimeRoute` at drain time (mirroring binary 1218:0d4e's
    // store_request_in_active_route_slot), which is the only place the binary
    // pairs a sim with a specific car.
    //
    // (The table is as long as the carrier's served span, which `slot` lies
    // within; JavaScript would grow it on a write past the end, Rust panics.)
    let table = if direction_flag == 1 {
        &mut carrier.primary_route_status_by_floor
    } else {
        &mut carrier.secondary_route_status_by_floor
    };
    table[slot as usize] = car_index + 1;
    carrier.cars[car_index as usize].pending_assignment_count += 1;
    recompute_car_target_and_direction(carrier, car_index);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carriers::make_carrier;

    #[test]
    fn all_idle_home_cars_fall_back_to_car_zero() {
        // Four idle cars parked at home (floor 10); a down call at floor 18 has
        // no forward or wrap candidate, so the binary's degenerate tail picks
        // car 0 even though every car is an idle-home candidate.
        let carrier = make_carrier(0, 5, 1, 10, 30, Some(4));
        assert_eq!(find_best_available_car_for_floor(&carrier, 18, 0), (0, true));
        // A car already at the floor with doors closed and matching direction.
        let mut carrier = make_carrier(0, 5, 1, 10, 30, Some(2));
        carrier.cars[1].current_floor = 18;
        carrier.cars[1].pending_assignment_count = 1;
        assert_eq!(find_best_available_car_for_floor(&carrier, 18, 1), (1, false));
    }

    #[test]
    fn idle_home_wins_ties_against_a_moving_car() {
        let mut carrier = make_carrier(0, 5, 1, 10, 30, Some(2));
        // Car 1 moving up from 12 with work: forward cost to 17 is 5.
        carrier.cars[1].current_floor = 12;
        carrier.cars[1].pending_assignment_count = 1;
        // Car 0 idle at home 10: cost 7. moving - idle = -2 < 5: the mover.
        assert_eq!(find_best_available_car_for_floor(&carrier, 17, 1), (1, true));
        // Move car 1 down to floor 2 (the span widened to reach it).
        carrier.cars[1].current_floor = 2;
        carrier.bottom_served_floor = 0;
        // forward 15, idle 7: 15 - 7 = 8, not < 5: the idle-home car.
        assert_eq!(find_best_available_car_for_floor(&carrier, 17, 1), (0, true));
    }
}

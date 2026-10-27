// ts: queue/process-travel.ts

// 1218:0351 process_unit_travel_queue
// 1218:0d4e assign_request_to_runtime_route
//
// Per-tick queue-drain + boarding pass. Called from the outer carrier-tick
// loop after `dispatch_carrier_car_arrivals`.

use crate::carriers::slot::floor_to_slot;
use crate::carriers::sync::{
    active_slot_limit_for, add_route_slot, find_route_index_by_id, has_active_slot, normalize_inactive_slots,
    sync_assignment_status, sync_pending_route_ids,
};
use crate::carriers::target::recompute_car_target_and_direction;
use crate::js;
use crate::reachability::mask_tests::choose_transfer_floor_from_carrier_reachability;
use crate::resources::{
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN, FAMILY_OFFICE,
    FAMILY_RESTAURANT, FAMILY_RETAIL,
};
use crate::sim_access::state_bits::set_sim_in_transit;
use crate::stress::accumulate_elapsed::accumulate_elapsed_delay_into_current_sim;
use crate::stress::add_delay::add_delay_to_current_sim;

/// (Upstream's `new Set<number>([...])`; a constant set is a slice.)
const STATE_BIT_FAMILIES: &[i64] = &[
    FAMILY_HOTEL_SINGLE,
    FAMILY_HOTEL_TWIN,
    FAMILY_HOTEL_SUITE,
    FAMILY_RESTAURANT,
    FAMILY_OFFICE,
    FAMILY_CONDO,
    FAMILY_FAST_FOOD,
    FAMILY_RETAIL,
];

use crate::time::TimeState;
use crate::world::{CarrierFloorQueue, CarrierRecord, RouteState, SimRecord, WorldState};

use super::route_record::RouteRequestRing;

const REQUEUE_FAILURE_DELAY: i64 = 0;

/// Upstream's inline `${floorAnchor}:${homeColumn}:${familyCode}:${baseOffset}`.
fn route_key(sim: &SimRecord) -> String {
    format!("{}:{}:{}:{}", sim.floor_anchor, sim.home_column, sim.family_code, sim.base_offset)
}

/// Binary `assign_request_to_runtime_route` (1218:0d4e) invokes
/// `accumulate_elapsed_delay_into_current_sim` for the assigned sim,
/// which rebases `elapsed_packed` from the clock and applies the lobby
/// discount for the request source floor.
///
/// Only non-service carriers (mode != 2) perform the rebase; service
/// carriers do not update sim stress.
///
/// (`carrier` is `world.carriers[carrier_index]`.)
fn apply_assignment_stress_update(
    world: &mut WorldState,
    time: &TimeState,
    carrier_index: usize,
    route_id: &str,
    source_floor: i64,
) {
    if world.carriers[carrier_index].carrier_mode == 2 {
        return;
    }
    let Some(sim_index) = world.sims.iter().position(|candidate| route_key(candidate) == route_id) else {
        return;
    };
    accumulate_elapsed_delay_into_current_sim(world, time, sim_index as i64, source_floor);
}

fn get_schedule_index(time: &TimeState) -> i64 {
    time.weekend_flag * 7 + time.daypart_index
}

fn get_car_capacity(carrier: &CarrierRecord) -> i64 {
    carrier.assignment_capacity
}

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

fn clear_sim_route_by_id(world: &mut WorldState, sim_id: &str) {
    for sim in world.sims.iter_mut() {
        let key = route_key(sim);
        if key != sim_id {
            continue;
        }
        sim.route = RouteState::Idle;
        // Phase 5b bit-sync: failed transfer-floor resolution returns the
        // sim to idle. We strip only the 0x40 in-transit bit — 0x20 overlaps
        // TS phase encodings (MORNING_GATE = 0x20 etc.) and stripping it
        // would corrupt the post-failure phase byte. Gated to
        // dispatch_sim_behavior families only.
        if STATE_BIT_FAMILIES.contains(&sim.family_code) {
            set_sim_in_transit(sim, false);
        }
        return;
    }
}

/// Binary `assign_request_to_runtime_route` (1218:0d4e). Moves a popped
/// request from the floor ring into one of the car's active-route slots.
/// Runs the transfer-floor resolver (so multi-hop routes pick the right
/// intermediate destination) and, on failure, applies
/// `g_requeue_failure_delay` (currently 0) and force-dispatches the sim
/// back through its family handler.
///
/// (`carrier` is `world.carriers[carrier_index]`, `car` is that carrier's
/// `cars[car_index]`, `route` its `pending_routes[route_index]`.  Upstream's
/// `carIndex` parameter is optional; its one caller always passes it, so
/// here it is the car's index and always present.)
pub fn assign_request_to_runtime_route(
    world: &mut WorldState,
    time: &TimeState,
    carrier_index: i64,
    car_index: i64,
    route_index: i64,
) -> bool {
    let ci = carrier_index as usize;
    let ki = car_index as usize;
    let ri = route_index as usize;
    let resolved_floor = {
        let carrier = &world.carriers[ci];
        choose_transfer_floor_from_carrier_reachability(
            world,
            carrier.carrier_id,
            carrier.cars[ki].current_floor,
            carrier.pending_routes[ri].destination_floor,
        )
    };
    if resolved_floor < 0 {
        let sim_id = world.carriers[ci].pending_routes[ri].sim_id.clone();
        world.carriers[ci].pending_routes.retain(|candidate| candidate.sim_id != sim_id);
        // Binary assign_request_to_runtime_route: on transfer-floor failure,
        // adds g_requeue_failure_delay then calls force_dispatch_sim_state_by_family,
        // which re-runs the family handler so routing is retried next stride.
        let failed_sim = world.sims.iter().position(|s| route_key(s) == sim_id);
        if let Some(i) = failed_sim {
            add_delay_to_current_sim(&mut world.sims[i], REQUEUE_FAILURE_DELAY);
        }
        clear_sim_route_by_id(world, &sim_id);
        return false;
    }
    world.carriers[ci].pending_routes[ri].destination_floor = resolved_floor;
    // Binary 1218:0d4e: store_request_in_active_route_slot writes the request
    // into THIS car's slot table — there's no per-car-index check on the slot
    // itself. If the route was previously assigned to a different car (via
    // assign_car_to_floor_request), the slot ownership transfers to the actual
    // boarding car. Mirror that by reassigning here so `boardWaitingRoutes` can
    // fire on the next pass without being blocked by a stale carIndex.
    world.carriers[ci].pending_routes[ri].assigned_car_index = car_index;
    let stored = add_route_slot(&mut world.carriers[ci], car_index, route_index);
    if stored {
        let route = &world.carriers[ci].pending_routes[ri];
        let (sim_id, source_floor) = (route.sim_id.clone(), route.source_floor);
        apply_assignment_stress_update(world, time, ci, &sim_id, source_floor);
    }
    if stored && world.carriers[ci].cars[ki].dwell_counter == 0 {
        recompute_car_target_and_direction(&mut world.carriers[ci], car_index);
    }
    stored
}

/// Binary 1218:0351 drains the queue strictly FIFO via repeated
/// `pop_unit_queue_request` + `assign_request_to_runtime_route` calls.
///
/// (Upstream's `drainDirection` closure inside `drainFloorQueueForCar`:
/// `remaining_slots`, `pop_cap` and the floor queue's index `floor_queue`
/// are the variables it captures.)
#[allow(clippy::too_many_arguments)]
fn drain_direction(
    world: &mut WorldState,
    time: &TimeState,
    ci: usize,
    car_index: i64,
    floor_queue: usize,
    pop_cap: i64,
    remaining_slots: &mut i64,
    direction_flag: i64,
) {
    // (Upstream's __DRAIN_TRACE__ logging is left out: TRANSLATION.md.)
    //
    // Binary 1218:0351 drains the queue strictly FIFO via repeated
    // `pop_unit_queue_request` + `assign_request_to_runtime_route` calls.
    // Pops `queue_count` items from the head (capped at `remainingSlots`
    // when dc==1, else 1). Each pop processes whoever is at the head; if
    // assignment fails the binary calls `force_dispatch_sim_state_by_family`
    // which may re-enqueue at the tail. There is NO per-car-index filter
    // in the binary; pre-assignment via `primary_route_status_by_floor`
    // selects a single car at enqueue time and that car drains the queue.
    //
    // Earlier code built a filtered preview of the queue (skipping boarded
    // duplicates and routes assigned to other cars) and then called
    // `buf.pop()` once per filtered match. Because pop() always removes the
    // head, the pop and the assigned route diverged: a popped head that
    // did not match the filter was discarded but the matching route a few
    // positions deeper was assigned without being removed from the queue,
    // leaving a stale duplicate. Subsequent pulses then re-popped that
    // duplicate, creating a self-perpetuating set of stale entries that
    // caused trace-test divergence (e.g., sim 22 in build_dense_office at
    // day=0 tick=187 boarded car 1 instead of car 5).
    //
    // Strict-FIFO behavior matching the binary: pop from the head and
    // attempt `assign_request_to_runtime_route`. Cap on REAL assignment
    // attempts (mirroring the binary's `queue_count`); stale duplicates
    // are silently popped without consuming an attempt, since the binary
    // never produces them in the first place. Cleaning them up is just a
    // background side effect of the strict-FIFO drain.
    let max_assignments = pop_cap.min(*remaining_slots);
    let mut assignment_attempts = 0;
    loop {
        if !(assignment_attempts < max_assignments
            && get_direction_queue(&mut world.carriers[ci].floor_queues[floor_queue], direction_flag).size() > 0)
        {
            break;
        }
        let route_id = get_direction_queue(&mut world.carriers[ci].floor_queues[floor_queue], direction_flag).pop();
        let Some(route_id) = route_id else {
            break;
        };
        let carrier = &world.carriers[ci];
        let Some(route_index) = find_route_index_by_id(carrier, &route_id) else {
            continue;
        };
        let route = &carrier.pending_routes[route_index as usize];
        if route.boarded || has_active_slot(&carrier.cars[car_index as usize], &route.sim_id) {
            // Stale duplicate left over from a prior pulse — drop and keep
            // popping without consuming an assignment attempt.
            continue;
        }
        assignment_attempts += 1;
        if assign_request_to_runtime_route(world, time, ci as i64, car_index, route_index) {
            *remaining_slots -= 1;
        }
    }
}

/// (`carrier` is `world.carriers[carrier_index]`, `car` is that carrier's
/// `cars[car_index]`.)
fn drain_floor_queue_for_car(world: &mut WorldState, carrier_index: i64, car_index: i64, time: &TimeState) {
    let ci = carrier_index as usize;
    let ki = car_index as usize;
    // When schedule is disabled, car does not pick up passengers
    let schedule_index = get_schedule_index(time);
    if js::at_or(&world.carriers[ci].service_schedule_flags, schedule_index, 1) == 0 {
        return;
    }

    let mut remaining_slots = {
        let carrier = &world.carriers[ci];
        0.max(get_car_capacity(carrier) - carrier.cars[ki].assigned_count)
    };
    if remaining_slots == 0 {
        return;
    }

    let Some(floor_queue) = get_queue_state(&world.carriers[ci], world.carriers[ci].cars[ki].current_floor) else {
        return;
    };

    // Binary 1218:0351 flip-during-dwell: when the car's current-direction
    // queue is empty, no routes are in-flight (pac==0), and no onboard riders
    // (ndc==0), check the opposite direction. If that queue has entries, flip
    // the car's direction flag and drain from the flipped direction.
    {
        let carrier = &mut world.carriers[ci];
        let car_direction = carrier.cars[ki].direction_flag;
        let q_cur_size = get_direction_queue(&mut carrier.floor_queues[floor_queue], car_direction).size();
        let car = &carrier.cars[ki];
        if q_cur_size == 0 && car.pending_assignment_count == 0 && car.nonempty_destination_count == 0 {
            let opposite_direction = if car.direction_flag == 1 { 0 } else { 1 };
            let q_opp_size = get_direction_queue(&mut carrier.floor_queues[floor_queue], opposite_direction).size();
            if q_opp_size != 0 {
                carrier.cars[ki].direction_flag = opposite_direction;
            }
        }
    }

    // Binary 1218:0351: pop cap per direction is 1 unless dwellCounter == 1
    // exactly (then cap = remainingSlots).
    let pop_cap = if world.carriers[ci].cars[ki].dwell_counter == 1 { remaining_slots } else { 1 };

    let primary_direction = world.carriers[ci].cars[ki].direction_flag;
    drain_direction(world, time, ci, car_index, floor_queue, pop_cap, &mut remaining_slots, primary_direction);

    // Binary: alternate-direction opportunistic drain when scheduleFlag != 0.
    if world.carriers[ci].cars[ki].schedule_flag != 0 && remaining_slots > 0 {
        let opposite_direction = if primary_direction == 1 { 0 } else { 1 };
        drain_direction(world, time, ci, car_index, floor_queue, pop_cap, &mut remaining_slots, opposite_direction);
    }

    sync_assignment_status(&mut world.carriers[ci]);
    if world.carriers[ci].cars[ki].dwell_counter == 0 {
        recompute_car_target_and_direction(&mut world.carriers[ci], car_index);
    }
}

// Boarding-only half. Mirrors the per-slot board step inside
// process_unit_travel_queue (1218:0351) — moves riders from active-route
// slots (populated by drainFloorQueueForCar) onto the car. Stress
// accumulation belongs to `assignRequestToRuntimeRoute`, matching binary
// `assign_request_to_runtime_route` (1218:0d4e), not this board step.
//
// (`car` is `carrier.cars[car_index]`.)
fn board_waiting_routes(carrier: &mut CarrierRecord, car_index: i64) -> bool {
    let ki = car_index as usize;
    let mut changed = false;
    let limit = active_slot_limit_for(carrier);

    for index in 0..limit.max(0) as usize {
        if carrier.cars[ki].assigned_count >= get_car_capacity(carrier) {
            break;
        }
        let Some(slot) = carrier.cars[ki].active_route_slots.get(index) else {
            continue;
        };
        if !slot.active || slot.boarded {
            continue;
        }
        let Some(route_index) = find_route_index_by_id(carrier, &slot.route_id) else {
            continue;
        };
        let ri = route_index as usize;
        let route = &carrier.pending_routes[ri];
        if route.boarded {
            continue;
        }
        if route.assigned_car_index != car_index && route.assigned_car_index != -1 {
            continue;
        }
        if route.source_floor != carrier.cars[ki].current_floor {
            continue;
        }
        let route = &mut carrier.pending_routes[ri];
        route.assigned_car_index = car_index;
        route.boarded = true;
        let route_destination_floor = route.destination_floor;
        let car = &mut carrier.cars[ki];
        car.active_route_slots[index].boarded = true;
        car.assigned_count += 1;
        let destination_slot = floor_to_slot(carrier, route_destination_floor);
        if destination_slot >= 0 {
            let car = &mut carrier.cars[ki];
            let prev = js::at_or(&car.destination_count_by_floor, destination_slot, 0);
            // (Upstream writes `destinationCountByFloor[destinationSlot]`;
            // the slot is always within the array, which has one entry per
            // served floor.)
            car.destination_count_by_floor[destination_slot as usize] = prev + 1;
            if prev == 0 {
                car.nonempty_destination_count += 1;
            }
        }
        changed = true;
    }

    if changed {
        let car = &mut carrier.cars[ki];
        normalize_inactive_slots(car);
        sync_pending_route_ids(car);
        sync_assignment_status(carrier);
        if carrier.cars[ki].dwell_counter == 0 {
            recompute_car_target_and_direction(carrier, car_index);
        }
    }
    changed
}

/// Binary `process_unit_travel_queue` (1218:0351). Pops riders from the
/// carrier's per-floor queue into this car's active-route slots, then
/// boards any slot whose source matches the current floor.
///
/// Binary gates the queue pop on `(car[-0x5c] & 1) != 0` — only runs when
/// dwellCounter is odd. This creates the 1-tick lag between route enqueue
/// (at dwell=4) and boarding (at dwell=3).
///
/// (`carrier` is `world.carriers[carrier_index]`, `car` is that carrier's
/// `cars[car_index]`.)
pub fn process_unit_travel_queue(world: &mut WorldState, carrier_index: i64, car_index: i64, time: &TimeState) {
    let ci = carrier_index as usize;
    let ki = car_index as usize;
    if !world.carriers[ci].cars[ki].active {
        return;
    }
    if js::to_int32(world.carriers[ci].cars[ki].dwell_counter) & 1 != 0 {
        drain_floor_queue_for_car(world, carrier_index, car_index, time);
    }
    board_waiting_routes(&mut world.carriers[ci], car_index);
    {
        let carrier = &world.carriers[ci];
        let car = &carrier.cars[ki];
        if car.dwell_counter == 1
            && time.day_counter == 1
            && car.direction_flag == 0
            && car.current_floor <= carrier.bottom_served_floor + 2
            && car.dwell_start_pending_assignment_count > 0
            && car.pending_assignment_count == 0
        {
            recompute_car_target_and_direction(&mut world.carriers[ci], car_index);
        }
    }
    {
        let carrier = &world.carriers[ci];
        let car = &carrier.cars[ki];
        if car.dwell_counter == 3
            && time.day_counter > 0
            && car.direction_flag == 0
            && car.current_floor == carrier.bottom_served_floor
            && car.target_floor == car.current_floor
            && car.assigned_count == 0
            && car.dwell_start_pending_assignment_count > 0
            && car.pending_assignment_count == 0
        {
            recompute_car_target_and_direction(&mut world.carriers[ci], car_index);
            drain_floor_queue_for_car(world, carrier_index, car_index, time);
            board_waiting_routes(&mut world.carriers[ci], car_index);
        }
    }
    {
        let car = &world.carriers[ci].cars[ki];
        if car.dwell_counter == 0 && car.settle_counter > 0 && car.current_floor == car.target_floor {
            recompute_car_target_and_direction(&mut world.carriers[ci], car_index);
        }
    }
    {
        let car = &world.carriers[ci].cars[ki];
        if car.dwell_counter == 0
            && car.settle_counter > 0
            && car.assigned_count == 0
            && car.pending_assignment_count == 0
            && car.nonempty_destination_count == 0
        {
            recompute_car_target_and_direction(&mut world.carriers[ci], car_index);
        }
    }
}

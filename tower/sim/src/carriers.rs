// ts: carriers.ts

// Carrier module hub. After Phase 3 of the routing refactor, the route-queue
// ops (enqueue_request_into_route_queue, pop_unit_queue_request,
// process_unit_travel_queue, assign_request_to_runtime_route,
// dispatch_carrier_car_arrivals, dispatch_destination_queue_entries,
// store_request_in_active_route_slot, pop_active_route_slot_request,
// remove_request_from_unit_queue, remove_request_from_active_route_slots,
// cancel_runtime_route_request, dispatch_queued_route_until_request,
// decrement_route_queue_direction_load, decode_runtime_route_target,
// resolve_sim_route_between_floors) live under queue/*.ts with one binary
// function per file. The per-car state machine (advance_carrier_car_state etc.)
// lives under carriers/*.ts.
//
// This file retains:
//   - Carrier record / car constructors and world-level lifecycle
//     (makeCarrier, makeCarrierCar, rebuildCarrierList, initCarrierState,
//      flushCarriersEndOfDay).
//   - resetCarrierTickBookkeeping.
//   - The re-exports for state-machine + queue functions so existing import
//     sites keep working (enqueueCarrierRoute aliases
//     enqueueRequestIntoRouteQueue; evictCarrierRoute aliases
//     cancelRuntimeRouteRequest).
//
// (Upstream's carriers/index.ts is only a re-export barrel for this file; it
// has no module of its own here, and its re-exports are the `pub use` lines
// below.)

pub mod advance;
pub mod arrival;
pub mod assign;
pub mod depart;
pub mod motion;
pub mod pending;
pub mod position;
pub mod slot;
pub mod sync;
pub mod target;

use indexmap::{IndexMap, IndexSet};

use crate::queue::route_record::RouteRequestRing;
use crate::world::{y_to_floor, CarrierCar, CarrierFloorQueue, CarrierRecord, CarrierRouteSlot, WorldState};

use self::sync::sync_assignment_status;

// Re-export the split state-machine functions so existing imports keep
// resolving to `./carriers`.
pub use self::advance::advance_carrier_car_state;
pub use self::arrival::{cancel_stale_floor_assignment, clear_floor_requests_on_arrival, reset_out_of_range_car};
pub use self::assign::{assign_car_to_floor_request, find_best_available_car_for_floor};
pub use self::depart::should_car_depart;
pub use self::motion::compute_car_motion_mode;
pub use self::pending::decrement_car_pending_assignment_count;
pub use self::position::advance_car_position_one_step;
pub use self::target::{
    find_nearest_work_floor, recompute_car_target_and_direction, select_next_target_floor, update_car_direction_flag,
};
pub use crate::queue::dispatch_arrivals::dispatch_carrier_car_arrivals;
pub use crate::queue::process_travel::process_unit_travel_queue;

pub use self::slot::{carrier_serves_floor, carrier_spans_floor, floor_to_slot};

// Binary-alias re-exports for the queue ops — existing call sites use the
// older TS names. `enqueueCarrierRoute` maps to
// `enqueue_request_into_route_queue` (1218:1002); `evictCarrierRoute` maps to
// `cancel_runtime_route_request` (1218:1a86).
pub use crate::queue::cancel::cancel_runtime_route_request as evict_carrier_route;
pub use crate::queue::enqueue::enqueue_request_into_route_queue as enqueue_carrier_route;

const ACTIVE_SLOT_CAPACITY: i64 = 42;

fn create_floor_queue() -> CarrierFloorQueue {
    CarrierFloorQueue {
        up: RouteRequestRing::new(),
        down: RouteRequestRing::new(),
    }
}

fn empty_route_slot() -> CarrierRouteSlot {
    CarrierRouteSlot {
        route_id: String::new(),
        source_floor: 0xff,
        destination_floor: 0xff,
        boarded: false,
        active: false,
    }
}

pub fn make_carrier_car(num_slots: i64, home_floor: i64) -> CarrierCar {
    CarrierCar {
        active: true,
        current_floor: home_floor,
        settle_counter: 0,
        dwell_counter: 0,
        assigned_count: 0,
        pending_assignment_count: 0,
        dwell_start_pending_assignment_count: 0,
        direction_flag: 1,
        target_floor: home_floor,
        prev_floor: home_floor,
        home_floor,
        nearest_work_floor: home_floor,
        schedule_flag: 0,
        arrival_seen: 0,
        arrival_tick: 0,
        arrival_dispatch_this_tick: false,
        arrival_dispatch_starting_assigned_count: 0,
        suppress_dwell_opposite_direction_flip: false,
        waiting_count: vec![0; num_slots as usize],
        destination_count_by_floor: vec![0; num_slots as usize],
        nonempty_destination_count: 0,
        active_route_slots: (0..ACTIVE_SLOT_CAPACITY).map(|_| empty_route_slot()).collect(),
        pending_route_ids: Vec::new(),
    }
}

/// `mode` is upstream's `0 | 1 | 2`; `num_cars` defaults to 1.
pub fn make_carrier(id: i64, col: i64, mode: i64, bottom: i64, top: i64, num_cars: Option<i64>) -> CarrierRecord {
    let num_cars = num_cars.unwrap_or(1);
    let num_slots = top - bottom + 1;
    let clamped_cars = 1.max(8.min(num_cars));
    // Binary `place_carrier_shaft` / `_add_shaft_cars` writes
    // bottom_served_floor to every car's home_floor byte at carrier+0xBA..0xC1.
    // Find-best-car's idle-home test (`car.currentFloor == car.homeFloor`)
    // observes this all-bottom layout, so spacing the homes here would let an
    // idle-home tiebreak fire on the wrong cars.
    let cars = (0..clamped_cars).map(|_| make_carrier_car(num_slots, bottom)).collect();

    CarrierRecord {
        carrier_id: id,
        column: col,
        carrier_mode: mode,
        top_served_floor: top,
        bottom_served_floor: bottom,
        served_floor_flags: vec![1; 14],
        primary_route_status_by_floor: vec![0; num_slots as usize],
        secondary_route_status_by_floor: vec![0; num_slots as usize],
        service_schedule_flags: vec![1; 14],
        dwell_delay: vec![0; 14],
        express_direction_flags: vec![0; 14],
        waiting_car_response_threshold: 5,
        assignment_capacity: if mode == 0 { 0x2a } else { 0x15 },
        floor_queues: (0..num_slots).map(|_| create_floor_queue()).collect(),
        pending_routes: Vec::new(),
        completed_route_ids: Vec::new(),
        suppressed_floor_assignments: Vec::new(),
        stop_floor_enabled: vec![1; num_slots as usize],
        cars,
    }
}

/// Exported handle for the new tick/carrier-tick.ts path. Runs the per-carrier
/// bookkeeping reset (completedRouteIds + syncAssignmentStatus) that used to
/// live at the head of tickAllCarriers. Kept here so all carrier-internal
/// invariants live in one module.
pub fn reset_carrier_tick_bookkeeping(carrier: &mut CarrierRecord) {
    carrier.completed_route_ids = Vec::new();
    for car in carrier.cars.iter_mut() {
        car.arrival_dispatch_this_tick = false;
        car.arrival_dispatch_starting_assigned_count = 0;
    }
    sync_assignment_status(carrier);
}

/// End-of-day carrier flush (checkpoint 0x9f6).
/// Drains all floor queues and clears pending route tracking. The binary
/// leaves per-car state (dwell counter, current/target floor, cycle phase)
/// untouched at the day boundary — resetting mid-cycle would shift the
/// reselect cadence and desynchronize dispatch from the binary.
pub fn flush_carriers_end_of_day(world: &mut WorldState) {
    for carrier in world.carriers.iter_mut() {
        for queue in carrier.floor_queues.iter_mut() {
            queue.up.count = 0;
            queue.down.count = 0;
        }
        carrier.pending_routes = Vec::new();
        carrier.completed_route_ids = Vec::new();
    }
}

/// `Number(s)` for the coordinate strings of a cell key ("x,y").
fn key_number(s: &str) -> i64 {
    s.parse().unwrap_or_else(|_| panic!("cell key coordinate {s:?} is not an integer"))
}

pub fn rebuild_carrier_list(world: &mut WorldState) {
    // Upstream: Map<number, { floors: Set<number>; mode: 0 | 1 | 2 }>.
    let mut columns: IndexMap<i64, (IndexSet<i64>, i64)> = IndexMap::new();
    let mut overlay_keys: IndexSet<String> = IndexSet::new();
    for key in world.overlays.keys() {
        overlay_keys.insert(key.to_string());
    }
    for key in world.overlay_to_anchor.keys() {
        overlay_keys.insert(key.to_string());
    }

    for key in &overlay_keys {
        let anchor_key = world.overlay_to_anchor.get(key).cloned().unwrap_or_else(|| key.clone());
        let Some(r#type) = world.overlays.get(&anchor_key) else {
            continue;
        };
        let mode = if r#type == "elevator" {
            1
        } else if r#type == "elevatorExpress" {
            0
        } else if r#type == "elevatorService" {
            2
        } else {
            continue;
        };

        let anchor_x_str = anchor_key.split(',').next().unwrap_or("");
        let y_str = key.split(',').nth(1).unwrap_or("");
        let x = key_number(anchor_x_str);
        let y = key_number(y_str);
        let floor = y_to_floor(y);

        columns.entry(x).or_insert_with(|| (IndexSet::new(), mode)).0.insert(floor);
    }

    // Split each column into contiguous floor runs. Two elevator overlays in
    // the same column with a vertical gap form two independent carriers — the
    // binary allows this, and routing/economy track each segment separately.
    struct Run {
        col: i64,
        mode: i64,
        bottom: i64,
        top: i64,
    }
    let mut runs: Vec<Run> = Vec::new();
    for (&col, (floors, mode)) in &columns {
        let mode = *mode;
        let mut sorted: Vec<i64> = floors.iter().copied().collect();
        sorted.sort_by(|a, b| a.cmp(b));
        let mut run_bottom = sorted[0];
        let mut prev = sorted[0];
        for &f in sorted.iter().skip(1) {
            if f == prev + 1 {
                prev = f;
                continue;
            }
            runs.push(Run { col, mode, bottom: run_bottom, top: prev });
            run_bottom = f;
            prev = f;
        }
        runs.push(Run { col, mode, bottom: run_bottom, top: prev });
    }

    // Upstream's `consumed` set of carrier objects: an old carrier, once
    // matched, is moved out of `old` (leaving None), which is the same test.
    let mut old: Vec<Option<CarrierRecord>> = std::mem::take(&mut world.carriers).into_iter().map(Some).collect();
    let mut new_carriers: Vec<CarrierRecord> = Vec::new();
    let mut id: i64 = 0;
    for run in &runs {
        let Run { col, mode, bottom, top } = *run;
        let num_slots = top - bottom + 1;

        // Match this run to whichever existing same-column carrier overlaps it
        // most. Preserves per-car state through ordinary extend/shrink edits;
        // when a single placement merges or splits segments, the larger overlap
        // wins and the smaller side starts fresh.
        let mut existing_index: Option<usize> = None;
        let mut best_overlap = 0;
        for (i, c) in old.iter().enumerate() {
            let Some(c) = c else { continue };
            if c.column != col {
                continue;
            }
            let overlap = c.top_served_floor.min(top) - c.bottom_served_floor.max(bottom) + 1;
            if overlap > best_overlap {
                existing_index = Some(i);
                best_overlap = overlap;
            }
        }

        if let Some(i) = existing_index {
            let mut existing = old[i].take().expect("matched carrier");
            existing.carrier_id = id;
            id += 1;
            existing.carrier_mode = mode;
            existing.top_served_floor = top;
            existing.bottom_served_floor = bottom;
            // (`waitingCarResponseThreshold ??= 5` and `assignmentCapacity ??=`
            // do nothing here: the fields are never missing in Rust.)
            if existing.served_floor_flags.len() != 14 {
                existing.served_floor_flags = vec![1; 14];
            }
            if existing.service_schedule_flags.len() != 14 {
                existing.service_schedule_flags = vec![1; 14];
            }
            if existing.dwell_delay.len() != 14 {
                existing.dwell_delay = vec![0; 14];
            }
            if existing.express_direction_flags.len() != 14 {
                existing.express_direction_flags = vec![0; 14];
            }
            // (`completedRouteIds ??= []`, `suppressedFloorAssignments ??= []`:
            // never missing.)
            if existing.stop_floor_enabled.len() as i64 != num_slots {
                existing.stop_floor_enabled = vec![1; num_slots as usize];
            }
            if existing.primary_route_status_by_floor.len() as i64 != num_slots {
                existing.primary_route_status_by_floor = vec![0; num_slots as usize];
                existing.secondary_route_status_by_floor = vec![0; num_slots as usize];
            }
            if existing.floor_queues.len() as i64 != num_slots {
                existing.floor_queues = (0..num_slots).map(|_| create_floor_queue()).collect();
            }
            for car_index in 0..existing.cars.len() {
                let car = &mut existing.cars[car_index];
                if car.waiting_count.len() as i64 != num_slots {
                    car.waiting_count = vec![0; num_slots as usize];
                }
                if car.destination_count_by_floor.len() as i64 != num_slots {
                    car.destination_count_by_floor = vec![0; num_slots as usize];
                }
                // (`nonemptyDestinationCount ??= 0`, `activeRouteSlots ??= ...`:
                // never missing.)
                car.home_floor = top.min(bottom.max(car.home_floor));
                // (`car.nearestWorkFloor === undefined` cannot happen.)
                if car.nearest_work_floor < bottom || car.nearest_work_floor > top {
                    car.nearest_work_floor = car.home_floor;
                }
                if car.current_floor < bottom || car.current_floor > top {
                    reset_out_of_range_car(&mut existing, car_index as i64);
                }
            }
            new_carriers.push(existing);
        } else {
            new_carriers.push(make_carrier(id, col, mode, bottom, top, None));
            id += 1;
        }
    }

    world.carriers = new_carriers;
    for carrier in world.carriers.iter_mut() {
        sync_assignment_status(carrier);
    }
}

/// Upstream fills in `world.carriers`, `world.floorWalkabilityFlags` and
/// `world.transferGroupCache` where a saved world lacks them (`??=`).  The
/// Rust world always has all three (a snapshot without them does not
/// deserialise), so there is nothing to do.
pub fn init_carrier_state(world: &mut WorldState) {
    let _ = world;
}

#[cfg(test)]
mod tests {
    use super::*;

    // carriers.test.ts
    #[test]
    fn flush_preserves_route_ring_heads_while_clearing_queue_counts() {
        let carrier = make_carrier(0, 100, 1, 10, 19, Some(1));
        let mut world = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        world.carriers = vec![carrier];
        {
            let queue = &mut world.carriers[0].floor_queues[0].up;
            for i in 0..15 {
                queue.push(&format!("r{i}"));
            }
            for _ in 0..11 {
                queue.pop();
            }
            assert_eq!(queue.head, 11);
            assert_eq!(queue.size(), 4);
        }

        flush_carriers_end_of_day(&mut world);

        let queue = &mut world.carriers[0].floor_queues[0].up;
        assert_eq!(queue.head, 11);
        assert_eq!(queue.size(), 0);

        queue.push("next");
        assert_eq!(queue.head, 11);
        assert_eq!(queue.peek_all(), ["next"]);
    }

    fn world_with_overlays(overlays: &[(&str, &str)], to_anchor: &[(&str, &str)]) -> WorldState {
        let mut world = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        for (k, v) in overlays {
            world.overlays.insert(*k, v.to_string());
        }
        for (k, v) in to_anchor {
            world.overlay_to_anchor.insert(*k, v.to_string());
        }
        world
    }

    #[test]
    fn rebuild_splits_columns_into_runs_and_keeps_matched_state() {
        // Column 5: floors 10..12 and 14 (a gap at 13); column 9 express 10..11.
        let y = |floor: i64| crate::world::floor_to_y(floor);
        let k = |x: i64, f: i64| format!("{x},{}", y(f));
        let (a10, a11, a12, a14) = (k(5, 10), k(5, 11), k(5, 12), k(5, 14));
        let (e10, e11) = (k(9, 10), k(9, 11));
        let mut world = world_with_overlays(
            &[(&a10, "elevator"), (&a14, "elevator"), (&e10, "elevatorExpress"), ("1,1", "stairs")],
            &[(&a11, &a10), (&a12, &a10), (&e11, &e10)],
        );
        rebuild_carrier_list(&mut world);
        let spans: Vec<_> = world
            .carriers
            .iter()
            .map(|c| (c.carrier_id, c.column, c.carrier_mode, c.bottom_served_floor, c.top_served_floor))
            .collect();
        assert_eq!(spans, [(0, 5, 1, 10, 12), (1, 5, 1, 14, 14), (2, 9, 0, 10, 11)]);
        assert_eq!(world.carriers[2].assignment_capacity, 0x2a);

        // Extend the lower run of column 5 by a floor: the old carrier keeps its
        // car state, and an out-of-range car is reset home.
        world.carriers[0].cars[0].current_floor = 12;
        world.carriers[0].cars[0].settle_counter = 3;
        world.carriers[1].cars[0].dwell_counter = 4;
        let a13 = k(5, 13);
        world.overlay_to_anchor.insert(a13.as_str(), a10.clone());
        rebuild_carrier_list(&mut world);
        let spans: Vec<_> = world
            .carriers
            .iter()
            .map(|c| (c.carrier_id, c.bottom_served_floor, c.top_served_floor))
            .collect();
        // Floors 10..14 are now one run; it overlaps the 10..12 carrier most.
        assert_eq!(spans, [(0, 10, 14), (1, 10, 11)]);
        assert_eq!(world.carriers[0].cars[0].settle_counter, 3);
        assert_eq!(world.carriers[0].floor_queues.len(), 5);
        assert_eq!(world.carriers[0].cars[0].waiting_count.len(), 5);
    }
}

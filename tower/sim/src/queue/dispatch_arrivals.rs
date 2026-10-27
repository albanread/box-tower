// ts: queue/dispatch-arrivals.ts

// 1218:07a6 dispatch_carrier_car_arrivals
// 1218:0883 dispatch_destination_queue_entries
//
// Per-tick arrival pass: at `dwellCounter == 5` (first dwell tick after
// stop), walks the car's active-slot ring and drops any slot whose
// destination matches the current floor. For each dropped slot, the binary
// calls the matching family's `dispatch_object_family_*_state_handler`
// inline — see `dispatchSimArrival` in ../sims/index.ts for the TS-side
// family dispatch. Phase 7 removed the `onArrival` callback trampoline;
// arrival now invokes the family handler directly from this file.

use crate::carriers::slot::floor_to_slot;
use crate::js;
use crate::carriers::sync::{active_slot_limit_for, sync_assignment_status, sync_pending_route_ids};
use crate::ledger::LedgerState;
use crate::sims::dispatch_sim_arrival;
use crate::sims::population::sim_key;
use crate::time::TimeState;
use crate::world::WorldState;

const DEPARTURE_SEQUENCE_TICKS: i64 = 5;

/// Binary `dispatch_destination_queue_entries` (1218:0883). Iterates the
/// car's active-slot ring up to `assignment_capacity`; for each boarded
/// slot whose destinationFloor matches the car's currentFloor, the binary
/// runs the per-slot sequence:
///
///   1. `pop_active_route_slot_request` (1218:1905) — clears the slot in
///      place and yields the routeId.
///   2. Family dispatch: switch on the sim's family code and invoke
///      `dispatch_object_family_*_state_handler` (or the
///      housekeeping/entertainment/recycling/parking variants). The handler
///      may itself call `cancel_runtime_route_request` (1218:1a86), whose
///      inner `remove_request_from_active_route_slots` (1218:173a) ends
///      with `recompute_car_target_and_direction` — the mechanism by which
///      an express car at the top served floor flips its directionFlag
///      1→0 the moment its first/last disembarking rider is processed.
///   3. Decrement `assignedCount` (-0x5b) and the per-floor
///      `destinationCountByFloor[+0xc]` slot. The
///      `nonemptyDestinationCount` (-0x52) decrement runs once at
///      loop-exit when the per-floor counter has reached 0.
///
/// Order matters: the family handler must run BEFORE the counter
/// decrements so that the in-handler recompute observes the same
/// `assignedCount` / `destinationCountByFloor` state the binary did.
///
/// (`carrier` is `world.carriers[carrier_index]`, `car` is that carrier's
/// `cars[car_index]`.  Upstream holds both objects across the family
/// handler; nothing the handlers do replaces `world.carriers` or a
/// carrier's `cars`, so reaching them by index again afterwards is the
/// same.)
pub fn dispatch_destination_queue_entries(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    carrier_index: i64,
    car_index: i64,
) -> bool {
    let ci = carrier_index as usize;
    let ki = car_index as usize;
    let mut changed = false;
    let assigned_before_arrivals = world.carriers[ci].cars[ki].assigned_count;
    let limit = active_slot_limit_for(&world.carriers[ci]);
    let destination_slot = floor_to_slot(&world.carriers[ci], world.carriers[ci].cars[ki].current_floor);

    for index in 0..limit.max(0) as usize {
        let car = &mut world.carriers[ci].cars[ki];
        let current_floor = car.current_floor;
        let Some(slot) = car.active_route_slots.get_mut(index) else {
            continue;
        };
        if !slot.active || !slot.boarded {
            continue;
        }
        if slot.destination_floor != current_floor {
            continue;
        }

        // Step 1: pop_active_route_slot_request — clear the slot in place,
        // matching binary order so the family handler sees the slot as
        // already consumed if it walks the ring (e.g., via
        // remove_request_from_active_route_slots inside cancel).
        let arrived_route_id = std::mem::take(&mut slot.route_id);
        let arrived_floor = slot.destination_floor;
        slot.active = false;
        slot.route_id = String::new();
        slot.source_floor = 0xff;
        slot.destination_floor = 0xff;
        slot.boarded = false;

        // Step 2: family dispatch. Look up the sim and invoke the
        // per-family state handler. This may recursively touch the same
        // car's slots/direction via cancel_runtime_route_request.
        let sim = world.sims.iter().position(|s| sim_key(s) == arrived_route_id);
        if let Some(sim_index) = sim {
            // BOX: emergency -- a rider due to collapse does so here instead
            // (false at once while nobody is due).
            if !crate::box_ext::emergency::collapse_on_arrival(world, time, sim_index as i64, arrived_floor) {
                dispatch_sim_arrival(world, ledger, time, sim_index as i64, arrived_floor);
            }
        } else {
            // BOX: emergency -- a medic or patient (route "box:er:..."); any
            // other id returns at once.
            crate::box_ext::emergency::on_carrier_arrival(world, &arrived_route_id, arrived_floor);
        }

        // Step 3: decrement counters. Done AFTER the handler so the in-
        // handler recompute sees the binary's unmodified count snapshot.
        let carrier = &mut world.carriers[ci];
        let car = &mut carrier.cars[ki];
        car.assigned_count = 0.max(car.assigned_count - 1);
        if destination_slot >= 0 {
            let ds = destination_slot as usize;
            let prev = js::at_or(&car.destination_count_by_floor, destination_slot, 0);
            // (Upstream writes `destinationCountByFloor[destinationSlot]`;
            // the slot is always within the array, which has one entry per
            // served floor.)
            car.destination_count_by_floor[ds] = 0.max(prev - 1);
            if prev == 1 {
                car.nonempty_destination_count = 0.max(car.nonempty_destination_count - 1);
            }
        }

        carrier.pending_routes.retain(|candidate| candidate.sim_id != arrived_route_id);
        if !carrier.completed_route_ids.contains(&arrived_route_id) {
            carrier.completed_route_ids.push(arrived_route_id);
        }
        changed = true;
    }

    if changed {
        let carrier = &mut world.carriers[ci];
        let car = &mut carrier.cars[ki];
        car.arrival_dispatch_this_tick = true;
        car.arrival_dispatch_starting_assigned_count = assigned_before_arrivals;
        sync_pending_route_ids(car);
        sync_assignment_status(carrier);
    }
    changed
}

/// Binary `dispatch_carrier_car_arrivals` (1218:07a6). Gate on
/// `dwellCounter == 5` (first dwell tick after arrival); delegates to
/// `dispatch_destination_queue_entries` for the actual slot-unload work.
///
/// (`carrier` is `world.carriers[carrier_index]`, `car` is that carrier's
/// `cars[car_index]`.)
pub fn dispatch_carrier_car_arrivals(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    carrier_index: i64,
    car_index: i64,
) {
    let car = &world.carriers[carrier_index as usize].cars[car_index as usize];
    if !car.active {
        return;
    }
    if car.dwell_counter != DEPARTURE_SEQUENCE_TICKS {
        return;
    }
    dispatch_destination_queue_entries(world, ledger, time, carrier_index, car_index);
}


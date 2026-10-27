// ts: families/maybe-dispatch-after-wait.ts

// 1228:15a0 maybe_dispatch_queued_route_after_wait
//
// Fires when a queued sim has waited past the route-failure delay. The binary
// reaches this path via `refresh_object_family_office_state_handler` (1228:1cb5)
// AND `refresh_object_family_hotel_state_handler` (1228:2aec) when
// `state_code >= 0x40` AND `encoded_route_target >= 0x40` — i.e. the sim is
// still sitting on a carrier queue, not a segment. When elapsed ticks since
// enqueue exceed 300 (`g_route_failure_delay`), the dispatch is force-driven
// into the family's per-state timeout handler:
//   office: 1228:193d → sim[+5] = 0x26 (NIGHT_B), evict route
//   hotel:  1228:19f4 → sim[+5] = 0x26 (NIGHT_B), evict route, fail service eval
// Both share the same NIGHT_B + eviction effect; hotel additionally calls
// fail_office_service_evaluation (1248:017d) — not yet modeled.

use crate::carriers::slot::floor_to_slot;
use crate::carriers::sync::sync_assignment_status;
use crate::ledger::LedgerState;
use crate::queue::dequeue::pop_unit_queue_request;
use crate::resources::{
    FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN, FAMILY_OFFICE, FAMILY_RESTAURANT,
    FAMILY_RETAIL,
};
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::office::advance_office_presence_counter;
use crate::sims::population::{clear_sim_route, find_object_for_sim, sim_key};
use crate::sims::release_service_request;
use crate::sims::states::{
    STATE_ACTIVE_TRANSIT, STATE_AT_WORK_TRANSIT, STATE_COMMUTE_TRANSIT, STATE_DEPARTURE, STATE_DEPARTURE_TRANSIT,
    STATE_DWELL_RETURN_TRANSIT, STATE_MORNING_TRANSIT, STATE_NIGHT_B, STATE_PARKED, STATE_VENUE_HOME_TRANSIT,
    STATE_VENUE_TRIP_TRANSIT,
};
use crate::stress::rebase_elapsed::rebase_sim_elapsed_from_clock;
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::time::{TimeState, DAY_TICK_MAX};
use crate::world::{RouteState, WorldState};

// Binary g_route_delay_table_base @ 1288:e5ee. Loaded by
// load_startup_tuning_resource_table (1198:0005) from resource type 0xff05
// id 1000 word 0 = 300 ticks.
const ROUTE_WAIT_TIMEOUT_TICKS: i64 = 300;

// Binary family-7 dispatch_sim_behavior jumptable at 1228:1c51. Entries for
// states {0x45, 0x60, 0x61, 0x62, 0x63} all point to handler 1228:193d, which
// unconditionally writes sim[+5] = 0x26 (NIGHT_B). Entries for {0x40, 0x41,
// 0x42} point to handler 1228:1989, which writes sim[+5] = 0x05 (DEPARTURE)
// after calling advance_stay_phase_or_wrap (1228:68c3). The 0x40/0x41/0x42
// timeout path is reached for FULL-stuck sims (sim+8 = 0xfe queue-full
// marker) once the 300-tick wait threshold passes.
const OFFICE_WAIT_TIMEOUT_TO_NIGHT_B_STATES: &[i64] = &[
    STATE_DEPARTURE_TRANSIT,
    STATE_MORNING_TRANSIT,
    STATE_AT_WORK_TRANSIT,
    STATE_VENUE_HOME_TRANSIT,
    STATE_DWELL_RETURN_TRANSIT,
];

// Binary family-3/4/5 dispatch_sim_behavior jumptable at cs:1c41. Only state
// 0x45 → 1228:19f4 writes NIGHT_B; entries for {0x41, 0x60, 0x62} point to
// 1228:1a4f (different handler — not yet decoded).
const HOTEL_WAIT_TIMEOUT_TO_NIGHT_B_STATES: &[i64] = &[STATE_DEPARTURE_TRANSIT];

const HOTEL_FAMILY_CODES: &[i64] = &[FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_HOTEL_SUITE];

// Binary `gate_object_family_restaurant_fast_food_state_handler` (1228:466d)
// calls `maybe_dispatch_queued_route_after_wait` (1228:15a0) for sims with
// state_code >= 0x40 AND sim+8 (encoded_route_target) >= 0x40 — same pattern
// as office/hotel. The retail/restaurant/fast-food state-0x45 jump table at
// 1228:4d37 maps rc=-1 AND rc=3 to 1228:4c9e, which writes sim+5 = 0x27
// (PARKED). So on timeout, the commercial state-0x45 path becomes PARKED,
// not NIGHT_B.
const COMMERCIAL_FAMILY_CODES: &[i64] = &[FAMILY_RESTAURANT, FAMILY_RETAIL, FAMILY_FAST_FOOD];

const COMMERCIAL_WAIT_TIMEOUT_TO_PARKED_STATES: &[i64] = &[STATE_DEPARTURE_TRANSIT];

/// (`sim` is `world.sims[sim_index]`, in this function and the two below.)
fn dispatch_timed_out_office_queue_entry(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    rebase_sim_elapsed_from_clock(&mut world.sims[si], time);
    advance_sim_trip_counters(&mut world.sims[si]);

    let state_code = world.sims[si].state_code;
    if state_code == STATE_COMMUTE_TRANSIT || state_code == STATE_ACTIVE_TRANSIT || state_code == STATE_VENUE_TRIP_TRANSIT
    {
        if let Some(object_key) = find_object_for_sim(world, sim_index) {
            if let Some(object) = world.object_mut(&object_key) {
                advance_office_presence_counter(object);
            }
        }
        let sim = &mut world.sims[si];
        sim.selected_floor = sim.floor_anchor;
        sim.destination_floor = -1;
        sim.state_code = STATE_DEPARTURE;
        clear_sim_route(sim);
        return;
    }

    let sim = &mut world.sims[si];
    if OFFICE_WAIT_TIMEOUT_TO_NIGHT_B_STATES.contains(&sim.state_code) {
        sim.state_code = STATE_NIGHT_B;
    }
    sim.destination_floor = -1;
    clear_sim_route(sim);
}

fn dispatch_timed_out_hotel_queue_entry(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let sim = &mut world.sims[sim_index as usize];
    // Mirror dispatch_sim_behavior (1228:186c) prologue for non-housekeeping
    // families before the per-state handler fires.
    rebase_sim_elapsed_from_clock(sim, time);
    advance_sim_trip_counters(sim);
    if HOTEL_WAIT_TIMEOUT_TO_NIGHT_B_STATES.contains(&sim.state_code) {
        sim.state_code = STATE_NIGHT_B;
    }
    sim.destination_floor = -1;
    clear_sim_route(sim);
}

fn dispatch_timed_out_commercial_queue_entry(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    // Binary 1228:186c dispatch_sim_behavior prologue.
    rebase_sim_elapsed_from_clock(&mut world.sims[si], time);
    advance_sim_trip_counters(&mut world.sims[si]);
    // Binary 1228:4c9e: state-0x45 rc=-1/rc=3 writer for restaurant/retail/
    // fast-food — transitions to PARKED (0x27) and releases the venue slot.
    if COMMERCIAL_WAIT_TIMEOUT_TO_PARKED_STATES.contains(&world.sims[si].state_code) {
        world.sims[si].state_code = STATE_PARKED;
        release_service_request(world, sim_index);
    }
    let sim = &mut world.sims[si];
    sim.destination_floor = -1;
    clear_sim_route(sim);
}

/// (`sim` is `world.sims[sim_index]`.  Upstream holds the carrier, its
/// pending route and the floor queue as objects across the loop; the
/// timed-out handlers touch only sims and placed objects, so they are
/// reached by index here.)
fn dispatch_queued_route_until_request(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let RouteState::Carrier { carrier_id, .. } = world.sims[si].route else {
        let family_code = world.sims[si].family_code;
        if family_code == FAMILY_OFFICE {
            dispatch_timed_out_office_queue_entry(world, time, sim_index);
        } else if HOTEL_FAMILY_CODES.contains(&family_code) {
            dispatch_timed_out_hotel_queue_entry(world, time, sim_index);
        } else if COMMERCIAL_FAMILY_CODES.contains(&family_code) {
            dispatch_timed_out_commercial_queue_entry(world, time, sim_index);
        }
        return;
    };
    let Some(ci) = world.carriers.iter().position(|c| c.carrier_id == carrier_id) else {
        return;
    };
    let route_id = sim_key(&world.sims[si]);
    let carrier = &world.carriers[ci];
    let Some(route) = carrier.pending_routes.iter().find(|candidate| candidate.sim_id == route_id) else {
        return;
    };
    if route.boarded {
        return;
    }
    // (The route's fields, which nothing in the loop writes.)
    let direction_flag = route.direction_flag;
    let slot = floor_to_slot(carrier, route.source_floor);
    if slot < 0 {
        return;
    }
    if slot as usize >= carrier.floor_queues.len() {
        return;
    }
    let queue = slot as usize;

    loop {
        let popped = pop_unit_queue_request(&mut world.carriers[ci].floor_queues[queue], direction_flag);
        // (`if (!poppedRouteId) break;`: an empty id is falsy too.)
        let popped_route_id = match popped {
            Some(id) if !id.is_empty() => id,
            _ => break,
        };
        world.carriers[ci].pending_routes.retain(|candidate| candidate.sim_id != popped_route_id);
        let popped_sim = world.sims.iter().position(|candidate| sim_key(candidate) == popped_route_id);
        if let Some(p) = popped_sim {
            let family_code = world.sims[p].family_code;
            if family_code == FAMILY_OFFICE {
                dispatch_timed_out_office_queue_entry(world, time, p as i64);
            } else if HOTEL_FAMILY_CODES.contains(&family_code) {
                dispatch_timed_out_hotel_queue_entry(world, time, p as i64);
            } else if COMMERCIAL_FAMILY_CODES.contains(&family_code) {
                dispatch_timed_out_commercial_queue_entry(world, time, p as i64);
            }
        }
        if popped_route_id == route_id {
            break;
        }
    }
    sync_assignment_status(&mut world.carriers[ci]);
}

/// (`sim` is `world.sims[sim_index]`.)
pub fn maybe_dispatch_queued_route_after_wait(
    world: &mut WorldState,
    _ledger: &LedgerState,
    time: &TimeState,
    sim_index: i64,
) {
    let si = sim_index as usize;
    let sim = &world.sims[si];
    // Binary gate: only fires for sims marked in-transit on a carrier queue.
    // The 0x40 bit check is the state_code half; the sim.route.mode === "carrier"
    // check is the encoded_route_target >= 0x40 half.
    if !is_sim_in_transit(sim.state_code) {
        return;
    }
    let RouteState::Carrier { carrier_id, .. } = sim.route else {
        return;
    };
    if sim.family_code == FAMILY_OFFICE {
        // Binary 1228:15a0 itself has no office-state filter; the caller reaches
        // it for queued office transit aliases 0x40/0x41/0x42/0x45/0x60..0x63.
        // The per-state dispatch result is decided after the queue pop.
    } else if HOTEL_FAMILY_CODES.contains(&sim.family_code) {
        if !HOTEL_WAIT_TIMEOUT_TO_NIGHT_B_STATES.contains(&sim.state_code) {
            return;
        }
    } else if COMMERCIAL_FAMILY_CODES.contains(&sim.family_code) {
        // Binary gate_object_family_restaurant_fast_food_state_handler
        // (1228:466d) reaches maybe_dispatch_queued_route_after_wait for
        // state_code >= 0x40 (the in-transit aliases). state 0x45 is the
        // fast-food / retail / restaurant DEPARTURE_TRANSIT.
        if !COMMERCIAL_WAIT_TIMEOUT_TO_PARKED_STATES.contains(&sim.state_code) {
            return;
        }
    } else {
        return;
    }
    if sim.last_demand_tick < 0 {
        return;
    }
    // Day-tick wraps 0..DAY_TICK_MAX-1; the binary uses 16-bit unsigned
    // subtraction so a stamp from before rollover compares correctly.
    let elapsed = (time.day_tick - sim.last_demand_tick + DAY_TICK_MAX) % DAY_TICK_MAX;
    if elapsed <= ROUTE_WAIT_TIMEOUT_TICKS {
        return;
    }
    let Some(carrier) = world.carriers.iter().find(|c| c.carrier_id == carrier_id) else {
        return;
    };
    // Binary: only fires for sims still waiting in the floor queue — once the
    // carrier picks them up, the arrival handler transitions state naturally.
    let key = sim_key(sim);
    let Some(route) = carrier.pending_routes.iter().find(|r| r.sim_id == key) else {
        return;
    };
    if route.boarded {
        return;
    }
    // Binary 1228:15a0 unconditionally calls dispatch_queued_route_until_request
    // (1218:1981) for both office and hotel families: pop the floor queue in
    // FIFO order and dispatch each popped sim through its family handler until
    // the requesting sim itself is popped. Without this drain the floor queue
    // keeps stale entries even though `pendingRoutes`/`sim.route` were cleared,
    // which causes the next dwell-1 drain to over-board the late-comers (e.g.
    // dense_hotel d1 t398 carrier@84 over-boarded 7 sims that the binary had
    // already cancelled via this timeout path).
    dispatch_queued_route_until_request(world, time, sim_index);
}

// ts: families/parking.ts

// 1228:5b5a gate_object_family_parking_state_handler (family 36)
// 1228:5cd2 dispatch_object_family_parking_state_handler (family 36)
//
// Parking state machine. Current TS (`sims/parking.ts`) contains demand-log
// rebuild + service assignment only; the family-36 state-machine gate and
// dispatch functions are not yet implemented in TS.

use crate::cathedral::check_eval_completion_and_award;
use crate::queue::cancel::decrement_route_queue_direction_load;
use crate::queue::resolve::{resolve_sim_route_between_floors, ResolveSimRouteOptions, RouteResolution};
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::states::{
    EVAL_ZONE_FLOOR, LOBBY_FLOOR, STATE_ARRIVED, STATE_DEPARTURE, STATE_DEPARTURE_TRANSIT, STATE_EVAL_OUTBOUND,
    STATE_MORNING_GATE, STATE_PARKED,
};
use crate::time::TimeState;
use crate::world::{sample_rng, RouteState, WorldState};

pub use crate::sims::parking::{rebuild_parking_demand_log, try_assign_parking_service};

// Binary quirk: the dispatch jump table at cs:5f29 lists exactly these four
// state codes mapped to handlers. Any other state_code falls through silently.
// States 0x05/0x45 → handle_family_parking_return_route (1228:5e7e).
// States 0x20/0x60 → handle_family_parking_outbound_route (1228:5ddd).

/// 1228:5ddd handle_family_parking_outbound_route.
///
/// Binary call site at 1228:5e04 passes `is_passenger_route = 1` and
/// `emit_distance_feedback = (sim.stateCode == 0x20) ? 1 : 0` — i.e. distance
/// feedback fires only on the BASE state 0x20 dispatch, not on the +0x40
/// alias 0x60 retry strides.
///
/// (`sim` is `world.sims[sim_index]`, here and below.)
fn handle_family_parking_outbound_route(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    // Binary: if state_code == 0x20 (fresh dispatch), use ground floor as
    // source with directionFlag=1 (upward). Otherwise use sim[+7] (originFloor)
    // with directionFlag=0.
    let is_fresh_dispatch = world.sims[si].state_code == STATE_MORNING_GATE;
    let source_floor = if is_fresh_dispatch { LOBBY_FLOOR } else { world.sims[si].origin_floor };
    let direction_flag = if is_fresh_dispatch { 1 } else { 0 };

    let result: RouteResolution = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        EVAL_ZONE_FLOOR,
        direction_flag,
        Some(time),
        Some(ResolveSimRouteOptions {
            emit_distance_feedback: Some(is_fresh_dispatch),
            ..Default::default()
        }),
    );

    if result == 3 {
        world.sims[si].state_code = STATE_ARRIVED;
        check_eval_completion_and_award(world, time, sim_index);
    } else if result == 0 || result == 1 || result == 2 {
        world.sims[si].state_code = STATE_EVAL_OUTBOUND; // 0x60: in-transit to eval zone
    } else {
        world.sims[si].state_code = STATE_PARKED; // 0x27: route failure
    }
}

/// 1228:5e7e handle_family_parking_return_route.
///
/// Binary call site at 1228:5ea5 passes `is_passenger_route = 1` and
/// `emit_distance_feedback = (sim.stateCode == 0x05) ? 1 : 0` — i.e. distance
/// feedback fires only on the BASE state 0x05 dispatch, not on the +0x40
/// alias 0x45 retry strides.
fn handle_family_parking_return_route(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    // Binary: if state_code == 0x05 (fresh return dispatch), use eval zone as
    // source with directionFlag=1 (upward/toward lobby). Otherwise use
    // sim[+7] (originFloor) with directionFlag=0.
    let is_fresh_dispatch = world.sims[si].state_code == STATE_DEPARTURE;
    let source_floor = if is_fresh_dispatch { EVAL_ZONE_FLOOR } else { world.sims[si].origin_floor };
    let direction_flag = if is_fresh_dispatch { 1 } else { 0 };

    let result: RouteResolution = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        LOBBY_FLOOR,
        direction_flag,
        Some(time),
        Some(ResolveSimRouteOptions {
            emit_distance_feedback: Some(is_fresh_dispatch),
            ..Default::default()
        }),
    );

    if result == 0 || result == 1 || result == 2 {
        world.sims[si].state_code = STATE_DEPARTURE_TRANSIT; // 0x45: in-transit back to lobby
    } else {
        // Binary quirk: both result === 3 (same-floor) and result === -1 (failure)
        // park the sim to 0x27. Same-floor arrival is treated as a no-op here.
        world.sims[si].state_code = STATE_PARKED; // 0x27
    }
}

fn run_dispatch_jump_table(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    // Prologue: for in-transit states, if the sim is on a special-link segment
    // (encoded_route_target in [0, 0x3f]), call decrement_route_queue_direction_load.
    // Carrier-queued sims (route.mode === "carrier") skip this.
    if is_sim_in_transit(world.sims[si].state_code) && matches!(world.sims[si].route, RouteState::Segment { .. }) {
        // (Upstream passes `world.carriers[0] ?? {}`; the call does nothing
        // either way, so with no carrier there is nothing to call.)
        if let Some(carrier) = world.carriers.first() {
            decrement_route_queue_direction_load(carrier, world.sims[si].origin_floor, 0);
        }
    }

    // 4-entry jump table at cs:5f29:
    //   0x05 → handle_family_parking_return_route
    //   0x20 → handle_family_parking_outbound_route
    //   0x45 → handle_family_parking_return_route
    //   0x60 → handle_family_parking_outbound_route
    match world.sims[si].state_code {
        STATE_DEPARTURE /* 0x05 */ | STATE_DEPARTURE_TRANSIT /* 0x45 */ => {
            handle_family_parking_return_route(world, time, sim_index);
        }
        STATE_MORNING_GATE /* 0x20 */ | STATE_EVAL_OUTBOUND /* 0x60 */ => {
            handle_family_parking_outbound_route(world, time, sim_index);
        }
        _ => {}
    }
}

/// 1228:5b5a gate_object_family_parking_state_handler.
pub fn gate_object_family_parking_state_handler(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let state_code = world.sims[si].state_code;

    if is_sim_in_transit(state_code) {
        // stateCode >= 0x40: already in-transit.
        // Binary: if encoded_route_target >= 0x40 (carrier queue), call
        // maybe_dispatch_queued_route_after_wait. For parking, that function
        // exits immediately (family != FAMILY_OFFICE guard), so we skip the
        // call and return. Otherwise fall through to dispatch (segment in progress).
        if matches!(world.sims[si].route, RouteState::Carrier { .. }) {
            return;
        }
        run_dispatch_jump_table(world, time, sim_index);
        return;
    }

    if state_code == STATE_DEPARTURE {
        // 0x05: unconditional dispatch (return route)
        run_dispatch_jump_table(world, time, sim_index);
        return;
    }

    if state_code == STATE_MORNING_GATE {
        // 0x20: morning activation gate — weekendFlag must be 1
        if time.weekend_flag != 1 {
            return;
        }

        if time.daypart_index == 0 {
            // Staggered dispatch: 1/12 chance after tick 0x50, guaranteed after 0xf0.
            if time.day_tick > 0x50 && sample_rng(world) % 12 == 0 {
                run_dispatch_jump_table(world, time, sim_index);
            }
            if time.day_tick > 0xf0 {
                run_dispatch_jump_table(world, time, sim_index);
            }
            return;
        }

        // daypartIndex >= 1: missed dispatch window — park the sim
        if time.daypart_index >= 1 {
            world.sims[si].state_code = STATE_PARKED;
        }
        return;
    }

    // All other states (STATE_ARRIVED 0x03, STATE_PARKED 0x27, etc.): no-op.
}

/// 1228:5cd2 dispatch_object_family_parking_state_handler.
///  Called by dispatch_destination_queue_entries (1218:0883) on carrier arrival.
pub fn dispatch_object_family_parking_state_handler(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    run_dispatch_jump_table(world, time, sim_index);
}

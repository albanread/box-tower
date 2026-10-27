// ts: cathedral.ts

use crate::queue::resolve::{resolve_sim_route_between_floors, ResolveSimRouteOptions};
use crate::sims::population::{clear_sim_route, find_object_for_sim};
use crate::sims::states::{
    CATHEDRAL_FAMILIES, EVAL_ZONE_FLOOR, LOBBY_FLOOR, NO_EVAL_ENTITY, STATE_ARRIVED, STATE_DEPARTURE,
    STATE_EVAL_OUTBOUND, STATE_EVAL_RETURN, STATE_MORNING_GATE, STATE_PARKED,
};
use crate::time::TimeState;
use crate::world::{sample_rng, RouteState, WorldState};

// 5 floor types × 8 slots
const EVAL_SIM_COUNT: i64 = 40;

fn is_eval_placed(world: &WorldState) -> bool {
    world.gate_flags.eval_sim_index >= 0 && world.gate_flags.eval_sim_index != NO_EVAL_ENTITY
}

/// Activate cathedral guest sims at the day-start checkpoint.
/// Forces all cathedral sim slots into the morning-gate state if a cathedral is
/// placed. The binary does not apply a star-count gate here.
pub fn activate_eval_sims(world: &mut WorldState) {
    if !is_eval_placed(world) {
        return;
    }

    for sim in world.sims.iter_mut() {
        if !CATHEDRAL_FAMILIES.contains(&sim.family_code) {
            continue;
        }
        sim.state_code = STATE_MORNING_GATE;
        sim.selected_floor = LOBBY_FLOOR;
        sim.origin_floor = sim.floor_anchor;
        clear_sim_route(sim);
        sim.destination_floor = -1;
        sim.venue_return_state = 0;
    }
}

/// Dispatch midday return for cathedral guest sims at the hotel-sale checkpoint.
/// Sims in the arrived state are advanced to the return state.
pub fn dispatch_eval_midday_return(world: &mut WorldState) {
    if !is_eval_placed(world) {
        return;
    }
    for object in world.placed_objects.values_mut() {
        if !CATHEDRAL_FAMILIES.contains(&object.object_type_code) {
            continue;
        }
        object.aux_value_or_timer = 0;
        object.dirty_flag = 1;
    }
    for sim in world.sims.iter_mut() {
        if !CATHEDRAL_FAMILIES.contains(&sim.family_code) {
            continue;
        }
        if sim.state_code == STATE_ARRIVED {
            sim.state_code = STATE_DEPARTURE;
            sim.selected_floor = EVAL_ZONE_FLOOR;
            sim.destination_floor = LOBBY_FLOOR;
        }
    }
}

fn dispatch_outbound(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let sim = &mut world.sims[sim_index as usize];
    let is_fresh_dispatch = sim.state_code == STATE_MORNING_GATE;
    let source_floor = if is_fresh_dispatch { LOBBY_FLOOR } else { sim.origin_floor };
    let direction_flag = if is_fresh_dispatch { 1 } else { 0 };
    sim.selected_floor = source_floor;
    sim.destination_floor = EVAL_ZONE_FLOOR;
    let result = resolve_sim_route_between_floors(
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
        world.sims[sim_index as usize].state_code = STATE_ARRIVED;
        check_eval_completion_and_award(world, time, sim_index);
    } else if result >= 0 {
        world.sims[sim_index as usize].state_code = STATE_EVAL_OUTBOUND;
    } else {
        world.sims[sim_index as usize].state_code = STATE_PARKED;
    }
}

fn dispatch_return(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let sim = &mut world.sims[sim_index as usize];
    let is_fresh_dispatch = sim.state_code == STATE_DEPARTURE;
    let source_floor = if is_fresh_dispatch { EVAL_ZONE_FLOOR } else { sim.origin_floor };
    let direction_flag = if is_fresh_dispatch { 1 } else { 0 };
    sim.selected_floor = source_floor;
    sim.destination_floor = LOBBY_FLOOR;
    let result = resolve_sim_route_between_floors(
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
        world.sims[sim_index as usize].state_code = STATE_EVAL_RETURN;
    } else {
        world.sims[sim_index as usize].state_code = STATE_PARKED;
    }
}

pub fn process_cathedral_sim(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    match world.sims[sim_index as usize].state_code {
        STATE_MORNING_GATE => {
            // Gate: weekendFlag must be 1
            if time.weekend_flag != 1 {
                return;
            }
            if time.daypart_index == 0 {
                if time.day_tick > 0x50 && sample_rng(world) % 12 == 0 {
                    dispatch_outbound(world, time, sim_index);
                }
                if time.day_tick > 0xf0 {
                    dispatch_outbound(world, time, sim_index);
                }
                return;
            }
            if time.daypart_index >= 1 {
                world.sims[sim_index as usize].state_code = STATE_PARKED;
            }
        }

        STATE_EVAL_OUTBOUND => {
            if !matches!(world.sims[sim_index as usize].route, RouteState::Carrier { .. }) {
                dispatch_outbound(world, time, sim_index);
            }
        }

        STATE_ARRIVED => {
            // Arrived at eval zone; waiting for midday return dispatch
        }

        STATE_DEPARTURE => {
            dispatch_return(world, time, sim_index);
        }

        STATE_EVAL_RETURN => {
            if !matches!(world.sims[sim_index as usize].route, RouteState::Carrier { .. }) {
                dispatch_return(world, time, sim_index);
            }
        }

        STATE_PARKED => {
            // Parked; will be reset at next day-start
        }

        _ => {}
    }
}

pub fn check_eval_completion_and_award(world: &mut WorldState, time: &TimeState, arrived_sim_index: i64) {
    if !is_eval_placed(world) {
        return;
    }
    if time.day_tick >= 800 {
        return;
    }

    // Count sims that arrived at eval zone
    let mut arrived_count = 0;
    for sim in &world.sims {
        if !CATHEDRAL_FAMILIES.contains(&sim.family_code) {
            continue;
        }
        if sim.state_code == STATE_ARRIVED {
            arrived_count += 1;
        }
    }

    if arrived_count < EVAL_SIM_COUNT {
        // Not all arrived yet — stamp the arrived sim's placed object
        if let Some(key) = find_object_for_sim(world, arrived_sim_index) {
            if let Some(object) = world.object_mut(&key) {
                object.aux_value_or_timer = 3;
            }
        }
        return;
    }

    // All 40 arrived — check ledger tier > star_count for tower promotion.
    let tier_thresholds = [300, 1000, 5000, 10_000, 15_000];
    let ledger_total = world.current_population;
    let mut tier = 1;
    for (index, threshold) in tier_thresholds.iter().enumerate() {
        if ledger_total >= *threshold {
            tier = index as i64 + 2;
        }
    }

    if tier > world.star_count {
        // Tower promotion: star_count := 6
        world.star_count = 6;
        for object in world.placed_objects.values_mut() {
            if !CATHEDRAL_FAMILIES.contains(&object.object_type_code) {
                continue;
            }
            object.aux_value_or_timer = 2;
            object.dirty_flag = 1;
        }
    }
}

pub fn handle_cathedral_sim_arrival(world: &mut WorldState, time: &TimeState, sim_index: i64, arrival_floor: i64) {
    let sim = &mut world.sims[sim_index as usize];
    if sim.state_code == STATE_EVAL_OUTBOUND && arrival_floor == EVAL_ZONE_FLOOR {
        sim.state_code = STATE_ARRIVED;
        sim.destination_floor = -1;
        check_eval_completion_and_award(world, time, sim_index);
        return;
    }

    if sim.state_code == STATE_EVAL_RETURN && arrival_floor == LOBBY_FLOOR {
        sim.state_code = STATE_PARKED;
        sim.destination_floor = -1;
    }
}

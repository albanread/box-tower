// ts: families/finalize.ts

// 1228:1481 finalize_runtime_route_state
//
// Fired on route completion / cancellation: clears the state-code mode
// bits (0x40 in-transit, 0x20 waiting), resets the route bookkeeping
// struct, rebases elapsed time against the current clock, and advances
// trip counters. The binary wires this from the queue cancel path
// (cancel_runtime_route_request) and the arrival dispatch path.
//
// Phase 5b note: the TS state constants encode the 0x20 waiting bit as
// part of the phase byte for states in the 0x20..0x27 range (see
// families/force-dispatch.ts). finalizeRuntimeRouteState is therefore
// careful NOT to strip 0x20 — it only strips 0x40 (the true in-transit
// bit). Callers that wish to park a sim to a specific low-phase state
// should overwrite sim.stateCode explicitly.

use crate::sim_access::state_bits::{is_sim_in_transit, is_sim_waiting, set_sim_in_transit};
use crate::sims::population::clear_sim_route;
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::world::{RouteState, WorldState};

/// (`sim` is `world.sims[sim_index]`.)
pub fn finalize_runtime_route_state(world: &mut WorldState, sim_index: i64) {
    let sim = &mut world.sims[sim_index as usize];
    // Binary: advance trip counters iff the sim was actually on a route.
    // Bit test rather than struct because state_code is authoritative
    // (per ROUTING-BINARY-MAP.md §4.1).
    let was_active = is_sim_in_transit(sim.state_code)
        || is_sim_waiting(sim.state_code)
        || sim.route != RouteState::Idle;
    if was_active {
        advance_sim_trip_counters(sim);
    }
    clear_sim_route(sim);
    // Strip the 0x40 in-transit bit (see file-level comment for why we
    // do NOT strip 0x20 — that bit overlaps TS phase encodings).
    set_sim_in_transit(sim, false);
    sim.transit_ticks_remaining = 0;
}

// ts: families/recycling.ts

// 1228:4d5b gate_object_family_recycling_center_lower_state_handler (family 33)
// 1228:4ea0 dispatch_object_family_recycling_center_lower_state_handler (family 33)
//
// Recycling-center lower-slice (family 33 / 0x21) state machine. The sims
// for this family use the same state machine as entertainment guests but with
// a coarser gate: state 0x01 only dispatches during dayparts 0–3, after
// dayTick >= 0xf1, and only when RNG % 36 == 0 (1-in-36). The dispatch
// delegates to the entertainment guest dispatch table (0x5b3a via
// dispatch_entertainment_guest_state at 1228:53ad), which runs the
// service-acquisition / linked-half / phase-consumption / release-return
// cycle.
//
// Binary table at cs:0x5221 (4 entries):
//   state 0x01 / 0x41 → handler at 0x530d (daypart+RNG gate then entertainment dispatch)
//   state 0x22 / 0x62 → handler at 0x537f (unconditional entertainment dispatch)
//
// Binary quirk: the state-0x01 recycling dispatch handler (body at 0x530d)
// applies an ADDITIONAL daypart+RNG gate (daypart 0–3, dayTick > 0xf0, RNG%6==0)
// before delegating to entertainment dispatch, and unconditionally forces state
// 0x27 (parked) when daypart >= 4. This is separate from the gate check in
// gate_object_family_recycling_center_lower_state_handler.

use super::entertainment::dispatch_entertainment_guest_state;
use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::ledger::LedgerState;
use crate::resources::FAMILY_RECYCLING_CENTER_LOWER;
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::time::TimeState;
use crate::world::sample_rng;
use crate::world::{RouteState, WorldState};

// Binary state constants for family 33 (match the entertainment guest machine).
const STATE_RECYCLING_ACTIVE: i64 = 0x01; // outbound / venue-seeking
const STATE_RECYCLING_VENUE_DWELL: i64 = 0x22; // at venue (pre-release)
const STATE_RECYCLING_ACTIVE_TRANSIT: i64 = 0x41; // 0x01 + in-transit bit
const STATE_RECYCLING_DWELL_TRANSIT: i64 = 0x62; // 0x22 + in-transit bit
const STATE_RECYCLING_PARKED: i64 = 0x27; // parked / idle

// Timing constants from the binary (g_day_tick thresholds).
const RECYCLING_GATE_DAY_TICK_MIN: i64 = 0xf1; // dayTick must be >= this for state-0x01 to fire
const RECYCLING_PARK_DAY_TICK: i64 = 0x8fd; // dayTick >= this resets state-0x27 to 0x01

// RNG thresholds (from gate and dispatch bodies).
const RECYCLING_GATE_RNG_MODULO: i64 = 0x24; // 36: gate fires on RNG % 36 == 0
const RECYCLING_DISPATCH_RNG_MODULO: i64 = 6; // inner dispatch gate for state-0x01

/// 1228:4d5b gate_object_family_recycling_center_lower_state_handler.
///
/// Two-tier gate:
///   - stateCode < 0x40: switch on state; apply timing/RNG gate for state 0x01;
///     reset state 0x27 to 0x01 at day end; fall through for state 0x22.
///   - stateCode >= 0x40: if encodedRouteTarget >= 0x40, delegate to
///     maybeDispatchQueuedRouteAfterWait; else fall through to dispatch.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn gate_object_family_recycling_center_lower_state_handler(
    world: &mut WorldState,
    ledger: &LedgerState,
    time: &TimeState,
    sim_index: i64,
) {
    let si = sim_index as usize;
    if world.sims[si].family_code != FAMILY_RECYCLING_CENTER_LOWER {
        return;
    }

    if !is_sim_in_transit(world.sims[si].state_code) {
        let state = world.sims[si].state_code;

        if state == STATE_RECYCLING_ACTIVE {
            // Binary quirk: gate on daypart 0–3, dayTick >= 0xf1, and 1-in-36 RNG.
            if time.daypart_index < 0 || time.daypart_index > 3 {
                return;
            }
            if time.day_tick < RECYCLING_GATE_DAY_TICK_MIN {
                return;
            }
            if sample_rng(world) % RECYCLING_GATE_RNG_MODULO != 0 {
                return;
            }
        } else if state == STATE_RECYCLING_PARKED {
            // Binary quirk: state 0x27 resets itself to 0x01 once dayTick >= 0x8fd.
            if time.day_tick < RECYCLING_PARK_DAY_TICK {
                return;
            }
            world.sims[si].state_code = STATE_RECYCLING_ACTIVE;
            return;
        } else if state != STATE_RECYCLING_VENUE_DWELL {
            return;
        }
        // state 0x22 falls through to dispatch.
    } else {
        // In-transit: if still waiting on a carrier queue, delegate to the
        // route-failure timeout handler. Otherwise fall through to dispatch.
        if matches!(world.sims[si].route, RouteState::Carrier { .. }) {
            maybe_dispatch_queued_route_after_wait(world, ledger, time, sim_index);
            return;
        }
    }

    dispatch_object_family_recycling_center_lower_state_handler(world, time, sim_index);
}

/// 1228:4ea0 dispatch_object_family_recycling_center_lower_state_handler.
///
/// Jump table at cs:0x5221 (4 entries, states 0x01/0x22/0x41/0x62):
///   - 0x01 / 0x41 → handler at 0x530d: daypart 0–3, dayTick > 0xf0, RNG%6==0
///     → delegate to entertainment dispatch; if daypart >= 4, force state 0x27.
///   - 0x22 / 0x62 → handler at 0x537f: unconditionally delegate to entertainment
///     dispatch.
///
/// Binary prologue: when stateCode >= 0x40 AND encodedRouteTarget is a
/// carrier index (>= 0 and < 0x40), call decrement_route_queue_direction_load
/// before entering the table. TS: carried implicitly by `evictCarrierRoute` in
/// the cancel path; no explicit call needed here.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn dispatch_object_family_recycling_center_lower_state_handler(
    world: &mut WorldState,
    time: &TimeState,
    sim_index: i64,
) {
    let si = sim_index as usize;
    let state = world.sims[si].state_code;

    if state == STATE_RECYCLING_ACTIVE || state == STATE_RECYCLING_ACTIVE_TRANSIT {
        // Binary quirk (handler at 0x530d): the dispatch applies a second daypart +
        // dayTick > 0xf0 + RNG%6==0 gate before delegating to entertainment dispatch.
        // If daypart is already >= 4, skip the dispatch entirely and park the sim.
        if time.daypart_index >= 0
            && time.daypart_index <= 3
            && time.day_tick > 0xf0
            && sample_rng(world) % RECYCLING_DISPATCH_RNG_MODULO == 0
        {
            dispatch_entertainment_guest_state(world, time, sim_index);
        }
        if time.daypart_index > 3 {
            world.sims[si].state_code = STATE_RECYCLING_PARKED;
        }
        return;
    }

    if state == STATE_RECYCLING_VENUE_DWELL || state == STATE_RECYCLING_DWELL_TRANSIT {
        // Binary quirk (handler at 0x537f): unconditional delegation.
        dispatch_entertainment_guest_state(world, time, sim_index);
    }
}

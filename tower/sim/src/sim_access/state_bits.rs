// ts: sim-access/state-bits.ts

// Sim state_code bit helpers — binary layout at SimRecord+5:
//   bits 0..3 : phase (0..7)
//   bit 5 (0x20) : currently waiting
//   bit 6 (0x40) : route queued / in transit
//
// Phase 5a introduced the readers; Phase 5b adds setters and wires them into
// every `sim.route = ...` transition site so `sim.stateCode` bits are the
// authoritative routing-mode flags (matching the binary's
// `dispatch_object_family_*_state_handler` two-tier switch on family_code +
// state_code).

use crate::js;
use crate::world::SimRecord;

pub const SIM_STATE_WAITING_BIT: i64 = 0x20;
pub const SIM_STATE_IN_TRANSIT_BIT: i64 = 0x40;
pub const SIM_STATE_BASE_MASK: i64 = 0x1f;
pub const SIM_STATE_MODE_MASK: i64 = SIM_STATE_WAITING_BIT | SIM_STATE_IN_TRANSIT_BIT;

pub fn is_sim_waiting(state_code: i64) -> bool {
    js::to_int32(state_code) & SIM_STATE_WAITING_BIT != 0
}

pub fn is_sim_in_transit(state_code: i64) -> bool {
    js::to_int32(state_code) & SIM_STATE_IN_TRANSIT_BIT != 0
}

/// Strip the mode bits to expose just the base phase (0..7).
pub fn sim_base_state(state_code: i64) -> i64 {
    js::to_int32(state_code) & SIM_STATE_BASE_MASK
}

/// Set or clear the waiting bit (0x20) on `sim.stateCode` without disturbing the
/// in-transit bit or base-phase bits.
pub fn set_sim_waiting(sim: &mut SimRecord, on: bool) {
    if on {
        sim.state_code = (js::to_int32(sim.state_code) | SIM_STATE_WAITING_BIT) & 0xff;
    } else {
        sim.state_code = js::to_int32(sim.state_code) & !SIM_STATE_WAITING_BIT & 0xff;
    }
}

/// Set or clear the in-transit bit (0x40) on `sim.stateCode` without disturbing
/// the waiting bit or base-phase bits.
pub fn set_sim_in_transit(sim: &mut SimRecord, on: bool) {
    if on {
        sim.state_code = (js::to_int32(sim.state_code) | SIM_STATE_IN_TRANSIT_BIT) & 0xff;
    } else {
        sim.state_code = js::to_int32(sim.state_code) & !SIM_STATE_IN_TRANSIT_BIT & 0xff;
    }
}

/// Clear both mode bits (0x60). Used when returning a sim to idle.
pub fn clear_sim_route_bits(sim: &mut SimRecord) {
    sim.state_code = js::to_int32(sim.state_code) & !SIM_STATE_MODE_MASK & 0xff;
}

/// Overwrite the entire state_code byte (phase + mode bits).
pub fn set_sim_state(sim: &mut SimRecord, new_code: i64) {
    sim.state_code = js::to_int32(new_code) & 0xff;
}

/// Replace just the base-phase bits (0..3) on `sim.stateCode`, preserving
/// the waiting/in-transit mode bits. Phase 5b helper requested by the
/// migration plan; not yet used by the TS state machines which currently
/// overwrite the entire byte, but exposed for future binary-faithful
/// handlers that mutate phase without disturbing mode.
pub fn set_sim_base_state(sim: &mut SimRecord, phase: i64) {
    sim.state_code = ((js::to_int32(sim.state_code) & SIM_STATE_MODE_MASK)
        | (js::to_int32(phase) & SIM_STATE_BASE_MASK))
        & 0xff;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits() {
        assert!(is_sim_in_transit(0x45));
        assert!(!is_sim_in_transit(0x25));
        assert!(is_sim_waiting(0x25));
        assert_eq!(sim_base_state(0x65), 5);
        // `&` acts on ToInt32: a negative state keeps its low bits.
        assert!(is_sim_in_transit(-1));
        assert_eq!(sim_base_state(-1), 0x1f);
    }
}

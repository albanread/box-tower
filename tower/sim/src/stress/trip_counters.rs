// ts: stress/trip-counters.ts

// 11e0:0000 advance_sim_trip_counters
//
// Captures a completed trip into the sim's trip counters and resets the
// per-trip elapsed window. Binary: trip_count += 1, accumulated_elapsed +=
// (elapsed_packed & 0x3ff), clear low 10 bits of elapsed_packed, clear
// last_trip_tick. Housekeeping (family 0x0f) is excluded — the binary
// gates stress tracking on dispatch-family membership and HK never hits
// the path.

use crate::resources::FAMILY_HOUSEKEEPING;
use crate::world::SimRecord;

pub fn advance_sim_trip_counters(sim: &mut SimRecord) {
    if sim.family_code == FAMILY_HOUSEKEEPING {
        return;
    }
    sim.trip_count += 1;
    sim.accumulated_ticks += sim.elapsed_ticks;
    sim.elapsed_ticks = 0;
    sim.last_demand_tick = -1;
}

// ts: stress/rebase-elapsed.ts

// 11e0:00fc rebase_sim_elapsed_from_clock
//
// Syncs per-trip elapsed ticks with the day clock. Binary:
//   elapsed = (elapsed_packed & 0x3ff) + g_day_tick - last_trip_tick
//   clamp to 300 (g_route_failure_delay)
//   elapsed_packed = (elapsed_packed & 0xfc00) | elapsed
//   last_trip_tick = 0
// The `lastDemandTick >= 0` sentinel reflects the binary's "no timestamp
// set" convention: dayTick == 0 is valid, so -1 is the cleared marker.
// Housekeeping (family 0x0f) is excluded from stress tracking.

use crate::resources::FAMILY_HOUSEKEEPING;
use crate::time::TimeState;
use crate::world::SimRecord;

pub fn rebase_sim_elapsed_from_clock(sim: &mut SimRecord, time: &TimeState) {
    if sim.family_code == FAMILY_HOUSEKEEPING {
        return;
    }
    if sim.last_demand_tick >= 0 {
        sim.elapsed_ticks = 300.min(sim.elapsed_ticks + time.day_tick - sim.last_demand_tick);
    }
    sim.last_demand_tick = -1;
}

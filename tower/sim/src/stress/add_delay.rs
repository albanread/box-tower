// ts: stress/add-delay.ts

// 11e0:02f7 add_delay_to_current_sim
//
// Adds a fixed tick penalty to the current-trip elapsed window. Binary:
//   elapsed = (elapsed_packed & 0x3ff) + delta
//   clamp to 300 (g_route_failure_delay)
//   store back, clear last_trip_tick
// Used for: no-route delay (300), distance penalties, queue-full waiting
// delay (5), stair/escalator per-stop delay. Housekeeping excluded.

use crate::resources::FAMILY_HOUSEKEEPING;
use crate::world::SimRecord;

pub fn add_delay_to_current_sim(sim: &mut SimRecord, delta: i64) {
    if sim.family_code == FAMILY_HOUSEKEEPING {
        return;
    }
    sim.elapsed_ticks = 300.min(sim.elapsed_ticks + delta);
    sim.last_demand_tick = -1;
}

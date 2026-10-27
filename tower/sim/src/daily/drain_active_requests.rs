// ts: daily/drain-active-requests.ts

// 1190:0977 dispatch_active_requests_by_family
//
// Fires once per day at the 0x9c4 checkpoint. The binary iterates
// `g_active_request_table` and re-dispatches each stuck request through
// its family handler. TS analog: iterate sims whose `stateCode` has the
// 0x40 in-transit bit set and route each through `dispatchSimBehavior`
// (1228:186c), which forwards in-transit sims into
// `maybeDispatchQueuedRouteAfterWait` so the route-failure timeout can
// fire.
//
// Runs once per day; does NOT run every tick.

use crate::families::dispatch_sim_behavior::dispatch_sim_behavior;
use crate::ledger::LedgerState;
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::time::TimeState;
use crate::world::WorldState;

pub fn dispatch_active_requests_by_family(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    // (`for (const sim of world.sims)`: an array iterator reads the length
    // afresh each step, as this loop does.  If a handler ever replaced
    // `world.sims` with a new array, upstream would go on walking the old
    // one; none does.)
    let mut i = 0;
    while i < world.sims.len() {
        if is_sim_in_transit(world.sims[i].state_code) {
            dispatch_sim_behavior(world, ledger, time, i as i64);
        }
        i += 1;
    }
}

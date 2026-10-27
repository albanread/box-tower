// ts: families/housekeeping.ts

// 1228:5f39 gate_housekeeping_room_claim_state (family 15)
// 1228:602b update_object_family_housekeeping_connection_state (family 15)
// 1228:6480 activate_object_family_housekeeping_connection_state (family 14)
//
// Housekeeping helper (family 0x0f) state machine. Phase 5b makes this the
// real implementation that wraps `sims/housekeeping.ts#processHousekeepingSim`
// with the binary's two-tier structure:
//   - gate (1228:5f39): room-claim search (state HK_STATE_SEARCH)
//   - update (1228:602b): countdown + route-to-target (state HK_STATE_COUNTDOWN,
//     HK_STATE_ROUTE_TO_*)
//   - activate (1228:6480): arrival handler (arrival at claimed floor)
// The existing TS function drives all three sub-states; Phase 5b keeps the
// driver in `sims/housekeeping.ts` and exposes the binary names here.

use crate::sims::housekeeping::{
    handle_housekeeping_sim_arrival as _handle_housekeeping_sim_arrival,
    process_housekeeping_sim as _process_housekeeping_sim,
};
use crate::time::TimeState;
use crate::world::WorldState;

pub use crate::sims::housekeeping::{handle_housekeeping_sim_arrival, process_housekeeping_sim};

/// 1228:5f39 gate_housekeeping_room_claim_state. Entry point for the
/// search/claim gate (state HK_STATE_SEARCH).
/// Binary quirk: housekeeping uses low-valued states (0..4) that do NOT
/// overlap the 0x20/0x40 bits used by the dispatch_sim_behavior families,
/// so isSimInTransit/isSimWaiting are NOT consulted here.
///
/// (`sim` is `world.sims[sim_index]`, here and below.)
pub fn gate_housekeeping_room_claim_state(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    _process_housekeeping_sim(world, time, sim_index);
}

/// 1228:602b update_object_family_housekeeping_connection_state. Per-stride
/// update; currently shares the driver with the gate because the TS body
/// already handles all housekeeping states inline.
pub fn update_object_family_housekeeping_connection_state(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    _process_housekeeping_sim(world, time, sim_index);
}

/// 1228:6480 activate_object_family_housekeeping_connection_state.
pub fn activate_object_family_housekeeping_connection_state(
    world: &mut WorldState,
    time: &TimeState,
    sim_index: i64,
    arrival_floor: i64,
) {
    _handle_housekeeping_sim_arrival(world, time, sim_index, arrival_floor);
}

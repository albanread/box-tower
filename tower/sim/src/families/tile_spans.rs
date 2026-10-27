// ts: families/tile-spans.ts

// 1228:0fc2 rebuild_all_sim_tile_spans
// 1228:1018 update_sim_tile_span
//
// Binary iterates every (floor, object-type) slot in the global object table
// and finalizes any stuck in-transit sims. In TS, sims are a flat array; we
// map the binary's per-slot loop onto a flat world.sims sweep.
//
// update_sim_tile_span(floor, type, apply_updates=0):
//   - Most families: finalize if stateCode & 0x40 (in-transit).
//   - Family 7 (office): also release service request (unconditional).
//   - Family 15 (housekeeping): finalize if stateCode > 2.
//   apply_updates=1 is only used by refresh_entertainment_link_tile_spans
//   (an edit-time path) and is not modeled in the daily-sweep call.

use super::finalize::finalize_runtime_route_state;
use crate::resources::{FAMILY_HOUSEKEEPING, FAMILY_OFFICE};
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::release_service_request;
use crate::world::WorldState;

/// (`sim` is `world.sims[sim_index]`.)
pub fn update_sim_tile_span(world: &mut WorldState, sim_index: i64) {
    let si = sim_index as usize;
    if world.sims[si].family_code == FAMILY_HOUSEKEEPING {
        if world.sims[si].state_code > 2 {
            finalize_runtime_route_state(world, sim_index);
        }
    } else {
        if is_sim_in_transit(world.sims[si].state_code) {
            finalize_runtime_route_state(world, sim_index);
        }
        if world.sims[si].family_code == FAMILY_OFFICE {
            release_service_request(world, sim_index);
        }
    }
}

pub fn rebuild_all_sim_tile_spans(world: &mut WorldState) {
    // (`for (const sim of world.sims)`: neither handler adds or removes sims.)
    let mut i = 0;
    while i < world.sims.len() {
        update_sim_tile_span(world, i as i64);
        i += 1;
    }
}

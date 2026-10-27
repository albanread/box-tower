// ts: stress/lobby-reduction.ts

// 11e0:0423 reduce_elapsed_for_lobby_boarding
//
// Discounts accumulated elapsed when a sim boards a non-service carrier
// from the lobby floor, keyed on g_lobby_height:
//   source_floor != LOBBY: no-op
//   lobby_height <= 1: no-op
//   lobby_height == 2: −25 ticks (min 0)
//   lobby_height >= 3: −50 ticks (min 0)
// Service-carrier exclusion is enforced by the CALLER
// (`applyBoardingStressUpdate`), matching the binary: service carriers
// skip `accumulate_elapsed_delay_into_current_sim` entirely, so this
// function never runs for them.

use crate::sims::states::LOBBY_FLOOR;
use crate::world::WorldState;

/// Upstream's `sim` parameter is typed `{ elapsedTicks: number }`, and
/// sims/scoring.ts passes a temporary `{ elapsedTicks }` object rather than
/// a sim; the function reads and writes that one field only.  So the Rust
/// takes the field itself: `&mut sim.elapsed_ticks` (or the temporary's
/// value).  A caller whose sim lives in `world.sims` copies the field out
/// and back (stress/accumulate-elapsed.rs does), which is the same thing.
pub fn reduce_elapsed_for_lobby_boarding(elapsed_ticks: &mut i64, source_floor: i64, world: &WorldState) {
    if source_floor != LOBBY_FLOOR {
        return;
    }
    // (`world.lobbyHeight ?? 1`: the field is always present.)
    let lobby_height = 1.max(world.lobby_height);
    let discount = if lobby_height >= 3 {
        50
    } else if lobby_height == 2 {
        25
    } else {
        0
    };
    if discount == 0 {
        return;
    }
    *elapsed_ticks = 0.max(*elapsed_ticks - discount);
}

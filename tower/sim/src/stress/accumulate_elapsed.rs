// ts: stress/accumulate-elapsed.ts

// 11e0:01f1 accumulate_elapsed_delay_into_current_sim
//
// Called from `assign_request_to_runtime_route` (1218:0d4e) when a sim
// boards a non-service carrier. Binary body:
//   elapsed = (elapsed_packed & 0x3ff) + g_day_tick - last_trip_tick
//   scale_delay_for_speed_mode(elapsed, source_floor)  // lobby reduction
//   clamp to 300, store back, clear last_trip_tick
//
// Current TS status: the boarding path in `queue/process-travel.ts`
// (`applyBoardingStressUpdate`) composes this behavior out of
// `rebaseSimElapsedFromClock` + `reduceElapsedForLobbyBoarding` rather
// than calling a single `accumulateElapsedDelayIntoCurrentSim` primitive.
// This wrapper exposes the binary-named entry point so future callers
// (e.g. a faithful `assignRequestToRuntimeRoute` port) can invoke it
// directly.

use super::lobby_reduction::reduce_elapsed_for_lobby_boarding;
use super::rebase_elapsed::rebase_sim_elapsed_from_clock;
use crate::time::TimeState;
use crate::world::WorldState;

/// (`sim` is `world.sims[sim_index]`.)
pub fn accumulate_elapsed_delay_into_current_sim(
    world: &mut WorldState,
    time: &TimeState,
    sim_index: i64,
    source_floor: i64,
) {
    let i = sim_index as usize;
    rebase_sim_elapsed_from_clock(&mut world.sims[i], time);
    let mut elapsed_ticks = world.sims[i].elapsed_ticks;
    reduce_elapsed_for_lobby_boarding(&mut elapsed_ticks, source_floor, world);
    world.sims[i].elapsed_ticks = elapsed_ticks;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{RouteState, SimRecord};

    pub(crate) fn sim(family_code: i64) -> SimRecord {
        SimRecord {
            floor_anchor: 12,
            home_column: 40,
            base_offset: 0,
            facility_slot: 0,
            family_code,
            state_code: 0,
            route: RouteState::Idle,
            selected_floor: 12,
            origin_floor: 12,
            destination_floor: -1,
            venue_return_state: 0,
            queue_tick: 0,
            elapsed_ticks: 0,
            transit_ticks_remaining: 0,
            last_demand_tick: -1,
            trip_count: 0,
            accumulated_ticks: 0,
            target_room_floor: -1,
            target_room_column: -1,
            spawn_floor: -1,
            post_claim_countdown: 0,
            encoded_target_floor: 0,
            commercial_venue_slot: -1,
        }
    }

    #[test]
    fn rebase_then_lobby_discount() {
        let mut world = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        let mut time = crate::time::create_time_state();
        let mut s = sim(crate::resources::FAMILY_OFFICE);
        s.elapsed_ticks = 20;
        s.last_demand_tick = 100;
        world.sims.push(s);
        time.day_tick = 180;
        world.lobby_height = 2;
        // 20 + 180 - 100 = 100, less 25 for a two-floor lobby.
        accumulate_elapsed_delay_into_current_sim(&mut world, &time, 0, crate::sims::states::LOBBY_FLOOR);
        assert_eq!((world.sims[0].elapsed_ticks, world.sims[0].last_demand_tick), (75, -1));
        // Off the lobby: no discount; clamped at 300.
        world.sims[0].last_demand_tick = 0;
        time.day_tick = 1000;
        accumulate_elapsed_delay_into_current_sim(&mut world, &time, 0, 30);
        assert_eq!(world.sims[0].elapsed_ticks, 300);
        // A three-floor lobby takes 50, never below 0.
        world.lobby_height = 3;
        let mut e = 30;
        reduce_elapsed_for_lobby_boarding(&mut e, crate::sims::states::LOBBY_FLOOR, &world);
        assert_eq!(e, 0);
    }

    #[test]
    fn housekeeping_keeps_no_stress() {
        let mut s = sim(crate::resources::FAMILY_HOUSEKEEPING);
        s.last_demand_tick = 5;
        crate::stress::add_delay::add_delay_to_current_sim(&mut s, 300);
        crate::stress::trip_counters::advance_sim_trip_counters(&mut s);
        assert_eq!((s.elapsed_ticks, s.trip_count, s.last_demand_tick), (0, 0, 5));
        let mut s = sim(crate::resources::FAMILY_OFFICE);
        s.elapsed_ticks = 290;
        crate::stress::add_delay::add_delay_to_current_sim(&mut s, 35);
        assert_eq!(s.elapsed_ticks, 300);
        crate::stress::trip_counters::advance_sim_trip_counters(&mut s);
        assert_eq!((s.elapsed_ticks, s.trip_count, s.accumulated_ticks), (0, 1, 300));
    }
}

// ts: tick/day-scheduler.ts

// 1208:0196 run_simulation_day_scheduler
//
// Advances g_day_tick and fires checkpoint handlers. In the binary the event
// rolls (VIP visitor, bomb, fire, random news) are triggered inside the
// checkpoint handler table; we keep them as free-standing calls for now and
// route them through this module in the same relative order as the previous
// `TowerSim.step()` implementation.

use indexmap::IndexMap;

use crate::carriers::flush_carriers_end_of_day;
use crate::cathedral::{activate_eval_sims, dispatch_eval_midday_return};
use crate::daily::drain_active_requests::dispatch_active_requests_by_family;
use crate::entertainment::{
    activate_entertainment_lower_half, activate_entertainment_upper_half,
    advance_entertainment_lower_paired_phase_and_accrue, advance_entertainment_upper_phase,
    advance_party_hall_phase_and_accrue, promote_cinema_and_activate_party_hall,
    promote_cinema_links_to_ready_phase, seed_entertainment_budgets,
};
use crate::events::{
    check_daily_events, tick_bomb_event, tick_fire_event, tick_vip_special_visitor, trigger_random_news_event,
};
use crate::families::tile_spans::rebuild_all_sim_tile_spans;
use crate::ledger::{
    activate_three_day_cashflow, do_expense_sweep, do_ledger_rollover, rebuild_facility_ledger, LedgerState,
};
use crate::recycling::{reset_recycling_center_duty_tier, update_recycling_center_state};
use crate::resources::{
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_OFFICE, FAMILY_RESTAURANT,
    FAMILY_RETAIL,
};
use crate::sims::facility_refunds::{
    close_commercial_venues_by_family, rebuild_commercial_venue_runtime, rebuild_restaurant_facility_records,
    refund_unhappy_facilities,
};
use crate::sims::hotel_facilities::{
    advance_object_stay_phase_tiers, normalize_unit_status_end_of_day, spread_cockroach_infestation,
    update_hotel_operational_and_occupancy,
};
use crate::sims::population::reset_sim_runtime_state;
use crate::sims::scoring::recompute_all_object_operational_status;
use crate::sims::states::{STATE_HOTEL_PARKED, STATE_MORNING_GATE};
use crate::sims::trip_counters::reset_sim_trip_counters;
use crate::time::{advance_one_tick, TimeState};
use crate::world::WorldState;

// ─── Sim state bundle ─────────────────────────────────────────────────────────

/// The `{ world, ledger, time }` context object (TRANSLATION.md's SimCtx):
/// upstream's `SimState` here and `ServiceIdleTasksContext` in
/// tick/service-idle-tasks.ts are both this shape, holding the TowerSim's
/// own state by reference as the JavaScript objects do.
pub struct SimCtx<'a> {
    pub world: &'a mut WorldState,
    pub ledger: &'a mut LedgerState,
    pub time: &'a mut TimeState,
}

/// Upstream's `SimState { time, world, ledger }`.
pub type SimState<'a> = SimCtx<'a>;

// ─── Checkpoint bodies ────────────────────────────────────────────────────────

fn checkpoint_start_of_day(s: &mut SimState) {
    // Binary: update_periodic_facility_progress_override (1020:0df8) — every
    // 8th day (dayCounter % 8 === 4), if tower is below 5 stars, enable the
    // override seed slot AND set bit 0x10 in g_game_state_flags. Cleared at
    // midday (0x640) by clear_facility_progress_override (1020:0e1c).
    if s.time.day_counter % 8 == 4 && s.world.star_count < 5 && (s.world.event_state.game_state_flags & 0x10) == 0 {
        s.world.gate_flags.facility_progress_override = 1;
        s.world.event_state.game_state_flags |= 0x10;
    }
    // Medical daily flag: latched to 1 at day-start when starCount > 2.
    // Cleared by failed medical trips during the day; gates star 3→4 and 4→5.
    if s.world.star_count > 2 {
        s.world.gate_flags.office_service_ok_medical = 1;
    }
    // Activate cathedral guest sims
    activate_eval_sims(s.world);
}

fn checkpoint_recycling_reset(s: &mut SimState) {
    reset_recycling_center_duty_tier(s.world);
}

fn checkpoint_facility_ledger_rebuild(s: &mut SimState) {
    check_daily_events(s.world, s.ledger, s.time);
    rebuild_facility_ledger(s.ledger, s.world);
    rebuild_commercial_venue_runtime(s.world, s.time);
    seed_entertainment_budgets(s.world);
}

fn checkpoint_entertainment_half1(s: &mut SimState) {
    // Binary 0x3e8 dispatches activate_entertainment_link_half_runtime_phase
    // only — it touches g_entertainment_link_table and sim-table state but
    // not g_commercial_venue_record_table. Earlier TS rolled fast-food/retail
    // today→yesterday here, which clobbered the day's accumulated visit
    // counts before the dayTick=240 ledger contribution could read them.
    activate_entertainment_upper_half(s.world);
}

fn checkpoint_hotel_sale_reset(s: &mut SimState) {
    s.world.gate_flags.family345_sale_count = 0;
    dispatch_eval_midday_return(s.world);
    promote_cinema_and_activate_party_hall(s.world);
    let mut hotel_family_counts: IndexMap<i64, i64> = IndexMap::new();
    for object in s.world.placed_objects.values() {
        if object.object_type_code == FAMILY_HOTEL_SINGLE || object.object_type_code == FAMILY_HOTEL_TWIN {
            *hotel_family_counts.entry(object.object_type_code).or_insert(0) += 1;
        }
    }
    if s.world.carriers.is_empty() {
        for sim in s.world.sims.iter_mut() {
            if sim.state_code == STATE_HOTEL_PARKED
                && (sim.family_code == FAMILY_HOTEL_SINGLE || sim.family_code == FAMILY_HOTEL_TWIN)
                && hotel_family_counts.get(&sim.family_code).copied().unwrap_or(0) == 1
            {
                sim.state_code = STATE_MORNING_GATE;
            }
        }
    }
}

fn checkpoint_entertainment_half2(s: &mut SimState) {
    // Binary 0x578: entertainment-only — see checkpointEntertainmentHalf1.
    activate_entertainment_lower_half(s.world);
}

fn checkpoint_entertainment_phase1(s: &mut SimState) {
    advance_entertainment_upper_phase(s.world);
}

fn checkpoint_midday(s: &mut SimState) {
    // Binary: clear_facility_progress_override (1020:0e1c) — disable override
    // seed slot AND clear bit 0x10 in g_game_state_flags (gated on bit set).
    if (s.world.event_state.game_state_flags & 0x10) != 0 {
        s.world.gate_flags.facility_progress_override = 0;
        s.world.event_state.game_state_flags &= !0x10;
    }
    // Spec execution order at checkpoint 0x640:
    // 0. rebuild_type6_facility_records (binary 1208:xxx): restaurant per-cycle
    //    seeding — reopens restaurants, refills remainingCapacity to 10, resets
    //    eligibility threshold, rolls visit counters.
    rebuild_restaurant_facility_records(s.world, s.time);
    // 1. Spread existing cockroach infestations
    spread_cockroach_infestation(s.world, s.time);
    // 2. Recompute hotel status + handle vacancy expiry + refresh occupancy
    update_hotel_operational_and_occupancy(s.world, s.time);
    // 3. Normal midday tasks (binary 0x640 commercial work is restricted to
    //    rebuild_type6_facility_records above; fast-food/retail counters
    //    stay untouched until dayTick=240's rebuild_linked_facility_records).
    advance_party_hall_phase_and_accrue(s.world, s.ledger, s.time.day_counter);
    // Binary 0x640: promote_entertainment_links_to_ready_phase(1, 1)
    // promotes cinema-paired link.linkPhaseState 2 -> 3.
    promote_cinema_links_to_ready_phase(s.world);
    update_recycling_center_state(s.world, s.ledger, 0);
    // 4. advance_object_stay_phase_tiers @ 1230:0b5f — raises unit_status bands
    //    (hotel 0x18→0x20, 0x28→0x30, 0x38→0x40; office 0x00→0x08, 0x10→0x18;
    //    condo 0x18→0x20 + low-band +8). Runs after the refresh pass so the
    //    gate sees the pre-advance status.
    advance_object_stay_phase_tiers(s.world);
}

fn checkpoint_noop(_s: &mut SimState) {
    // Intentional no-op (previously mislabeled in the spec)
}

fn checkpoint_entertainment_phase2(s: &mut SimState) {
    advance_entertainment_lower_paired_phase_and_accrue(s.world, s.ledger, s.time.day_counter);
}

fn checkpoint_late_facility(s: &mut SimState) {
    close_commercial_venues_by_family(s.world, s.ledger, FAMILY_RETAIL);
    close_commercial_venues_by_family(s.world, s.ledger, FAMILY_FAST_FOOD);
    update_recycling_center_state(s.world, s.ledger, 2);
    // Note: `tryAdvanceStarCount` no longer fires from this checkpoint. The
    // binary calls `check_and_advance_star_rating` (1148:002d) every tick
    // from `FUN_1098_03ab` (1098:03ab), which we mirror by invoking it at
    // the top of `carrierTick` in tick/carrier-tick.ts.
}

fn checkpoint_type6_advance(s: &mut SimState) {
    close_commercial_venues_by_family(s.world, s.ledger, FAMILY_RESTAURANT);
}

fn checkpoint_day_counter(s: &mut SimState) {
    // Increment dayCounter and recompute weekendFlag.
    // (time.ts already does this via advanceOneTick; this body is a no-op here
    //  because time state is mutated in advanceOneTick before runCheckpoints.)
    let _ = s;
}

fn checkpoint_runtime_refresh(s: &mut SimState) {
    rebuild_all_sim_tile_spans(s.world);
    reset_sim_runtime_state(s.world);
    normalize_unit_status_end_of_day(s.world);
    // Binary 1208:0196 at g_day_tick == 0x9c4 additionally fires
    // 1190:0977 dispatch_active_requests_by_family — a once-per-day
    // sweep over the active-request table that re-routes stuck sims
    // through their family dispatch handler.
    dispatch_active_requests_by_family(s.world, s.ledger, s.time);
}

fn checkpoint_ledger_rollover(s: &mut SimState) {
    // Binary also calls reset_sim_runtime_state at checkpoint 0x9e5.
    reset_sim_runtime_state(s.world);
    do_ledger_rollover(s.ledger, s.world, s.time.day_counter);
    // BOX: the BOX ledgers' period with upstream's.
    crate::box_ext::period_rollover(s.world, s.time.day_counter);
    // BOX: amenities -- rooms open, their units chosen, the day's visits
    // paid (before the scores, which their relief enters).
    crate::box_ext::amenities::daily(s.world, s.ledger, s.time);
    // BOX: car elevator -- the day's parking fees.
    crate::box_ext::car_elevator::daily(s.world, s.ledger);
    // BOX: fire -- floors closed by a fire open again.
    crate::box_ext::fire::daily(s.world);
    // BOX: locals -- the office jobs the tower's residents hold, given out.
    crate::box_ext::locals::daily(s.world);
    recompute_all_object_operational_status(s.world);
    if s.time.day_counter % 3 == 0 {
        activate_three_day_cashflow(s.world, s.ledger, s.time.day_counter);
        do_expense_sweep(s.ledger, s.world);
        // BOX: amenities -- their upkeep with the other running costs.
        crate::box_ext::amenities::upkeep(s.world, s.ledger);
        refund_unhappy_facilities(s.world, s.ledger, s.time);
        // Binary activate_family_cashflow_if_operational resets trip counters for
        // all office and condo sims on the 3-day cycle (spec: 2533 step 2).
        for sim in s.world.sims.iter_mut() {
            if sim.family_code == FAMILY_OFFICE || sim.family_code == FAMILY_CONDO {
                reset_sim_trip_counters(sim);
            }
        }
    }
}

fn checkpoint_end_of_day(s: &mut SimState) {
    flush_carriers_end_of_day(s.world);
}

fn checkpoint_recycling_final(s: &mut SimState) {
    update_recycling_center_state(s.world, s.ledger, 5);
}

// ─── Checkpoint table ─────────────────────────────────────────────────────────

type Checkpoint = fn(&mut SimState);

const CHECKPOINTS: &[(i64, Checkpoint)] = &[
    (0x000, checkpoint_start_of_day),
    (0x020, checkpoint_recycling_reset),
    (0x0f0, checkpoint_facility_ledger_rebuild),
    (0x3e8, checkpoint_entertainment_half1),
    (0x4b0, checkpoint_hotel_sale_reset),
    (0x578, checkpoint_entertainment_half2),
    (0x5dc, checkpoint_entertainment_phase1),
    (0x640, checkpoint_midday),
    (0x708, checkpoint_noop),
    (0x76c, checkpoint_entertainment_phase2),
    (0x7d0, checkpoint_late_facility),
    (0x898, checkpoint_type6_advance),
    (0x8fc, checkpoint_day_counter),
    (0x9c4, checkpoint_runtime_refresh),
    (0x9e5, checkpoint_ledger_rollover),
    (0x9f6, checkpoint_end_of_day),
    (0x0a06, checkpoint_recycling_final),
];

// ─── Dispatcher ───────────────────────────────────────────────────────────────

/// Fire all checkpoints whose tick falls in the half-open interval
/// (prev_tick, curr_tick].  Handles day wraparound: when curr_tick < prev_tick
/// the tick counter crossed zero, so checkpoints at tick 0 are included.
pub fn run_checkpoints(state: &mut SimState, prev_tick: i64, curr_tick: i64) {
    let wrapped = curr_tick < prev_tick; // day boundary crossed this step
    for (tick, f) in CHECKPOINTS {
        let tick = *tick;
        if wrapped {
            // Fire everything after prev_tick through day-end, then 0..curr_tick
            if tick > prev_tick || tick <= curr_tick {
                f(state);
            }
        } else if tick > prev_tick && tick <= curr_tick {
            f(state);
        }
    }
}

/// Binary `run_simulation_day_scheduler` (1208:0196).
///
/// Advances `g_day_tick` and fires all checkpoint handlers. Also drives the
/// per-tick event rolls (random news, VIP visitor, bomb, fire). The TS order
/// preserved here matches the previous `TowerSim.step()` sequence:
///
///   advanceOneTick
///   triggerRandomNewsEvent
///   tickVipSpecialVisitor
///   runCheckpoints
///   tickBombEvent
///   tickFireEvent
///
/// Returns the updated TimeState. Callers should assign the result back onto
/// their time slot before invoking `carrierTick`.
pub fn run_simulation_day_scheduler(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) -> TimeState {
    let prev_tick = time.day_tick;
    let (mut next_time, _) = advance_one_tick(time);
    let curr_tick = next_time.day_tick;

    trigger_random_news_event(world, &next_time);
    tick_vip_special_visitor(world, &next_time);
    run_checkpoints(
        &mut SimState {
            time: &mut next_time,
            world: &mut *world,
            ledger: &mut *ledger,
        },
        prev_tick,
        curr_tick,
    );

    // The bomb cleanup and the fire's resolution write the day tick through
    // `nextTime` (events.ts), and the caller sees it.
    tick_bomb_event(world, ledger, &mut next_time);
    tick_fire_event(world, ledger, &mut next_time);

    next_time
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which checkpoint ticks `run_checkpoints`' interval test selects (the
    /// same test over the table's ticks, without running the bodies).
    fn selected(prev: i64, curr: i64) -> Vec<i64> {
        let wrapped = curr < prev;
        CHECKPOINTS
            .iter()
            .map(|(t, _)| *t)
            .filter(|&t| if wrapped { t > prev || t <= curr } else { t > prev && t <= curr })
            .collect()
    }

    #[test]
    fn checkpoint_intervals() {
        assert_eq!(selected(0x9e4, 0x9e5), [0x9e5]);
        assert_eq!(selected(0x9e5, 0x9e6), Vec::<i64>::new());
        // The wrap from 2599 to 0 fires tick 0 only.
        assert_eq!(selected(2599, 0), [0]);
        assert_eq!(selected(0, 1), Vec::<i64>::new());
        assert_eq!(selected(0xef, 0xf0), [0xf0]);
        assert_eq!(selected(0xa05, 0xa06), [0xa06]);
        // A fast-forward to 1500 (bomb or fire cleanup) skips nothing at
        // the next step: (1500, 1501] holds no checkpoint.
        assert_eq!(selected(1500, 1501), Vec::<i64>::new());
    }

    #[test]
    fn table_is_in_tick_order() {
        let ticks: Vec<i64> = CHECKPOINTS.iter().map(|(t, _)| *t).collect();
        let mut sorted = ticks.clone();
        sorted.sort();
        assert_eq!(ticks, sorted);
        assert_eq!(ticks.len(), 17);
    }
}

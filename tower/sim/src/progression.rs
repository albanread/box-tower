// ts: progression.ts

// Star advancement (binary `check_and_advance_star_rating` @ 1148:002d /
// `compute_tower_tier_from_ledger` @ 1148:041d / `reset_star_gate_state`).
//
// Runs every tick from `FUN_1098_03ab` (the per-tick scheduler that we mirror
// in `tick/carrier-tick.ts`). Two independent checks must both pass for
// starCount to advance:
//
// 1. `computeTowerTierFromLedger()` > current starCount — the ledger total
//    `g_primary_family_ledger_total` (1288:c13a, modeled here as
//    `world.currentPopulation`) crosses the next tier threshold.
//    Thresholds (binary DS:e630..e63c + hardcoded 15000): [300, 1000, 5000,
//    10000, 15000]. Comparison is `>=` (binary uses `< threshold` for the
//    lower-tier branch).
// 2. `checkStarAdvancementConditions()` — all qualitative gates for the
//    current-tier transition are satisfied.
//
// On success, starCount is incremented (capped at 5 here — rank 6 "Tower"
// is reached exclusively via the cathedral evaluation path in cathedral.ts),
// the per-day star-gate flags are reset, and a `star_advanced` notification
// is queued.

use crate::resources::STAR_THRESHOLDS;
use crate::time::TimeState;
use crate::world::{SimNotification, SimNotificationKind, WorldState};

pub fn compute_star_count_from_population(world: &WorldState) -> i64 {
    let total = world.current_population;
    let mut tier = 1;
    for (index, threshold) in STAR_THRESHOLDS.iter().enumerate() {
        // Binary compares `total < THRESHOLD[index]` to keep tier == index+1;
        // the inverse here (>=) advances tier to index+2.
        if total >= *threshold {
            tier = index as i64 + 2;
        }
    }
    tier
}

/// Mirrors binary `add_to_primary_family_ledger_bucket` (1068:07f7).
/// Binary 1068:0812-082b: bucket[family] += amount AND total += amount, with
/// the total update unconditional. We mirror both so the running total stays
/// consistent with the per-family bucket sums and so
/// `clearPopulationBucket` can later subtract the right amount.
pub fn add_to_population_bucket(world: &mut WorldState, family_code: i64, amount: i64) {
    let key = family_code.to_string();
    let current = world.current_population_buckets.get(&key).copied().unwrap_or(0);
    world.current_population_buckets.insert(key, current + amount);
    world.current_population += amount;
}

/// Mirrors binary `clear_primary_family_ledger_bucket` (1068:07b3) at
/// 1068:07d3-07e2: subtract the bucket's current value from the running total,
/// then zero the bucket. Used by daily reseed paths (e.g.
/// `rebuild_linked_facility_records` for non-restaurant commercial families)
/// so that re-adding the new yesterday-visit-count yields a NET delta of
/// `(newVisits - oldVisits)` on the total.
pub fn clear_population_bucket(world: &mut WorldState, family_code: i64) {
    let key = family_code.to_string();
    let current = world.current_population_buckets.get(&key).copied().unwrap_or(0);
    world.current_population -= current;
    world.current_population_buckets.insert(key, 0);
}

pub fn check_star_advancement_conditions(world: &WorldState, time: &TimeState) -> bool {
    let flags = &world.gate_flags;
    match world.star_count {
        1 => true,
        2 => flags.security_placed != 0,
        3 => {
            flags.office_placed != 0
                && flags.recycling_adequate != 0
                && flags.office_service_ok != 0
                && flags.office_service_ok_medical != 0
                && flags.routes_viable != 0
                && time.daypart_index >= 4
                && time.weekend_flag == 0
        }
        4 => {
            flags.metro_placed != 0
                && flags.recycling_adequate != 0
                && flags.office_service_ok_medical != 0
                && flags.routes_viable != 0
                && time.daypart_index >= 4
                && time.weekend_flag == 0
        }
        _ => false,
    }
}

pub fn reset_star_gate_state(world: &mut WorldState) {
    world.gate_flags.office_service_ok = 0;
}

pub fn try_advance_star_count(world: &mut WorldState, time: &TimeState) -> bool {
    if world.star_count >= 5 {
        return false;
    }
    if compute_star_count_from_population(world) <= world.star_count {
        return false;
    }
    if !check_star_advancement_conditions(world, time) {
        return false;
    }

    world.star_count += 1;
    reset_star_gate_state(world);
    world.pending_notifications.push(SimNotification {
        kind: SimNotificationKind::StarAdvanced,
        message: Some(world.star_count.to_string()),
    });
    true
}

#[cfg(test)]
mod tests {
    // ts: progression.test.ts
    use super::*;
    use crate::time::create_time_state;
    use crate::world::PlacedObjectRecord;

    fn make_world() -> WorldState {
        let mut w = crate::snapshot::create_initial_snapshot("t", "T", 0, Some(crate::world::LobbyMode::PerfectParity))
            .world;
        w.special_links.clear();
        w.special_link_records.clear();
        w.transfer_group_entries.clear();
        w
    }

    fn add_object_with_activation(world: &mut WorldState, key: &str, ledger_contribution: i64) {
        let object = PlacedObjectRecord {
            left_tile_index: 0,
            right_tile_index: 0,
            object_type_code: 0,
            unit_status: 0,
            aux_value_or_timer: 0,
            linked_record_index: -1,
            dirty_flag: 0,
            occupied_flag: 0,
            eval_level: 0,
            eval_score: -1,
            rent_level: 1,
            activation_tick_count: 0,
            housekeeping_claimed_flag: 0,
            vip_flag: None,
        };
        world.detach_object(&key);
        world.placed_objects.insert(key, object);
        // Mirrors `add_to_primary_family_ledger_bucket` (1068:07f7): the binary
        // stores the ledger total in g_primary_family_ledger_total; this test
        // helper bumps the same field so star-tier checks see the contribution.
        world.current_population += ledger_contribution;
    }

    fn late_afternoon_time() -> TimeState {
        let mut t = create_time_state();
        t.day_tick = 2000;
        t.daypart_index = 5;
        t.day_counter = 0;
        t.weekend_flag = 0;
        t
    }

    // describe("computeTowerTierFromLedger")

    #[test]
    fn returns_tier_1_for_empty_tower() {
        let world = make_world();
        assert_eq!(compute_star_count_from_population(&world), 1);
    }

    #[test]
    fn returns_tier_2_once_first_threshold_is_met() {
        let mut world = make_world();
        // Binary `compute_tower_tier_from_ledger` (1148:041d) compares
        // `total < THRESHOLD` for the lower-tier branch, so reaching the
        // threshold value (e.g. 300) is enough to advance.
        add_object_with_activation(&mut world, "a", STAR_THRESHOLDS[0]);
        assert_eq!(compute_star_count_from_population(&world), 2);
    }

    #[test]
    fn stays_in_lower_tier_just_below_the_threshold() {
        let mut world = make_world();
        add_object_with_activation(&mut world, "a", STAR_THRESHOLDS[0] - 1);
        assert_eq!(compute_star_count_from_population(&world), 1);
    }

    #[test]
    fn sums_contributions_to_the_primary_ledger_total() {
        let mut world = make_world();
        add_object_with_activation(&mut world, "a", 200);
        assert_eq!(compute_star_count_from_population(&world), 1);
        add_object_with_activation(&mut world, "b", 100);
        assert_eq!(compute_star_count_from_population(&world), 2);
    }

    #[test]
    fn returns_tier_6_when_ledger_reaches_the_top_threshold() {
        let mut world = make_world();
        add_object_with_activation(&mut world, "a", STAR_THRESHOLDS[4]);
        assert_eq!(compute_star_count_from_population(&world), 6);
    }

    // describe("checkStarAdvancementConditions")

    #[test]
    fn one_to_two_has_no_qualitative_gate() {
        let world = make_world();
        assert!(check_star_advancement_conditions(&world, &late_afternoon_time()));
    }

    #[test]
    fn two_to_three_requires_security_placed() {
        let mut world = make_world();
        world.star_count = 2;
        assert!(!check_star_advancement_conditions(&world, &late_afternoon_time()));
        world.gate_flags.security_placed = 1;
        assert!(check_star_advancement_conditions(&world, &late_afternoon_time()));
    }

    #[test]
    fn three_to_four_requires_all_office_tier_gates_daypart_and_weekend_flag() {
        let mut world = make_world();
        world.star_count = 3;
        world.gate_flags.office_placed = 1;
        world.gate_flags.recycling_adequate = 1;
        world.gate_flags.office_service_ok = 1;
        world.gate_flags.office_service_ok_medical = 1;
        world.gate_flags.routes_viable = 1;

        let time = late_afternoon_time();
        assert!(check_star_advancement_conditions(&world, &time));

        // Missing any flag blocks the gate
        world.gate_flags.office_service_ok = 0;
        assert!(!check_star_advancement_conditions(&world, &time));
        world.gate_flags.office_service_ok = 1;

        // daypart < 4 blocks
        let early_time = TimeState { daypart_index: 3, ..time.clone() };
        assert!(!check_star_advancement_conditions(&world, &early_time));

        // weekendFlag != 0 blocks
        let weekend_time = TimeState { weekend_flag: 1, ..time.clone() };
        assert!(!check_star_advancement_conditions(&world, &weekend_time));
    }

    #[test]
    fn four_to_five_requires_metro_placed_not_office_service_ok() {
        let mut world = make_world();
        world.star_count = 4;
        world.gate_flags.metro_placed = 1;
        world.gate_flags.recycling_adequate = 1;
        world.gate_flags.office_service_ok_medical = 1;
        world.gate_flags.routes_viable = 1;

        let time = late_afternoon_time();
        assert!(check_star_advancement_conditions(&world, &time));

        world.gate_flags.metro_placed = 0;
        assert!(!check_star_advancement_conditions(&world, &time));
    }

    #[test]
    fn returns_false_at_star_count_5_or_more() {
        let mut world = make_world();
        world.star_count = 5;
        assert!(!check_star_advancement_conditions(&world, &late_afternoon_time()));
    }

    // describe("tryAdvanceStarCount")

    #[test]
    fn advances_one_to_two_once_the_activity_threshold_is_met() {
        let mut world = make_world();
        add_object_with_activation(&mut world, "a", STAR_THRESHOLDS[0]);
        assert!(try_advance_star_count(&mut world, &late_afternoon_time()));
        assert_eq!(world.star_count, 2);
        assert_eq!(
            serde_json::to_value(&world.pending_notifications).unwrap(),
            serde_json::json!([{ "kind": "star_advanced", "message": "2" }])
        );
    }

    #[test]
    fn does_nothing_if_activity_threshold_not_met() {
        let mut world = make_world();
        add_object_with_activation(&mut world, "a", 10);
        assert!(!try_advance_star_count(&mut world, &late_afternoon_time()));
        assert_eq!(world.star_count, 1);
        assert!(world.pending_notifications.is_empty());
    }

    #[test]
    fn does_nothing_if_qualitative_gate_fails() {
        let mut world = make_world();
        world.star_count = 2;
        add_object_with_activation(&mut world, "a", STAR_THRESHOLDS[1]);
        assert!(!try_advance_star_count(&mut world, &late_afternoon_time()));
        assert_eq!(world.star_count, 2);
    }

    #[test]
    fn resets_office_service_ok_on_advancement() {
        let mut world = make_world();
        world.star_count = 3;
        add_object_with_activation(&mut world, "a", STAR_THRESHOLDS[2]);
        world.gate_flags.office_placed = 1;
        world.gate_flags.recycling_adequate = 1;
        world.gate_flags.office_service_ok = 1;
        world.gate_flags.office_service_ok_medical = 1;
        world.gate_flags.routes_viable = 1;
        assert!(try_advance_star_count(&mut world, &late_afternoon_time()));
        assert_eq!(world.star_count, 4);
        assert_eq!(world.gate_flags.office_service_ok, 0);
    }

    #[test]
    fn caps_at_5_stars() {
        let mut world = make_world();
        world.star_count = 5;
        add_object_with_activation(&mut world, "a", STAR_THRESHOLDS[4]);
        assert!(!try_advance_star_count(&mut world, &late_afternoon_time()));
        assert_eq!(world.star_count, 5);
    }

    // Not upstream's: the population buckets.

    #[test]
    fn population_buckets_net_out() {
        let mut world = make_world();
        add_to_population_bucket(&mut world, 12, 10);
        add_to_population_bucket(&mut world, 7, 6);
        add_to_population_bucket(&mut world, 12, 5);
        assert_eq!(world.current_population, 21);
        clear_population_bucket(&mut world, 12);
        assert_eq!(world.current_population, 6);
        add_to_population_bucket(&mut world, 12, 4);
        assert_eq!(world.current_population, 10);
        // Numeric keys iterate in ascending order, as JavaScript's do.
        assert_eq!(
            serde_json::to_string(&world.current_population_buckets).unwrap(),
            r#"{"7":6,"12":4}"#
        );
        clear_population_bucket(&mut world, 18);
        assert_eq!(
            serde_json::to_string(&world.current_population_buckets).unwrap(),
            r#"{"7":6,"12":4,"18":0}"#
        );
    }
}

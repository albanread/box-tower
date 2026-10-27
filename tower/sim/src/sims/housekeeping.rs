// ts: sims/housekeeping.ts

use std::collections::HashMap;

use crate::queue::resolve::{resolve_sim_route_between_floors, ResolveSimRouteOptions};
use crate::resources::FAMILY_HOUSEKEEPING;
use crate::sims::states::{
    HK_CLAIM_DAY_TICK_CUTOFF, HK_FLOOR_CLASS_MOD, HK_POST_CLAIM_COUNTDOWN, HK_SEARCHING_SENTINEL,
    HK_STATE_COUNTDOWN, HK_STATE_ROUTE_TO_CANDIDATE, HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT,
    HK_STATE_ROUTE_TO_TARGET, HK_STATE_SEARCH, HOTEL_FAMILIES, STATE_ARRIVED, STATE_HOTEL_PARKED,
};
use crate::time::TimeState;
use crate::world::{y_to_floor, PlacedObjectRecord, SimRecord, WorldState};

const HOTEL_TURNOVER_STATUS: &[i64] = &[0x28, 0x30];

/// Upstream holds the room's PlacedObjectRecord; here its key.
struct FoundRoom {
    floor: i64,
    object_key: String,
    #[allow(dead_code)]
    subtype_byte: i64,
}

/// The x and y of an "x,y" key (`key.split(",").map(Number)`).
fn key_xy(key: &str) -> (i64, i64) {
    let mut parts = key.split(',').map(|p| {
        p.parse::<i64>()
            .unwrap_or_else(|_| panic!("key {key:?} is not \"x,y\""))
    });
    let x = parts.next().unwrap();
    let y = parts.next().unwrap_or_else(|| panic!("key {key:?} is not \"x,y\""));
    (x, y)
}

fn is_claimable_hotel_room(object: &PlacedObjectRecord) -> bool {
    if !HOTEL_FAMILIES.contains(&object.object_type_code) {
        return false;
    }
    HOTEL_TURNOVER_STATUS.contains(&object.unit_status)
}

/// Scan upward from `spawnFloor` to the top of the tower, then downward from
/// `spawnFloor - 1`. Within each floor satisfying `floor % 6 === floorClass`,
/// scan room slots in ascending subtype/slot (x) order and return the first
/// turnover-band (`0x28`/`0x30`) hotel-family slot. Returns null if none found.
fn find_vacant_hotel_room_for_claim(world: &WorldState, spawn_floor: i64, floor_class: i64) -> Option<FoundRoom> {
    // Only looked up by floor, so a HashMap serves for upstream's Map.
    let mut by_floor: HashMap<i64, Vec<(i64, &str, &PlacedObjectRecord)>> = HashMap::new();
    for (key, object) in world.placed_objects.iter() {
        let (x, y) = key_xy(key);
        let floor = y_to_floor(y);
        by_floor.entry(floor).or_default().push((x, key, object));
    }
    for list in by_floor.values_mut() {
        list.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let try_floor = |floor: i64| -> Option<FoundRoom> {
        if floor < 0 || floor >= world.height {
            return None;
        }
        if ((floor % HK_FLOOR_CLASS_MOD) + HK_FLOOR_CLASS_MOD) % HK_FLOOR_CLASS_MOD != floor_class {
            return None;
        }
        let list = by_floor.get(&floor)?;
        for &(x, key, object) in list {
            if is_claimable_hotel_room(object) {
                return Some(FoundRoom {
                    floor,
                    object_key: key.to_string(),
                    subtype_byte: x & 0xff,
                });
            }
        }
        None
    };

    let mut floor = spawn_floor;
    while floor < world.height {
        if let Some(hit) = try_floor(floor) {
            return Some(hit);
        }
        floor += 1;
    }
    let mut floor = spawn_floor - 1;
    while floor >= 0 {
        if let Some(hit) = try_floor(floor) {
            return Some(hit);
        }
        floor -= 1;
    }
    None
}

fn attempt_route_to_floor(world: &mut WorldState, sim_index: i64, target_floor: i64, time: &TimeState) -> i64 {
    // Binary `sim+0` is the HK helper's *current* floor (updated on arrival).
    // TS splits that into two fields: `floorAnchor` stays fixed as the tile
    // linkage key (required by `rebuildRuntimeSims`), while `selectedFloor`
    // tracks current position as the HK helper roams.
    let selected_floor = world.sims[sim_index as usize].selected_floor;
    let direction = if target_floor > selected_floor { 1 } else { 0 };
    // Binary 1228:620f / 1228:6320: housekeeping passes `is_passenger_route = 0`
    // AND `emit_distance_feedback = 0`. The post-resolve delay/trip-counter
    // writes are no-ops for housekeeping anyway (advanceSimTripCounters and
    // addDelayToCurrentSim early-return for FAMILY_HOUSEKEEPING), but pass the
    // flags explicitly to mirror the binary call shape.
    resolve_sim_route_between_floors(
        world,
        sim_index,
        selected_floor,
        target_floor,
        direction,
        Some(time),
        Some(ResolveSimRouteOptions {
            is_passenger_route: Some(false),
            emit_distance_feedback: Some(false),
            ..Default::default()
        }),
    )
}

/// Upstream takes the room's PlacedObjectRecord or undefined; here its key.
fn promote_claim(world: &mut WorldState, time: &TimeState, sim_index: i64, object_key: Option<&str>) {
    // Binary `activate_selected_vacant_unit` (1158:02e2) bails on:
    //   - room_family ∉ [3,5] (1158:0381 / 03b3)
    //   - room.unitStatus ∉ {0x28, 0x30} (1158:03e5 / 0415)
    // When it bails, no fields are written. The caller (state-1 result-3 arm
    // at 1228:6264) unconditionally sets sim+5 = 2 / sim+0xa = 3 afterward,
    // so the helper still enters state-2 with countdown 3. Mirror that here.
    let claimable = object_key.is_some_and(|key| is_claimable_hotel_room(&world.object(key).unwrap()));
    if let (true, Some(object_key)) = (claimable, object_key) {
        // Binary `activate_selected_vacant_unit` (1158:02e2): the cleaned-room
        // unit_status is 0x18 while daypart_index < 4, and 0x20 otherwise, i.e.
        // the same morning/evening VACANT bucket `place_object` would have picked
        // when the room was first placed. The room is *not* re-randomized to the
        // occupied trip-counter band — that happens when the new guest arrives.
        let object = world.object_mut(object_key).unwrap();
        object.unit_status = if time.daypart_index < 4 { 0x18 } else { 0x20 };
        object.housekeeping_claimed_flag = 1;
        // The binary also flips the first guest occupant of the cleaned room to
        // state 3 (ARRIVED) so the hotel-family state machine picks up the new
        // occupancy on its next stride.
        let (object_type_code, left_tile_index) = (object.object_type_code, object.left_tile_index);
        let target_room_floor = world.sims[sim_index as usize].target_room_floor;
        let new_occupant = world.sims.iter_mut().find(|candidate| {
            candidate.family_code == object_type_code
                && candidate.floor_anchor == target_room_floor
                && candidate.home_column == left_tile_index
                && candidate.base_offset == 0
        });
        if let Some(new_occupant) = new_occupant {
            new_occupant.state_code = STATE_ARRIVED;
        }
    }
    let sim = &mut world.sims[sim_index as usize];
    sim.post_claim_countdown = HK_POST_CLAIM_COUNTDOWN;
    sim.state_code = HK_STATE_COUNTDOWN;
}

fn reset_to_search(sim: &mut SimRecord) {
    sim.state_code = HK_STATE_SEARCH;
    sim.target_room_floor = HK_SEARCHING_SENTINEL;
    sim.target_room_column = -1;
    sim.post_claim_countdown = 0;
}

/// Binary `flag_selected_unit_unavailable` (1158:04e2): called when the HK
/// helper's post-claim countdown (sim+10) reaches 0 in state 2. Looks up the
/// first occupant (base_offset=0) of the claimed hotel-family room and writes
/// stateCode = 0x24 (STATE_HOTEL_PARKED) so the hotel state machine picks up
/// the room on its next stride, and sets the object's "dirty" byte to 1.
fn flag_selected_unit_unavailable(world: &mut WorldState, sim_index: i64) {
    let sim = &world.sims[sim_index as usize];
    if sim.target_room_floor < 0 || sim.target_room_column < 0 {
        return;
    }
    let target_room_floor = sim.target_room_floor;
    let y = world.height - 1 - target_room_floor;
    let Some(room) = world.object(&format!("{},{}", sim.target_room_column, y)) else {
        return;
    };
    if !HOTEL_FAMILIES.contains(&room.object_type_code) {
        return;
    }
    let (object_type_code, left_tile_index) = (room.object_type_code, room.left_tile_index);
    let first_occupant = world.sims.iter_mut().find(|candidate| {
        candidate.family_code == object_type_code
            && candidate.floor_anchor == target_room_floor
            && candidate.home_column == left_tile_index
            && candidate.base_offset == 0
    });
    if let Some(first_occupant) = first_occupant {
        first_occupant.state_code = STATE_HOTEL_PARKED;
    }
}

pub fn process_housekeeping_sim(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    if world.sims[si].family_code != FAMILY_HOUSEKEEPING {
        return;
    }

    match world.sims[si].state_code {
        HK_STATE_SEARCH => {
            // Binary state-0 handler only progresses while day_tick < 1500.
            if time.day_tick >= HK_CLAIM_DAY_TICK_CUTOFF {
                return;
            }
            let sim = &mut world.sims[si];
            if sim.spawn_floor < 0 {
                sim.spawn_floor = sim.floor_anchor;
            }

            // Binary `find_matching_vacant_unit_floor` (1158:0000) uses
            // `get_current_sim_state_word(sim)` — the u16 at sim+2, populated by
            // `initialize_runtime_entities_for_object_span` with the occupant's
            // span index (0..population-1). For housekeeping (pop=6), this is the
            // TS `baseOffset` field, which ranges 0..5 and acts as the floor-class
            // filter: each of the 6 HK helpers in a tile services a distinct
            // `floor % 6` residue.
            let floor_class = ((sim.base_offset % HK_FLOOR_CLASS_MOD) + HK_FLOOR_CLASS_MOD) % HK_FLOOR_CLASS_MOD;
            let spawn_floor = sim.spawn_floor;
            let candidate = find_vacant_hotel_room_for_claim(world, spawn_floor, floor_class);
            let Some(candidate) = candidate else {
                // Binary state-0: find_matching returns -1 → sim+6 = -1 → state = 1.
                // State 1 will next stride route from current_floor to sim+7 (spawn);
                // same-floor → reset → state 0, producing the observed 0↔1 oscillation.
                let sim = &mut world.sims[si];
                sim.target_room_floor = HK_SEARCHING_SENTINEL;
                sim.state_code = HK_STATE_ROUTE_TO_CANDIDATE;
                return;
            };
            // Binary falls through to the state-0/3 shared route resolution in the
            // same stride: state becomes 3 and the route to the target floor is
            // initiated immediately (result 0/1/2 → state 3, result 3 → claim,
            // result -1 → reset). Mirror that by advancing to state 3 and running
            // the state-3 body on the current stride.
            // Binary `find_matching_vacant_unit_floor` (1158:0000) writes
            // entity[+0xc] to identify the chosen room (slot rank within the
            // floor). On arrival the state-3 handler hands that stored
            // identifier to `activate_selected_vacant_unit`, which bails if the
            // specific room's family/phase don't match. Stash the room's
            // column at search time and look it up directly on arrival —
            // re-scanning the floor for *any* claimable room lets us steal a
            // room another helper is already routing to.
            let left_tile_index = world.object(candidate.object_key.as_str()).unwrap().left_tile_index;
            let sim = &mut world.sims[si];
            sim.target_room_floor = candidate.floor;
            sim.target_room_column = left_tile_index;
            sim.state_code = HK_STATE_ROUTE_TO_TARGET;
            try_claim_on_current_floor(world, time, sim_index);
        }

        HK_STATE_ROUTE_TO_CANDIDATE | HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT => {
            // Binary state-1/4 handler (jump table @ 1228:6404 / 1228:640a both
            // → 0x62ec): both call resolve(sim+7=spawn, sim+0a=arg) every
            // stride. rc=0/1/2 → state=4 (stay in transit; per-stride re-resolve
            // advances the leg). rc=-1/3 → state=0 (reset to search).
            let sim = &mut world.sims[si];
            if sim.selected_floor == sim.spawn_floor {
                if sim.target_room_floor == HK_SEARCHING_SENTINEL {
                    reset_to_search(sim);
                    return;
                }
                sim.state_code = HK_STATE_ROUTE_TO_TARGET;
                try_claim_on_current_floor(world, time, sim_index);
                return;
            }
            let spawn_floor = sim.spawn_floor;
            let result = attempt_route_to_floor(world, sim_index, spawn_floor, time);
            if result == -1 {
                reset_to_search(&mut world.sims[si]);
                return;
            }
            if result == 3 {
                world.sims[si].state_code = HK_STATE_ROUTE_TO_TARGET;
                try_claim_on_current_floor(world, time, sim_index);
                return;
            }
            if result != 0 {
                world.sims[si].state_code = HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT;
            }
        }

        HK_STATE_ROUTE_TO_TARGET => {
            try_claim_on_current_floor(world, time, sim_index);
        }

        HK_STATE_COUNTDOWN => {
            // Binary state-2 (1228:602b case 2): `if (sim+10 != 0) { sim+10--;
            // return; } else { flag_unavailable; sim+5 = 0; }`. The check-first
            // order gives 4 strides in state 2 for a starting value of 3, not 3.
            let sim = &mut world.sims[si];
            if sim.post_claim_countdown != 0 {
                sim.post_claim_countdown -= 1;
                return;
            }
            flag_selected_unit_unavailable(world, sim_index);
            reset_to_search(&mut world.sims[si]);
        }

        _ => reset_to_search(&mut world.sims[si]),
    }
}

fn try_claim_on_current_floor(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    if world.sims[si].selected_floor != world.sims[si].target_room_floor {
        let target_room_floor = world.sims[si].target_room_floor;
        let result = attempt_route_to_floor(world, sim_index, target_room_floor, time);
        if result == -1 {
            reset_to_search(&mut world.sims[si]);
            return;
        }
        // Queued / in-transit: stay in HK_STATE_ROUTE_TO_TARGET.
        return;
    }
    if time.day_tick >= HK_CLAIM_DAY_TICK_CUTOFF {
        reset_to_search(&mut world.sims[si]);
        return;
    }
    // Look up the specific room targeted at search time, not whatever's
    // currently claimable on this floor. promoteClaim handles the case where
    // the room's status no longer qualifies (binary's activate bails, but the
    // caller still transitions the helper to state 2).
    let sim = &world.sims[si];
    let y = world.height - 1 - sim.target_room_floor;
    let room_key = format!("{},{}", sim.target_room_column, y);
    let room = if sim.target_room_column >= 0 && world.placed_objects.contains_key(&room_key) {
        Some(room_key)
    } else {
        None
    };
    promote_claim(world, time, sim_index, room.as_deref());
}

pub fn handle_housekeeping_sim_arrival(world: &mut WorldState, time: &TimeState, sim_index: i64, arrival_floor: i64) {
    let sim = &mut world.sims[sim_index as usize];
    sim.selected_floor = arrival_floor;
    sim.destination_floor = -1;

    if sim.state_code == HK_STATE_ROUTE_TO_CANDIDATE || sim.state_code == HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT {
        if arrival_floor != sim.spawn_floor {
            return;
        }
        sim.state_code = HK_STATE_ROUTE_TO_TARGET;
        try_claim_on_current_floor(world, time, sim_index);
        return;
    }

    if sim.state_code == HK_STATE_ROUTE_TO_TARGET {
        if arrival_floor != sim.target_room_floor {
            return;
        }
        try_claim_on_current_floor(world, time, sim_index);
    }
}

#[cfg(test)]
mod tests {
    // Upstream's housekeeping.test.ts, all but "cross-floor candidate
    // transitions through route-to-candidate states", which needs
    // queue/resolve.ts (another owner's).
    use super::*;
    use crate::resources::FAMILY_HOTEL_SINGLE;
    use crate::time::create_time_state;
    use crate::world::{floor_to_y, LobbyMode, RouteState};

    fn make_world() -> WorldState {
        let mut w = crate::snapshot::create_initial_snapshot("t", "T", 0, None).world;
        w.placed_objects.clear();
        w.cells.clear();
        w.sims.clear();
        w.lobby_mode = LobbyMode::PerfectParity;
        w.star_count = 1;
        w
    }

    fn add_hotel_room(world: &mut WorldState, x: i64, floor: i64, unit_status: i64) -> String {
        let y = floor_to_y(floor);
        world.placed_objects.insert(
            format!("{x},{y}"),
            PlacedObjectRecord {
                left_tile_index: x,
                right_tile_index: x + 3,
                object_type_code: FAMILY_HOTEL_SINGLE,
                unit_status,
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
            },
        );
        format!("{x},{y}")
    }

    fn make_housekeeper(floor: i64, home_column: i64) -> SimRecord {
        SimRecord {
            floor_anchor: floor,
            home_column,
            base_offset: 0,
            facility_slot: 0,
            family_code: FAMILY_HOUSEKEEPING,
            state_code: HK_STATE_SEARCH,
            route: RouteState::Idle,
            selected_floor: floor,
            origin_floor: floor,
            destination_floor: -1,
            venue_return_state: 0,
            queue_tick: 0,
            elapsed_ticks: 0,
            transit_ticks_remaining: 0,
            last_demand_tick: -1,
            trip_count: 0,
            accumulated_ticks: 0,
            target_room_floor: HK_SEARCHING_SENTINEL,
            target_room_column: -1,
            spawn_floor: floor,
            post_claim_countdown: 0,
            encoded_target_floor: 0,
            commercial_venue_slot: -1,
        }
    }

    fn time_at(day_tick: i64) -> TimeState {
        TimeState {
            day_tick,
            ..create_time_state()
        }
    }

    fn room<'a>(world: &'a WorldState, key: &str) -> &'a PlacedObjectRecord {
        &world.object(key).unwrap()
    }

    #[test]
    fn stays_searching_when_no_turnover_band_hotel_rooms_exist() {
        let mut world = make_world();
        world.sims.push(make_housekeeper(12, 0));
        process_housekeeping_sim(&mut world, &time_at(100), 0);
        // Binary state-0: find_matching returns -1 → sim+6 = -1 → state = 1.
        // Observable effect: no room claim occurred and target remains sentinel.
        assert_eq!(world.sims[0].target_room_floor, HK_SEARCHING_SENTINEL);
        assert_eq!(world.sims[0].post_claim_countdown, 0);
    }

    #[test]
    fn skips_rooms_whose_floor_does_not_match_the_claimants_floor_class() {
        let mut world = make_world();
        // baseOffset=0 → floorClass = 0. Add a turnover room on floor 13 (class 1).
        let r = add_hotel_room(&mut world, 20, 13, 0x28);
        world.sims.push(make_housekeeper(12, 0));
        process_housekeeping_sim(&mut world, &time_at(100), 0);
        // No matching candidate → state transitions to HK_STATE_ROUTE_TO_CANDIDATE
        // but room is not claimed.
        assert_eq!(room(&world, &r).housekeeping_claimed_flag, 0);
        assert_eq!(world.sims[0].target_room_floor, HK_SEARCHING_SENTINEL);
    }

    #[test]
    fn claims_a_same_floor_turnover_room_immediately_before_the_cutoff() {
        let mut world = make_world();
        let r = add_hotel_room(&mut world, 20, 12, 0x28);
        world.sims.push(make_housekeeper(12, 0));
        process_housekeeping_sim(&mut world, &time_at(100), 0);

        let sim = &world.sims[0];
        assert_eq!(sim.state_code, HK_STATE_COUNTDOWN);
        assert_eq!(sim.target_room_floor, 12);
        assert_eq!(sim.post_claim_countdown, HK_POST_CLAIM_COUNTDOWN);
        assert_eq!(room(&world, &r).housekeeping_claimed_flag, 1);
        // Binary `activate_selected_vacant_unit` (1158:02e2): unit_status is 0x18
        // pre-daypart-4, 0x20 otherwise. The test's timeAt(100) is daypart 0.
        assert_eq!(room(&world, &r).unit_status, 0x18);
        assert_eq!(sim.target_room_column, 20);
    }

    #[test]
    fn refuses_to_promote_once_day_tick_has_reached_the_cutoff() {
        let mut world = make_world();
        let r = add_hotel_room(&mut world, 20, 12, 0x28);
        world.sims.push(make_housekeeper(12, 0));
        process_housekeeping_sim(&mut world, &time_at(1500), 0);
        assert_eq!(world.sims[0].state_code, HK_STATE_SEARCH);
        assert_eq!(room(&world, &r).housekeeping_claimed_flag, 0);
        assert_eq!(room(&world, &r).unit_status, 0x28);
    }

    #[test]
    fn upward_scan_reaches_the_spawn_floor_before_any_downward_candidate() {
        let mut world = make_world();
        // Both floors are class 0 (%6 == 0). Spawn-floor candidate should win
        // because the upward scan starts at spawn_floor inclusive.
        let spawn_floor_room = add_hotel_room(&mut world, 10, 12, 0x28);
        let downward_room = add_hotel_room(&mut world, 10, 6, 0x28);
        world.sims.push(make_housekeeper(12, 0));
        process_housekeeping_sim(&mut world, &time_at(50), 0);
        assert_eq!(world.sims[0].state_code, HK_STATE_COUNTDOWN);
        assert_eq!(room(&world, &spawn_floor_room).housekeeping_claimed_flag, 1);
        assert_eq!(room(&world, &downward_room).housekeeping_claimed_flag, 0);
    }

    #[test]
    fn within_a_matching_floor_picks_the_leftmost_turnover_band_slot() {
        let mut world = make_world();
        add_hotel_room(&mut world, 40, 12, 0x28);
        let leftmost = add_hotel_room(&mut world, 10, 12, 0x30);
        world.sims.push(make_housekeeper(12, 0));
        process_housekeeping_sim(&mut world, &time_at(50), 0);
        assert_eq!(world.sims[0].state_code, HK_STATE_COUNTDOWN);
        assert_eq!(room(&world, &leftmost).housekeeping_claimed_flag, 1);
    }

    #[test]
    fn post_claim_countdown_decrements_from_3_to_0_then_returns_to_search() {
        let mut world = make_world();
        let r = add_hotel_room(&mut world, 20, 12, 0x28);
        world.sims.push(make_housekeeper(12, 0));
        process_housekeeping_sim(&mut world, &time_at(50), 0);
        assert_eq!(world.sims[0].post_claim_countdown, 3);

        // Binary state-2 is check-before-decrement, so 4 strides in state 2 for
        // a starting countdown of 3 (values 3→2→1→0 then exit on the 4th).
        for i in 1..=4 {
            process_housekeeping_sim(&mut world, &time_at(50 + i), 0);
        }
        assert_eq!(world.sims[0].state_code, HK_STATE_SEARCH);
        assert_eq!(world.sims[0].post_claim_countdown, 0);
        // Flag stays set — hotel deactivation clears it, not housekeeping itself.
        assert_eq!(room(&world, &r).housekeeping_claimed_flag, 1);
    }

    #[test]
    fn arrival_handler_promotes_once_sim_reaches_the_target_room_floor() {
        let mut world = make_world();
        let r = add_hotel_room(&mut world, 20, 12, 0x28);
        let mut sim = make_housekeeper(18, 0);
        // Simulate mid-commute state: searched upward, committed floor 12, now en route.
        sim.state_code = HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT;
        sim.spawn_floor = 12;
        sim.target_room_floor = 12;
        sim.target_room_column = 20;
        sim.floor_anchor = 12; // arrival snaps floorAnchor forward in tests
        world.sims.push(sim);
        handle_housekeeping_sim_arrival(&mut world, &time_at(50), 0, 12);
        assert_eq!(world.sims[0].state_code, HK_STATE_COUNTDOWN);
        assert_eq!(room(&world, &r).housekeeping_claimed_flag, 1);
    }

    #[test]
    fn ignores_arrivals_at_unrelated_floors() {
        let mut world = make_world();
        add_hotel_room(&mut world, 20, 12, 0x28);
        let mut sim = make_housekeeper(18, 0);
        sim.state_code = HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT;
        sim.spawn_floor = 12;
        sim.target_room_floor = 12;
        world.sims.push(sim);
        handle_housekeeping_sim_arrival(&mut world, &time_at(50), 0, 30);
        assert_eq!(world.sims[0].state_code, HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT);
    }

    #[test]
    fn route_to_target_state_ignores_expired_day_tick_and_resets() {
        let mut world = make_world();
        add_hotel_room(&mut world, 20, 12, 0x28);
        let mut sim = make_housekeeper(12, 0);
        sim.state_code = HK_STATE_ROUTE_TO_TARGET;
        sim.target_room_floor = 12;
        world.sims.push(sim);
        process_housekeeping_sim(&mut world, &time_at(1600), 0);
        assert_eq!(world.sims[0].state_code, HK_STATE_SEARCH);
    }
}

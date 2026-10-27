// ts: sims/scoring.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::js;
use crate::resources::{
    FAMILY_CINEMA, FAMILY_CINEMA_LOWER, FAMILY_CINEMA_STAIRS_LOWER, FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN,
    FAMILY_OFFICE, FAMILY_PARTY_HALL, FAMILY_PARTY_HALL_LOWER, FAMILY_RESTAURANT, FAMILY_RETAIL,
    OP_SCORE_THRESHOLDS,
};
use crate::sims::population::{find_object_for_sim, find_sibling_sims, sim_key};
use crate::sims::states::{
    ENTITY_POPULATION_BY_TYPE, EVALUATABLE_FAMILIES, HOTEL_FAMILIES, UNIT_STATUS_CONDO_OCCUPIED,
    UNIT_STATUS_HOTEL_SOLD_OUT, UNIT_STATUS_OFFICE_OCCUPIED,
};
use crate::sims::trip_counters::reset_facility_sim_trip_counters;
use crate::stress::add_delay::add_delay_to_current_sim;
use crate::stress::lobby_reduction::reduce_elapsed_for_lobby_boarding;
use crate::time::{TimeState, DAY_TICK_MAX};
use crate::world::{
    y_to_floor, CarrierPendingRoute, CarrierRecord, RouteState, WorldState, GRID_HEIGHT,
};

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

const ENTERTAINMENT_NOISE_FAMILIES: &[i64] = &[
    FAMILY_CINEMA,
    FAMILY_CINEMA_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CINEMA_STAIRS_LOWER,
    FAMILY_PARTY_HALL,
    FAMILY_PARTY_HALL_LOWER,
];

// Binary compute_object_operational_score (1138:040f) divisor per family. For
// hotels the binary loops bo=1..N (skipping bo=0) and divides by the loop
// count: single=1, twin=2, suite=2. Office and condo do a full sweep.
const OPERATIONAL_SCORE_DIVISOR: &[(i64, i64)] = &[
    (FAMILY_HOTEL_SINGLE, 1),
    (FAMILY_HOTEL_TWIN, 2),
    (FAMILY_HOTEL_SUITE, 2),
    (FAMILY_OFFICE, 6),
    (FAMILY_CONDO, 3),
];

/// `SimStateRecord.currentTripStressLevel` and `.stressLevel` (upstream's
/// inline `"low" | "medium" | "high"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StressLevel {
    Low,
    Medium,
    High,
}

/// The fields are in the order createSimStateRecord's object literal writes
/// them (which is the order JSON shows), not the interface's order, where
/// `destinationFloor` comes third.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimStateRecord {
    pub id: String,
    pub floor_anchor: i64,
    pub selected_floor: i64,
    pub home_column: i64,
    pub base_offset: i64,
    pub family_code: i64,
    pub state_code: i64,
    pub route_mode: i64,
    pub destination_floor: i64,
    /// `number | null`: written as null, never left out.
    pub carrier_id: Option<i64>,
    pub assigned_car_index: i64,
    pub boarded_on_carrier: bool,
    pub current_trip_stress_ticks: i64,
    pub current_trip_stress_level: StressLevel,
    pub average_trip_stress_ticks: i64,
    pub stress_level: StressLevel,
    pub trip_count: i64,
    pub accumulated_ticks: i64,
    pub elapsed_ticks: i64,
}

fn is_noise_source(origin_family_code: i64, target_family_code: i64) -> bool {
    if ENTERTAINMENT_NOISE_FAMILIES.contains(&target_family_code) {
        return EVALUATABLE_FAMILIES.contains(&origin_family_code);
    }

    if HOTEL_FAMILIES.contains(&origin_family_code) {
        return target_family_code == FAMILY_RESTAURANT
            || target_family_code == FAMILY_OFFICE
            || target_family_code == FAMILY_FAST_FOOD
            || target_family_code == FAMILY_RETAIL;
    }

    if origin_family_code == FAMILY_OFFICE {
        return target_family_code == FAMILY_RESTAURANT
            || target_family_code == FAMILY_FAST_FOOD
            || target_family_code == FAMILY_RETAIL;
    }

    if origin_family_code == FAMILY_CONDO {
        return target_family_code == FAMILY_HOTEL_SINGLE
            || target_family_code == FAMILY_HOTEL_TWIN
            || target_family_code == FAMILY_HOTEL_SUITE
            || target_family_code == FAMILY_RESTAURANT
            || target_family_code == FAMILY_OFFICE
            || target_family_code == FAMILY_FAST_FOOD
            || target_family_code == FAMILY_RETAIL;
    }

    target_family_code == FAMILY_RESTAURANT
        || target_family_code == FAMILY_FAST_FOOD
        || target_family_code == FAMILY_RETAIL
}

/// Upstream's `candidate === object` is the key comparison here.
pub(crate) fn has_nearby_noise(world: &WorldState, object_key: &str, floor_anchor: i64, radius: i64) -> bool {
    let object = &world.object(object_key).unwrap();
    for (key, candidate) in world.placed_objects.iter() {
        if key == object_key {
            continue;
        }
        let (_x, y) = key_xy(key);
        if y_to_floor(y) != floor_anchor {
            continue;
        }
        if !is_noise_source(object.object_type_code, candidate.object_type_code) {
            continue;
        }
        let left_delta = (candidate.left_tile_index - object.right_tile_index).abs();
        let right_delta = (object.left_tile_index - candidate.right_tile_index).abs();
        if left_delta.min(right_delta) <= radius {
            return true;
        }
    }

    false
}

pub fn recompute_object_operational_status(world: &mut WorldState, sim_index: i64, object_key: &str) {
    let object_type_code = world.object(object_key).unwrap().object_type_code;
    if !EVALUATABLE_FAMILIES.contains(&object_type_code) {
        return;
    }

    // Always compute the raw average stress so the debug badge stays useful
    // for sold-out hotels and other states the binary skips for evalLevel.
    // Binary compute_object_operational_score (1138:040f) sums stress over the
    // non-primary occupants only (loop starts at occupant index 1) and divides
    // by the loop iteration count: single (3) → 1 non-primary occupant, twin/
    // suite (4/5) → 2, office (7) → 6 (full sweep), condo (9) → 3 (full sweep).
    // ENTITY_POPULATION_BY_TYPE counts ALL occupants including bo=0, so it
    // overestimates the divisor for hotels by 1 — using it would lower scores
    // and force eval=1/2 → midday-reset more rooms than the binary does.
    let siblings = find_sibling_sims(world, sim_index);
    let population_count = js::record_get(OPERATIONAL_SCORE_DIVISOR, object_type_code)
        .or_else(|| js::record_get(ENTITY_POPULATION_BY_TYPE, object_type_code))
        .copied()
        .unwrap_or(1);
    let mut stress_sum = 0;
    let mut has_trip_data = false;
    for &sibling_index in &siblings {
        let sibling = &world.sims[sibling_index as usize];
        if sibling.base_offset == 0 && HOTEL_FAMILIES.contains(&object_type_code) {
            // Binary skips bo=0 in the hotel loop (occupant index starts at 1).
            continue;
        }
        if sibling.trip_count > 0 {
            stress_sum += js::trunc_div(sibling.accumulated_ticks, sibling.trip_count);
            has_trip_data = true;
        }
    }
    let raw_average_stress = if has_trip_data {
        js::trunc_div(stress_sum, population_count)
    } else {
        -1
    };
    let object = world.object_mut(object_key).unwrap();
    object.eval_score = raw_average_stress;

    if HOTEL_FAMILIES.contains(&object.object_type_code) && object.unit_status > UNIT_STATUS_HOTEL_SOLD_OUT {
        object.eval_level = 0xff;
        return;
    }
    if object.object_type_code == FAMILY_OFFICE
        && object.unit_status > UNIT_STATUS_OFFICE_OCCUPIED
        && object.occupied_flag != 0
    {
        object.eval_level = 0xff;
        return;
    }
    if object.object_type_code == FAMILY_CONDO
        && object.unit_status > UNIT_STATUS_CONDO_OCCUPIED
        && object.occupied_flag != 0
    {
        object.eval_level = 0xff;
        return;
    }

    if raw_average_stress < 0 {
        object.eval_level = 0xff;
        return;
    }

    let mut score = if has_trip_data { raw_average_stress } else { 0 };
    match object.rent_level {
        0 => score += 30,
        2 => score = 0.max(score - 30),
        3 => score = 0,
        _ => {}
    }

    let noise_radius = if object.object_type_code == FAMILY_OFFICE {
        10
    } else if object.object_type_code == FAMILY_CONDO {
        30
    } else {
        20
    };
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    if has_nearby_noise(world, object_key, floor_anchor, noise_radius) {
        score += 60;
    }
    // BOX: amenities -- what the tower's amenities do for the unit (0
    // with BOX RULES off).
    score = 0.max(score - crate::box_ext::amenities::relief(world, object_key));

    let [lower, upper] = js::record_get(OP_SCORE_THRESHOLDS, world.star_count.min(5))
        .copied()
        .unwrap_or([80, 200]);
    let object = world.object_mut(object_key).unwrap();
    object.eval_level = if score < lower {
        2
    } else if score < upper {
        1
    } else {
        0
    };
    // Binary recompute_object_operational_status tail (1138:093c CMP +0x14,
    // 1138:09d6 MOV byte ptr ES:[BX+0x14],1):
    //   if (occupiedFlag == 0 && evalLevel != 0) occupiedFlag = 1
    // Hotels (families 3/4/5) gate the set on unitStatus <= 0x27 — above that
    // band the room must stay unoccupied until the dormant phase clears.
    // No call to refresh_occupied_flag_and_trip_counters here for any family.
    if object.occupied_flag == 0 && object.eval_level > 0 {
        if !HOTEL_FAMILIES.contains(&object.object_type_code) || object.unit_status <= 0x27 {
            object.occupied_flag = 1;
        }
    }
}

pub fn recompute_all_object_operational_status(world: &mut WorldState) {
    // Object.entries: no object is added or removed in the loop, so a list of
    // keys walked in order sees what upstream sees.
    for key in world.placed_objects.key_list() {
        let object_type_code = world.object(key.as_str()).unwrap().object_type_code;
        if !EVALUATABLE_FAMILIES.contains(&object_type_code) {
            continue;
        }
        let (x, y) = key_xy(&key);
        let floor_anchor = y_to_floor(y);
        let sim = world.sims.iter().position(|candidate| {
            candidate.family_code == object_type_code
                && candidate.floor_anchor == floor_anchor
                && candidate.home_column == x
        });
        let Some(sim) = sim else {
            continue;
        };
        recompute_object_operational_status(world, sim as i64, &key);
    }
}

/// Upstream returns `{ medium, high }`; here the pair `(medium, high)`.
pub fn active_stress_thresholds(world: &WorldState) -> (i64, i64) {
    let [medium, high] = js::record_get(OP_SCORE_THRESHOLDS, world.star_count.min(5))
        .copied()
        .unwrap_or([80, 200]);
    (medium, high)
}

pub fn average_trip_stress_ticks(sim: &crate::world::SimRecord) -> i64 {
    if sim.trip_count > 0 {
        js::trunc_div(sim.accumulated_ticks, sim.trip_count)
    } else {
        0
    }
}

pub fn current_trip_stress_ticks(world: &WorldState, time: &TimeState, sim_index: i64) -> i64 {
    let sim = &world.sims[sim_index as usize];
    let mut live_elapsed = 0;
    if sim.last_demand_tick >= 0 {
        live_elapsed = (time.day_tick - sim.last_demand_tick + DAY_TICK_MAX) % DAY_TICK_MAX;
        // Upstream passes a fresh `{ elapsedTicks: liveElapsed }` object.
        let mut reduced = live_elapsed;
        reduce_elapsed_for_lobby_boarding(&mut reduced, sim.origin_floor, world);
        live_elapsed = reduced;
    }
    300.min(sim.elapsed_ticks + live_elapsed)
}

pub fn current_trip_stress_level(world: &WorldState, time: &TimeState, sim_index: i64) -> StressLevel {
    let elapsed = current_trip_stress_ticks(world, time, sim_index);
    let (medium, high) = active_stress_thresholds(world);
    if elapsed >= high {
        return StressLevel::High;
    }
    if elapsed >= medium {
        return StressLevel::Medium;
    }
    StressLevel::Low
}

pub fn refresh_occupied_flag_and_trip_counters(world: &mut WorldState, sim_index: i64, object_key: &str) {
    // Binary refresh_occupied_flag_and_trip_counters @ 1138:0f79:
    //   Branch A: evalLevel >= 1 (not 0xff) → occupiedFlag(+0x14)=1, reset trip counters
    //   Branch B: evalLevel == 0 with A-rated sibling on same floor+family
    //             → downgrade both to 1, occupiedFlag=1 both, reset trip counters
    //   Branch C: evalLevel == 0 with no donor → occupiedFlag=0, NO reset
    // 1138:1072: MOV byte ptr ES:[BX+0x14],1; 1138:10ab: MOV byte ptr ES:[BX+0x14],0
    let object = &world.object(object_key).unwrap();
    if object.eval_level != 0xff && object.eval_level >= 1 {
        world.object_mut(object_key).unwrap().occupied_flag = 1;
        reset_facility_sim_trip_counters(world, sim_index);
        return;
    }
    if object.eval_level == 0 {
        let object_type_code = object.object_type_code;
        let y = GRID_HEIGHT - 1 - world.sims[sim_index as usize].floor_anchor;
        let donor = world
            .placed_objects
            .iter()
            .find(|(key, candidate)| {
                // `candidate === object` is the key comparison here.
                *key != object_key
                    && candidate.object_type_code == object_type_code
                    && key_xy(key).1 == y
                    && candidate.eval_level == 2
            })
            .map(|(key, _)| key.to_string());
        if let Some(donor) = donor {
            let candidate = world.object_mut(&donor).unwrap();
            candidate.eval_level = 1;
            candidate.occupied_flag = 1;
            let object = world.object_mut(object_key).unwrap();
            object.eval_level = 1;
            object.occupied_flag = 1;
            reset_facility_sim_trip_counters(world, sim_index);
            return;
        }
        world.object_mut(object_key).unwrap().occupied_flag = 0;
    }
}

fn sim_stress_level(
    world: &WorldState,
    time: &TimeState,
    sim_index: i64,
    _object_key: Option<&str>,
) -> StressLevel {
    current_trip_stress_level(world, time, sim_index)
}

fn distance_feedback_penalty(route_height_metric: i64, target_height_metric: i64) -> i64 {
    let delta = (route_height_metric - target_height_metric).abs();
    if delta >= 125 {
        return 60;
    }
    if delta > 79 {
        return 30;
    }
    0
}

pub fn maybe_apply_distance_feedback(
    world: &mut WorldState,
    sim_index: i64,
    route_height_metric: i64,
    target_height_metric: i64,
    can_apply_for_route_kind: bool,
) {
    if !can_apply_for_route_kind {
        return;
    }
    // BOX: walking distance -- under BOX RULES every cell walked to the lift
    // or stairs beyond the first few is a tick (upstream: nothing below 80
    // cells, then 30, then 60 beyond 125)
    let penalty = if crate::box_ext::box_rules(world) {
        ((route_height_metric - target_height_metric).abs() - crate::box_ext::FREE_WALK_CELLS).max(0)
    } else {
        distance_feedback_penalty(route_height_metric, target_height_metric)
    };
    if penalty == 0 {
        return;
    }
    add_delay_to_current_sim(&mut world.sims[sim_index as usize], penalty);
}

/// Upstream's `Map<string, { carrier, route }>`: the pair `(carrier, route)`.
pub type PendingBySimId<'a> = IndexMap<String, (&'a CarrierRecord, &'a CarrierPendingRoute)>;

pub fn create_sim_state_record(
    world: &WorldState,
    time: &TimeState,
    sim_index: i64,
    pending_by_sim_id: &PendingBySimId<'_>,
) -> Option<SimStateRecord> {
    let object = find_object_for_sim(world, sim_index);
    let object = object?;
    let sim = &world.sims[sim_index as usize];
    let id = sim_key(sim);
    let pending = pending_by_sim_id.get(&id);
    let pending_route = pending.map(|(_, route)| *route);
    let route_carrier_id = match sim.route {
        RouteState::Carrier { carrier_id, .. } => Some(carrier_id),
        _ => None,
    };
    let carrier_id = if pending_route.is_some() || route_carrier_id.is_some() {
        pending.map(|(carrier, _)| carrier.carrier_id).or(route_carrier_id)
    } else {
        None
    };
    let route_mode_num = match sim.route {
        RouteState::Carrier { .. } => 2,
        RouteState::Segment { .. } => 1,
        RouteState::Idle => 0,
    };
    let current_ticks = current_trip_stress_ticks(world, time, sim_index);
    let current_level = sim_stress_level(world, time, sim_index, Some(&object));
    Some(SimStateRecord {
        id,
        floor_anchor: sim.floor_anchor,
        selected_floor: sim.selected_floor,
        home_column: sim.home_column,
        base_offset: sim.base_offset,
        family_code: sim.family_code,
        state_code: sim.state_code,
        route_mode: route_mode_num,
        destination_floor: sim.destination_floor,
        carrier_id,
        assigned_car_index: pending_route.map(|r| r.assigned_car_index).unwrap_or(-1),
        boarded_on_carrier: pending_route.map(|r| r.boarded).unwrap_or(false),
        current_trip_stress_ticks: current_ticks,
        current_trip_stress_level: current_level,
        average_trip_stress_ticks: average_trip_stress_ticks(sim),
        stress_level: current_level,
        trip_count: sim.trip_count,
        accumulated_ticks: sim.accumulated_ticks,
        elapsed_ticks: sim.elapsed_ticks,
    })
}

pub fn create_sim_state_records(world: &WorldState, time: &TimeState) -> Vec<SimStateRecord> {
    // Pre-index pending routes by simId to avoid O(sims × carriers × routes).
    let mut pending_by_sim_id: PendingBySimId<'_> = IndexMap::new();
    for carrier in &world.carriers {
        for route in &carrier.pending_routes {
            pending_by_sim_id.insert(route.sim_id.clone(), (carrier, route));
        }
    }

    let mut result: Vec<SimStateRecord> = Vec::new();
    for sim_index in 0..world.sims.len() as i64 {
        let record = create_sim_state_record(world, time, sim_index, &pending_by_sim_id);
        if let Some(record) = record {
            result.push(record);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    // Upstream's scoring.test.ts, the first case (the second runs the day
    // scheduler, which is another file's), and a scored case.
    use super::*;
    use crate::sims::states::STATE_MORNING_GATE;
    use crate::world::{floor_to_y, PlacedObjectRecord, SimRecord};

    fn make_world() -> WorldState {
        let mut w = crate::snapshot::create_initial_snapshot("t", "T", 0, None).world;
        w.placed_objects.clear();
        w.sims.clear();
        w.star_count = 1;
        w
    }

    fn add_office(world: &mut WorldState, x: i64, floor: i64) -> String {
        let y = floor_to_y(floor);
        let object = PlacedObjectRecord {
            left_tile_index: x,
            right_tile_index: x + 5,
            object_type_code: FAMILY_OFFICE,
            unit_status: 0,
            aux_value_or_timer: 1,
            linked_record_index: -1,
            dirty_flag: 1,
            occupied_flag: 1,
            eval_level: 0xff,
            eval_score: -1,
            rent_level: 1,
            activation_tick_count: 0,
            housekeeping_claimed_flag: 0,
            vip_flag: None,
        };
        world.cells.insert(format!("{x},{y}"), "office".to_string());
        world.placed_objects.insert(format!("{x},{y}"), object);
        format!("{x},{y}")
    }

    fn add_office_sim(world: &mut WorldState, x: i64, floor: i64, trip_count: i64, accumulated_ticks: i64) {
        world.sims.push(SimRecord {
            floor_anchor: floor,
            home_column: x,
            base_offset: 0,
            facility_slot: 0,
            family_code: FAMILY_OFFICE,
            state_code: STATE_MORNING_GATE,
            route: RouteState::Idle,
            selected_floor: floor,
            origin_floor: floor,
            destination_floor: -1,
            venue_return_state: 0,
            queue_tick: 0,
            elapsed_ticks: 0,
            transit_ticks_remaining: 0,
            last_demand_tick: -1,
            trip_count,
            accumulated_ticks,
            target_room_floor: -1,
            target_room_column: -1,
            spawn_floor: floor,
            post_claim_countdown: 0,
            encoded_target_floor: 0,
            commercial_venue_slot: -1,
        });
    }

    #[test]
    fn keeps_occupied_offices_unscored_until_they_have_trip_data() {
        let mut world = make_world();
        let office = add_office(&mut world, 10, 1);
        add_office_sim(&mut world, 10, 1, 0, 0);

        recompute_all_object_operational_status(&mut world);

        assert_eq!(world.object(office.as_str()).unwrap().eval_score, -1);
        assert_eq!(world.object(office.as_str()).unwrap().eval_level, 0xff);
    }

    #[test]
    fn scores_from_trip_data_and_noise() {
        // What the second upstream case expects of the sweep itself: one
        // trip of 20 ticks over a divisor of 6 is 3, below 80: level 2.
        let mut world = make_world();
        let office = add_office(&mut world, 10, 1);
        add_office_sim(&mut world, 10, 1, 1, 20);
        recompute_all_object_operational_status(&mut world);
        assert_eq!(world.object(office.as_str()).unwrap().eval_score, 3);
        assert_eq!(world.object(office.as_str()).unwrap().eval_level, 2);

        // A fast-food beside it adds 60 noise; rent level 0 adds 30: 93 → 1.
        let mut ff = world.object(office.as_str()).unwrap().clone();
        ff.object_type_code = FAMILY_FAST_FOOD;
        ff.left_tile_index = 20;
        ff.right_tile_index = 35;
        world.placed_objects.insert(format!("20,{}", floor_to_y(1)), ff);
        world.object_mut(&office).unwrap().rent_level = 0;
        recompute_all_object_operational_status(&mut world);
        assert_eq!(world.object(office.as_str()).unwrap().eval_level, 1);
    }

    #[test]
    fn stress_record_serialises_in_upstream_order() {
        let mut world = make_world();
        add_office(&mut world, 10, 1);
        add_office_sim(&mut world, 10, 1, 2, 50);
        let mut time = crate::time::create_time_state();
        time.day_tick = 100;
        let records = create_sim_state_records(&world, &time);
        assert_eq!(records.len(), 1);
        let json = serde_json::to_string(&records[0]).unwrap();
        assert!(json.starts_with(r#"{"id":"1:10:7:0","floorAnchor":1,"selectedFloor":1,"homeColumn":10"#));
        assert!(json.contains(r#""destinationFloor":-1,"carrierId":null,"assignedCarIndex":-1"#));
        assert!(json.contains(r#""currentTripStressLevel":"low","averageTripStressTicks":25"#));
    }

    #[test]
    fn distance_feedback_bands() {
        assert_eq!(distance_feedback_penalty(0, 79), 0);
        assert_eq!(distance_feedback_penalty(80, 0), 30);
        assert_eq!(distance_feedback_penalty(0, 125), 60);
    }
}

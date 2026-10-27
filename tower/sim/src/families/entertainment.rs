// ts: families/entertainment.ts

// 1228:5231 gate_entertainment_guest_state (family 18/29)
// 1228:53ad dispatch_entertainment_guest_state (family 18/29)
//
// Entertainment guest state machine shared by Movie Theater (family 18 /
// FAMILY_CINEMA) and Party Hall (family 29 / FAMILY_PARTY_HALL) visitors.
//
// Binary jump table at cs:0x539d (gate, 4 entries) maps stateCode values
// {0x01, 0x05, 0x20, 0x22} to two inner handlers:
//   0x537f → unconditional call to dispatch_entertainment_guest_state
//   0x530d → daypart+RNG gate then dispatch_entertainment_guest_state
//
// Binary jump table at cs:0x5b3a (dispatch, 8 entries) maps {0x01, 0x05,
// 0x20, 0x22, 0x41, 0x45, 0x60, 0x62} to four helper functions:
//   0x01/0x41 → handle_entertainment_service_acquisition  (1228:57e2)
//   0x05/0x45 → handle_entertainment_linked_half_routing  (1228:5746)
//   0x20/0x60 → handle_entertainment_phase_consumption    (1228:54b8)
//   0x22/0x62 → handle_entertainment_service_release_return (1228:5a23)
//
// Binary selectors:
//   sim[+5]  = stateCode
//   sim[+6]  = selectedFloor (venue sub-selector byte; 0xb0 = no venue chosen)
//   sim[+7]  = originFloor   (return-to floor written at route time)
//
// (Throughout, `sim` is `world.sims[sim_index]`, and an
// EntertainmentLinkRecord that upstream holds as an object is held as its
// index in `world.sidecars`.)

use indexmap::IndexMap;

use super::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::js;
use crate::ledger::LedgerState;
use crate::queue::resolve::{resolve_sim_route_between_floors, ResolveSimRouteOptions};
use crate::resources::{
    FAMILY_CINEMA, FAMILY_CINEMA_LOWER, FAMILY_CINEMA_STAIRS_LOWER, FAMILY_CINEMA_STAIRS_UPPER, FAMILY_FAST_FOOD,
    FAMILY_PARTY_HALL, FAMILY_PARTY_HALL_LOWER, FAMILY_RESTAURANT, FAMILY_RETAIL,
};
use crate::sim_access::state_bits::is_sim_in_transit;
use crate::sims::states::LOBBY_FLOOR;
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::time::TimeState;
use crate::world::{
    create_commercial_venue_buckets, sample_rng, y_to_floor, EntertainmentLinkRecord, RouteState, SidecarRecord,
    WorldState, COMMERCIAL_VENUE_BUCKET_ROWS,
};

// Family codes emitted by cinema / party hall placement. Cinema is split
// into 4 sub-records (2 per floor: stairway + theater); party hall into 2
// (one per floor). The state machine below treats any sub-record's sim as a
// guest of the same venue.
const ENTERTAINMENT_GUEST_FAMILIES: &[i64] = &[
    FAMILY_CINEMA,
    FAMILY_CINEMA_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CINEMA_STAIRS_LOWER,
    FAMILY_PARTY_HALL,
    FAMILY_PARTY_HALL_LOWER,
];

// Entertainment state codes (match binary sim[+5] values).
const ENT_STATE_SERVICE_ACQUIRE: i64 = 0x01; // select venue and route to it
const ENT_STATE_LINKED_HALF: i64 = 0x05; // route to lobby (linked-half transition)
const ENT_STATE_PHASE_CONSUME: i64 = 0x20; // consume phase budget; route to venue
const ENT_STATE_VENUE_DWELL: i64 = 0x22; // at venue; release slot and return
const ENT_STATE_SERVICE_ACQUIRE_TRANSIT: i64 = 0x41; // 0x01 + in-transit bit
const ENT_STATE_LINKED_HALF_TRANSIT: i64 = 0x45; // 0x05 + in-transit bit
const ENT_STATE_PHASE_CONSUME_TRANSIT: i64 = 0x60; // 0x20 + in-transit bit
const ENT_STATE_VENUE_DWELL_TRANSIT: i64 = 0x62; // 0x22 + in-transit bit
const ENT_STATE_PARKED: i64 = 0x27; // parked / idle

// Daypart+RNG gate constants (handler at 0x530d, shared with recycling dispatch).
const ENT_GATE_DAY_TICK_MIN: i64 = 0xf0; // dayTick must be > this for state-0x20 to fire
const ENT_GATE_RNG_MODULO: i64 = 6; // 1-in-6 RNG gate

// Venue selector bucket RNG divisor (binary: select_random_commercial_venue_record).
const ENT_VENUE_BUCKET_MODULO: i64 = 3; // 0=retail, 1=restaurant, 2=fast-food

// Sentinel stored in sim.commercialVenueSlot / sim.originFloor on route failure.
const ENT_ROUTE_FAIL_VENUE: i64 = 0xff;

/// The y of a "x,y" key (`key.split(",").map(Number)` and its second element).
fn key_y(key: &str) -> i64 {
    key.split(',')
        .nth(1)
        .and_then(|y| y.parse().ok())
        .unwrap_or_else(|| panic!("cell key {key:?} is not \"x,y\""))
}

/// The link record at `world.sidecars[link]`, which find_entertainment_link
/// found.
fn link_mut(world: &mut WorldState, link: usize) -> &mut EntertainmentLinkRecord {
    match &mut world.sidecars[link] {
        SidecarRecord::EntertainmentLink(record) => record,
        _ => unreachable!("sidecar {link} is not an entertainment link"),
    }
}

/// Find the EntertainmentLinkRecord for a sim (keyed by ownerSubtypeIndex = homeColumn).
///
/// (Returns the record's index in `world.sidecars`.)
fn find_entertainment_link(world: &WorldState, sim_index: i64) -> Option<usize> {
    let home_column = world.sims[sim_index as usize].home_column;
    for (index, sidecar) in world.sidecars.iter().enumerate() {
        if let SidecarRecord::EntertainmentLink(link) = sidecar {
            if link.owner_subtype_index == home_column || link.owner_subtype_index + 7 == home_column {
                return Some(index);
            }
        }
    }
    None
}

fn entertainment_target_height_metric(world: &WorldState, link: Option<usize>, sim_index: i64) -> i64 {
    match link {
        Some(link) => match &world.sidecars[link] {
            SidecarRecord::EntertainmentLink(record) => record.owner_subtype_index,
            _ => unreachable!(),
        },
        None => world.sims[sim_index as usize].home_column,
    }
}

/// Binary `try_consume_entertainment_phase_budget` @ 1188:0ce9.
///
/// Selects which budget byte to consume by entity placed-object type:
///   - 0x12 (cinema upper) | 0x22 (cinema upper stair) | 0x1d (party hall upper)
///       → upper budget (offset 4)
///   - all other types (0x13, 0x23, 0x1e) → lower budget (offset 5)
///
/// Returns false if the selected budget byte is 0; otherwise decrements it
/// by 1 and returns true. Does NOT touch `attendance_counter` (that is the
/// job of `increment_entertainment_link_runtime_counters` on arrival).
fn entertainment_budget_uses_upper_byte(entity_type: i64) -> bool {
    entity_type == FAMILY_CINEMA || entity_type == FAMILY_CINEMA_STAIRS_UPPER || entity_type == FAMILY_PARTY_HALL
}

fn try_consume_entertainment_phase_budget(link: &mut EntertainmentLinkRecord, entity_type: i64) -> bool {
    if entertainment_budget_uses_upper_byte(entity_type) {
        if link.upper_budget == 0 {
            return false;
        }
        link.upper_budget -= 1;
    } else {
        if link.lower_budget == 0 {
            return false;
        }
        link.lower_budget -= 1;
    }
    true
}

/// Binary `increment_entertainment_half_phase` @ 1188 (refund path).
/// Called on a -1 route failure inside `handle_entertainment_phase_consumption`
/// to give back the unit just consumed by `try_consume_entertainment_phase_budget`.
/// Increments the same budget byte (saturating at 0xff), NOT `link_phase_state`.
fn increment_entertainment_half_phase(link: &mut EntertainmentLinkRecord, entity_type: i64) {
    if entertainment_budget_uses_upper_byte(entity_type) {
        link.upper_budget = 0xff.min(link.upper_budget + 1);
    } else {
        link.lower_budget = 0xff.min(link.lower_budget + 1);
    }
}

/// Binary `increment_entertainment_link_runtime_counters`: called on a route
/// result 3 (arrival) during phase consumption.
///
///   - increments `active_runtime_count` (currently-present attendees)
///   - increments `attendance_counter` (total arrivals this cycle)
///   - if `link_phase_state == 1`, promotes it to 2 (first arrival)
fn increment_entertainment_link_runtime_counters(link: &mut EntertainmentLinkRecord) {
    link.active_runtime_count = 0xff.min(link.active_runtime_count + 1);
    link.attendance_counter = 0xff.min(link.attendance_counter + 1);
    if link.link_phase_state == 1 {
        link.link_phase_state = 2;
    }
}

/// Binary `get_entertainment_link_venue_floor` (1188:0d98). Returns the
/// destination venue floor for phase-consumption arrivals. Binary picks
/// `upperHalfFloor` for cinema (`kindOrMovieId>=0`) and `lowerHalfFloor`
/// for party hall (`kindOrMovieId<0`). In TS we keep using `sim.floorAnchor`
/// here — for cinema-upper / party-hall guests this resolves to the same
/// floor the binary picks, and the trace fixtures rely on that mapping for
/// lower-half guests being routed back to their own home half rather than
/// their sibling's.
fn get_entertainment_link_venue_floor(world: &WorldState, _link: Option<usize>, sim_index: i64) -> i64 {
    world.sims[sim_index as usize].floor_anchor
}

/// Binary `get_entertainment_link_routing_source_floor` (1188:0dce). Returns
/// `link.lowerHalfFloor` — but `allocate_entertainment_link_record` stores
/// the raw `param_3` byte the caller passed for that field. For both the
/// cinema and party-hall placements observed in fixtures the caller passes
/// 0xff (sentinel: lower half not registered), so the helper sign-extends
/// to -1 and `resolve_sim_route_between_floors` then clamps it to
/// LOBBY_FLOOR. Falls back to `sim.floorAnchor` when no link is bound.
fn get_entertainment_link_routing_source_floor(world: &WorldState, link: Option<usize>, sim_index: i64) -> i64 {
    if link.is_none() {
        return world.sims[sim_index as usize].floor_anchor;
    }
    -1
}

fn bucket_rows_for_family(world: &WorldState, service_bucket: i64) -> Option<&Vec<Vec<i64>>> {
    if service_bucket == 0 {
        return Some(&world.commercial_venue_buckets.retail);
    }
    if service_bucket == 1 {
        return Some(&world.commercial_venue_buckets.restaurant);
    }
    if service_bucket == 2 {
        return Some(&world.commercial_venue_buckets.fast_food);
    }
    None
}

/// Binary `select_random_commercial_venue_record_from_bucket` (11b0:1361):
/// indexes the maintained per-family zone-row bucket, falls back to row 0 when
/// the target row is empty, and picks uniformly via `abs(sample_lcg15()) %
/// count`. Critically the LCG is sampled BEFORE the post-pick availability
/// check (LAB_11b0_14c1): a dormant or closed pick still burns RNG and returns
/// -1.
fn select_random_commercial_venue_record_from_bucket(world: &mut WorldState, service_bucket: i64, zone_row: i64) -> i64 {
    let Some(rows) = bucket_rows_for_family(world, service_bucket) else {
        return -1;
    };
    // (`let row = rows[zoneRow]`, falling back to `rows[0]`: the row's index.)
    let nonempty = |r: i64| r >= 0 && rows.get(r as usize).is_some_and(|row| !row.is_empty());
    let row = if nonempty(zone_row) {
        zone_row as usize
    } else if nonempty(0) {
        0
    } else {
        return -1;
    };
    let row_len = rows[row].len() as i64;
    let pick = sample_rng(world).abs() % row_len;
    let record_idx = bucket_rows_for_family(world, service_bucket).unwrap()[row][pick as usize];
    // (`world.sidecars[recordIdx] as CommercialVenueRecord | undefined`: a
    // record of another kind has no `availabilityState`, which is then
    // neither 0xff nor 3.)
    let sidecar = if record_idx >= 0 { world.sidecars.get(record_idx as usize) } else { None };
    let Some(sidecar) = sidecar else {
        return -1;
    };
    if let SidecarRecord::CommercialVenue(venue) = sidecar {
        if venue.availability_state == 0xff || venue.availability_state == 3 {
            return -1;
        }
    }
    record_idx
}

/// Binary `classify_path_bucket_index` (11b0:16f0): maps a floor index in
/// 5..104 to one of seven bucket rows; floors outside the 10-floor strip per
/// 15-floor zone return -1 (the venue is not appended to any bucket row).
fn classify_path_bucket_index(floor: i64) -> i64 {
    let offset = floor - 5;
    let row = js::trunc_div(offset, 15);
    if row < 0 || row > 6 {
        return -1;
    }
    if offset % 15 > 9 {
        return -1;
    }
    row
}

fn append_buckets_for_families(world: &mut WorldState, families: &[i64]) {
    let mut sidecar_to_floor: IndexMap<i64, i64> = IndexMap::new();
    let mut sidecar_to_family: IndexMap<i64, i64> = IndexMap::new();
    for (key, object) in world.placed_objects.iter() {
        if object.linked_record_index < 0 {
            continue;
        }
        let family = object.object_type_code;
        if !families.contains(&family) {
            continue;
        }
        let y = key_y(key);
        sidecar_to_floor.insert(object.linked_record_index, y_to_floor(y));
        sidecar_to_family.insert(object.linked_record_index, family);
    }
    for idx in 0..world.sidecars.len() {
        let SidecarRecord::CommercialVenue(sidecar) = &world.sidecars[idx] else {
            continue;
        };
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        let floor = sidecar_to_floor.get(&(idx as i64));
        let family = sidecar_to_family.get(&(idx as i64));
        let (Some(&floor), Some(&family)) = (floor, family) else {
            continue;
        };
        let row = classify_path_bucket_index(floor);
        if row < 0 || row >= COMMERCIAL_VENUE_BUCKET_ROWS {
            continue;
        }
        let target: Option<&mut Vec<Vec<i64>>> = if family == FAMILY_RETAIL {
            Some(&mut world.commercial_venue_buckets.retail)
        } else if family == FAMILY_RESTAURANT {
            Some(&mut world.commercial_venue_buckets.restaurant)
        } else if family == FAMILY_FAST_FOOD {
            Some(&mut world.commercial_venue_buckets.fast_food)
        } else {
            None
        };
        if let Some(target) = target {
            target[row as usize].push(idx as i64);
        }
    }
}

/// Binary `rebuild_linked_facility_records` (11b0:0184) bucket-rebuild path.
/// Clears all three family buckets via FUN_11b0_154e, then iterates the
/// facility record table appending entries for every family EXCEPT 6 (restaurant).
/// The restaurant bucket therefore stays empty between tick 240 and tick 1600;
/// it is re-populated separately by `rebuildRestaurantBuckets`. Used by
/// `runGlobalRebuilds` (placement/demolition) and `rebuildCommercialVenueRuntime`
/// (tick 240).
pub fn recompute_commercial_venue_buckets(world: &mut WorldState) {
    world.commercial_venue_buckets = create_commercial_venue_buckets();
    append_buckets_for_families(world, &[FAMILY_RETAIL, FAMILY_FAST_FOOD]);
}

/// Binary `rebuild_type6_facility_records` (11b0:0250). Re-appends restaurant
/// (family 6) entries into `world.commercialVenueBuckets.restaurant`. Called
/// at the tick 1600 restaurant restock checkpoint. Does NOT touch the other
/// two family buckets. Note the binary leaves stale restaurant entries in
/// place: the restaurant bucket was cleared at tick 240 and stays empty until
/// this point, after which it holds the current placement set.
pub fn recompute_restaurant_venue_buckets(world: &mut WorldState) {
    for row in world.commercial_venue_buckets.restaurant.iter_mut() {
        row.clear();
    }
    append_buckets_for_families(world, &[FAMILY_RESTAURANT]);
}

/// Binary `select_random_commercial_venue_record_for_floor` (11b0:151d):
/// computes `zone_row = max(0, (floor_index - 9) / 15)` and delegates.
fn select_random_commercial_venue_record_for_floor(world: &mut WorldState, service_bucket: i64, floor_index: i64) -> i64 {
    let zone_row = 0.max(js::trunc_div(floor_index - 9, 15));
    select_random_commercial_venue_record_from_bucket(world, service_bucket, zone_row)
}

/// Binary `get_current_commercial_venue_destination_floor` (11b0:10fe):
/// returns lobby (10) when the slot is < 0, else the slot's owner_floor.
fn get_current_commercial_venue_destination_floor(world: &WorldState, sim_index: i64) -> i64 {
    let slot = world.sims[sim_index as usize].commercial_venue_slot;
    if slot < 0 {
        return LOBBY_FLOOR;
    }
    for (key, object) in world.placed_objects.iter() {
        if object.linked_record_index != slot {
            continue;
        }
        return y_to_floor(key_y(key));
    }
    LOBBY_FLOOR
}

/// `{ emitDistanceFeedback, targetHeightMetric }`.
fn route_options(emit_distance_feedback: bool, target_height_metric: i64) -> Option<ResolveSimRouteOptions> {
    Some(ResolveSimRouteOptions {
        emit_distance_feedback: Some(emit_distance_feedback),
        target_height_metric: Some(target_height_metric),
        ..Default::default()
    })
}

// ─── Helper state machines (binary 1228:54b8..5a22) ─────────────────────────

/// 1228:54b8 handle_entertainment_phase_consumption.
///
/// State 0x20 (fresh): try consume budget byte (selected by entity type);
///   if budget==0 stay idle in 0x20. Otherwise route from lobby to venue floor.
/// State 0x60 (retry): route from sim.originFloor to venue floor.
/// Route results:
///   0/1/2 → state 0x60 (in-transit retry)
///   3     → increment active+attendance counters (and promote phase 1→2),
///            state 0x03 (arrived)
///   -1    → for 0x20: state 0x20, refund consumed budget byte;
///            for 0x60: state 0x27 (parked)
fn handle_entertainment_phase_consumption(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let link = find_entertainment_link(world, sim_index);

    if world.sims[si].state_code == ENT_STATE_PHASE_CONSUME {
        if let Some(link) = link {
            let family_code = world.sims[si].family_code;
            if !try_consume_entertainment_phase_budget(link_mut(world, link), family_code) {
                return;
            }
        }
    }

    let venue_floor = get_entertainment_link_venue_floor(world, link, sim_index);
    let is_fresh_dispatch = world.sims[si].state_code == ENT_STATE_PHASE_CONSUME;
    let source_floor = if is_fresh_dispatch { LOBBY_FLOOR } else { world.sims[si].selected_floor };
    let direction_flag = if venue_floor >= source_floor { 1 } else { 0 };

    // Binary 1228:5592 (handle_entertainment_phase_consumption call site):
    // `is_passenger_route = 1`, `emit_distance_feedback = (state == 0x20) ? 1 : 0`.
    // Distance feedback fires on the BASE state 0x20, not the +0x40 alias 0x60.
    let target_height_metric = entertainment_target_height_metric(world, link, sim_index);
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        venue_floor,
        direction_flag,
        Some(time),
        route_options(is_fresh_dispatch, target_height_metric),
    );

    match result {
        0 | 1 | 2 => {
            world.sims[si].state_code = ENT_STATE_PHASE_CONSUME_TRANSIT;
        }
        3 => {
            if let Some(link) = link {
                increment_entertainment_link_runtime_counters(link_mut(world, link));
            }
            world.sims[si].state_code = 0x03;
        }
        _ => {
            // -1
            if world.sims[si].state_code == ENT_STATE_PHASE_CONSUME {
                let sim = &mut world.sims[si];
                sim.state_code = ENT_STATE_PHASE_CONSUME;
                sim.origin_floor = 0;
                sim.elapsed_ticks = 0;
                sim.accumulated_ticks = 0;
                let family_code = sim.family_code;
                if let Some(link) = link {
                    increment_entertainment_half_phase(link_mut(world, link), family_code);
                }
            } else {
                world.sims[si].state_code = ENT_STATE_PARKED;
            }
        }
    }
}

/// 1228:5746 handle_entertainment_linked_half_routing.
///
/// State 0x05 (fresh): route from link reverse floor to lobby.
/// State 0x45 (retry): route from sim.originFloor to lobby.
/// Route results:
///   0/1/2 → state 0x45 (in-transit retry)
///   3/-1  → state 0x27 (parked)
fn handle_entertainment_linked_half_routing(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let is_fresh_dispatch = world.sims[si].state_code == ENT_STATE_LINKED_HALF;
    let link = find_entertainment_link(world, sim_index);
    let source_floor = if is_fresh_dispatch {
        get_entertainment_link_routing_source_floor(world, link, sim_index)
    } else {
        world.sims[si].origin_floor
    };
    let direction_flag = if LOBBY_FLOOR >= source_floor { 1 } else { 0 };

    // Binary 1228:579d (handle_entertainment_linked_half_routing call site):
    // `is_passenger_route = 1`, `emit_distance_feedback = (state == 0x05) ? 1 : 0`.
    // Distance feedback fires on the BASE state 0x05, not the +0x40 alias 0x45.
    let target_height_metric = entertainment_target_height_metric(world, link, sim_index);
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        LOBBY_FLOOR,
        direction_flag,
        Some(time),
        route_options(is_fresh_dispatch, target_height_metric),
    );

    match result {
        0 | 1 | 2 => {
            world.sims[si].state_code = ENT_STATE_LINKED_HALF_TRANSIT;
        }
        _ => {
            // 3 or -1
            world.sims[si].state_code = ENT_STATE_PARKED;
        }
    }
}

/// Binary `acquire_commercial_venue_slot` (11b0:0d92).
///
/// Lobby-fallback short-circuit: if `recordIdx < 0`, return rc=3 without
/// touching any venue record. Otherwise validate slot status (rejects
/// `availabilityState` 0xff or 3 → -1), capacity (>=0x28 currentPopulation
/// → 2), then increments occupancy and `acquireCount` (when the visitor's
/// family differs from the venue owner) and returns 3.
///
/// In all paths sim's `lastDemandTick` is stamped with the current dayTick.
fn acquire_commercial_venue_slot(world: &mut WorldState, time: &TimeState, sim_index: i64, record_idx: i64) -> i64 {
    let si = sim_index as usize;
    if record_idx < 0 {
        world.sims[si].last_demand_tick = time.day_tick;
        return 3;
    }
    let family_code = world.sims[si].family_code;
    let sidecar = match world.sidecars.get_mut(record_idx as usize) {
        Some(SidecarRecord::CommercialVenue(venue))
            if !(venue.owner_subtype_index == 0xff
                || venue.availability_state == 0xff
                || venue.availability_state == 3) =>
        {
            venue
        }
        _ => {
            let sim = &mut world.sims[si];
            advance_sim_trip_counters(sim);
            sim.last_demand_tick = time.day_tick;
            return -1;
        }
    };
    if sidecar.current_population > 39 {
        world.sims[si].last_demand_tick = time.day_tick;
        return 2;
    }
    sidecar.current_population += 1;
    if family_code != sidecar.owner_subtype_index {
        sidecar.acquire_count += 1;
    }
    world.sims[si].last_demand_tick = time.day_tick;
    3
}

/// 1228:57e2 handle_entertainment_service_acquisition.
///
/// Fresh dispatch (state 0x01):
///   - rng()%3 selects a service-bucket family (retail/restaurant/fastfood)
///     for `select_random_commercial_venue_record_for_floor`. The picker's
///     return value (record index 0..N-1, or 0xffff/-1 on failure) is
///     stored as the LOW BYTE into `sim[+6]` (1228:5826 POP AX; 5827 MOV
///     ES:[BX+6], AL). On failure that byte becomes 0xff, which sign-extends
///     to -1 in `get_current_commercial_venue_destination_floor`, routing to
///     LOBBY (10).
///   - destination = `get_current_commercial_venue_destination_floor`: for
///     `sim[+6] < 0` returns `LOBBY_FLOOR` (10); else slot's owner floor.
///   - source = `link.lowerHalfFloor` (`get_entertainment_link_routing_source_floor`).
///
/// Retry dispatch (state 0x41): source = `sim.originFloor`, dest unchanged.
///
/// After `resolve_sim_route_between_floors`, switch on rc:
///   0/1/2 → state 0x41 (in transit)
///   3     → `acquire_commercial_venue_slot(sim, sim[+6])`:
///             sim[+6]<0 → rc=3 → state 0x22
///             rc=2 (full) → state 0x41
///             rc=-1 (rejected) or rc=3 → state 0x22
///             rc=0 → return without state change (binary fallthrough)
///   -1    → fresh: state 0x41, sim[+6]=0xff, sim[+7]=0x88, sim[+8]=0xff;
///           non-fresh: state 0x27.
fn handle_entertainment_service_acquisition(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let is_fresh_dispatch = world.sims[si].state_code == ENT_STATE_SERVICE_ACQUIRE;
    let link = find_entertainment_link(world, sim_index);
    // 1228:5402-540b: dispatch reads sim[+7] (= selectedFloor in TS) into [BP-6].
    // resolve_sim_route_between_floors updates sim[+7] to the post-link floor on
    // each segment leg, so on retry the source advances toward the destination.
    let source_floor = if is_fresh_dispatch {
        get_entertainment_link_routing_source_floor(world, link, sim_index)
    } else {
        world.sims[si].selected_floor
    };

    if is_fresh_dispatch {
        // Binary 1228:5807-5827 (handle_entertainment_service_acquisition):
        // after `select_random_commercial_venue_record_for_floor`, the picker's
        // AX return value is pushed at 1228:5807 and popped back at 1228:5826,
        // then `MOV byte ptr ES:[BX+0x6], AL` at 1228:5827 writes the LOW
        // BYTE of the recordIdx into sim[+6]. The picker advances the LCG once
        // (bucket selector via `lcg15 % 3`) and once more inside
        // `select_random_commercial_venue_record_from_bucket` when a non-empty
        // row exists. We store the picker's full int return; sentinel -1 makes
        // `getCurrentCommercialVenueDestinationFloor` fall back to LOBBY_FLOOR.
        let bucket = sample_rng(world).abs() % ENT_VENUE_BUCKET_MODULO;
        let record_idx = select_random_commercial_venue_record_for_floor(world, bucket, source_floor);
        world.sims[si].commercial_venue_slot = record_idx;
    }

    let dest_floor = get_current_commercial_venue_destination_floor(world, sim_index);
    let direction_flag = if dest_floor >= source_floor { 1 } else { 0 };

    // Binary 1228:5899 (handle_entertainment_service_acquisition call site):
    // `is_passenger_route = 1`, `emit_distance_feedback = (state == 0x01) ? 1 : 0`.
    let target_height_metric = entertainment_target_height_metric(world, link, sim_index);
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        dest_floor,
        direction_flag,
        Some(time),
        route_options(is_fresh_dispatch, target_height_metric),
    );

    match result {
        0 | 1 | 2 => {
            world.sims[si].state_code = ENT_STATE_SERVICE_ACQUIRE_TRANSIT;
        }
        3 => {
            let slot = world.sims[si].commercial_venue_slot;
            let acquire_rc = acquire_commercial_venue_slot(world, time, sim_index, slot);
            if acquire_rc == 2 {
                world.sims[si].state_code = ENT_STATE_SERVICE_ACQUIRE_TRANSIT;
            } else if acquire_rc == -1 || acquire_rc == 3 {
                world.sims[si].state_code = ENT_STATE_VENUE_DWELL;
            }
            // acquireRc == 0 → binary fallthrough: no state change.
        }
        _ => {
            // -1
            if is_fresh_dispatch {
                // 1228:58db-5981: first-attempt rc=-1 writes sim[+5]=0x41, sim[+6]=0xff,
                // sim[+7]=lowerHalfFloor (re-fetched via get_entertainment_link_routing_source_floor),
                // sim[+8]=0xff.
                let routing_source_floor = get_entertainment_link_routing_source_floor(world, link, sim_index);
                let sim = &mut world.sims[si];
                sim.state_code = ENT_STATE_SERVICE_ACQUIRE_TRANSIT;
                sim.commercial_venue_slot = ENT_ROUTE_FAIL_VENUE;
                sim.selected_floor = routing_source_floor;
                sim.origin_floor = ENT_ROUTE_FAIL_VENUE;
            } else {
                world.sims[si].state_code = ENT_STATE_PARKED;
            }
        }
    }
}

/// 1228:5a23 handle_entertainment_service_release_return.
///
/// State 0x22 (fresh): gate on dwell time elapsed before releasing venue slot;
///   return early if not yet ready.
/// State 0x62 (retry): skip the dwell gate.
/// Route from sim.originFloor back to lobby.
/// Route results:
///   0/1/2 → state 0x62 (in-transit retry)
///   3/-1  → state 0x27 (parked)
fn handle_entertainment_service_release_return(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let link = find_entertainment_link(world, sim_index);
    if world.sims[si].state_code == ENT_STATE_VENUE_DWELL {
        // Binary `release_commercial_venue_slot` (11b0:0fae). On entry:
        //   - slotIdx < 0 (sentinel 0xff/0xfe): skip the venue table and write
        //     sim[+7] = LOBBY_FLOOR, sim[+8] = 0xfe; return 1.
        //   - valid record but record[+1] == -1 OR record[+2] in {-1, 3}:
        //     skip dwell check; write sim[+7] = ownerFloor, sim[+8] = 0xfe;
        //     return 1.
        //   - otherwise: dwell = dayTick - sim[+10]; if dwell < per-category
        //     service-duration global → return 0 (handler early-returns);
        //     else decrement record[+9], update availability, write sim[+7]
        //     and sim[+8] as above; return 1.
        let sim = &world.sims[si];
        if sim.commercial_venue_slot >= 0 && sim.last_demand_tick > 0 && time.day_tick - sim.last_demand_tick < 60 {
            return;
        }
        // Release succeeded: stamp sim.selectedFloor with the post-release
        // destination (lobby for sentinel slots, venue's owner floor otherwise).
        // Mirrors the binary's `sim[+7] = ownerFloor or 10` write inside
        // release_commercial_venue_slot. The route call below uses
        // sim.selectedFloor as its source, matching the binary's reliance on
        // sim[+7] for the post-release route on both fresh and retry paths.
        world.sims[si].selected_floor = get_current_commercial_venue_destination_floor(world, sim_index);
    }

    let source_floor = world.sims[si].selected_floor;
    let direction_flag = if LOBBY_FLOOR >= source_floor { 1 } else { 0 };

    // Binary 1228:5aa2 (handle_entertainment_service_release_return call site):
    // `is_passenger_route = 1`, `emit_distance_feedback = (state == 0x22) ? 1 : 0`.
    // Distance feedback fires on the BASE state 0x22, not the +0x40 alias 0x62.
    let is_fresh_dispatch = world.sims[si].state_code == ENT_STATE_VENUE_DWELL;
    let target_height_metric = entertainment_target_height_metric(world, link, sim_index);
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        LOBBY_FLOOR,
        direction_flag,
        Some(time),
        route_options(is_fresh_dispatch, target_height_metric),
    );

    match result {
        0 | 1 | 2 => {
            world.sims[si].state_code = ENT_STATE_VENUE_DWELL_TRANSIT;
        }
        _ => {
            // 3 or -1
            world.sims[si].state_code = ENT_STATE_PARKED;
        }
    }
}

// ─── Gate and dispatch entry points ─────────────────────────────────────────

/// Carrier-arrival entry point for entertainment guests (cinema / party hall).
///
/// Binary `finalize_runtime_route_state` (1228:1481) does NOT touch the
/// sim's state byte — entertainment sims keep their `0x4_` in-transit state
/// across the carrier arrival, so the next-tick dispatch enters the
/// non-fresh arm of the matching state handler with `sim.originFloor`
/// pointing at the lobby. TS strips the 0x40 bit in `finalizeRuntimeRouteState`
/// (queue/dispatch-arrivals.ts wires it inline before this handler runs);
/// we therefore re-stamp the sim's `originFloor` to the arrival floor and
/// invoke the matching state handler at the *retry* state. Mirrors the
/// binary's "sim arrived at lobby in 0x41/0x45/0x60/0x62 → next dispatch
/// routes lobby→destination as same-floor or onward leg" behaviour.
pub fn handle_entertainment_sim_arrival(world: &mut WorldState, time: &TimeState, sim_index: i64, arrival_floor: i64) {
    let sim = &mut world.sims[sim_index as usize];
    if !ENTERTAINMENT_GUEST_FAMILIES.contains(&sim.family_code) {
        return;
    }
    sim.origin_floor = arrival_floor;
    match sim.state_code {
        ENT_STATE_SERVICE_ACQUIRE => sim.state_code = ENT_STATE_SERVICE_ACQUIRE_TRANSIT,
        ENT_STATE_LINKED_HALF => sim.state_code = ENT_STATE_LINKED_HALF_TRANSIT,
        ENT_STATE_PHASE_CONSUME => sim.state_code = ENT_STATE_PHASE_CONSUME_TRANSIT,
        ENT_STATE_VENUE_DWELL => sim.state_code = ENT_STATE_VENUE_DWELL_TRANSIT,
        _ => {}
    }
    dispatch_entertainment_guest_state(world, time, sim_index);
}

/// 1228:53ad dispatch_entertainment_guest_state.
///
/// 8-entry jump table at cs:0x5b3a. Binary prologue decrements the direction-
/// load counter when stateCode >= 0x40 and encoded_route_target is a carrier
/// index (0x00..0x3f). TS omits the explicit counter decrement; it is handled
/// by the eviction path in the queue module.
pub fn dispatch_entertainment_guest_state(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let state = world.sims[sim_index as usize].state_code;

    if state == ENT_STATE_SERVICE_ACQUIRE || state == ENT_STATE_SERVICE_ACQUIRE_TRANSIT {
        handle_entertainment_service_acquisition(world, time, sim_index);
    } else if state == ENT_STATE_LINKED_HALF || state == ENT_STATE_LINKED_HALF_TRANSIT {
        handle_entertainment_linked_half_routing(world, time, sim_index);
    } else if state == ENT_STATE_PHASE_CONSUME || state == ENT_STATE_PHASE_CONSUME_TRANSIT {
        handle_entertainment_phase_consumption(world, time, sim_index);
    } else if state == ENT_STATE_VENUE_DWELL || state == ENT_STATE_VENUE_DWELL_TRANSIT {
        handle_entertainment_service_release_return(world, time, sim_index);
    }
}

/// 1228:5231 gate_entertainment_guest_state.
///
/// Jump table at cs:0x539d (4 entries for stateCode < 0x40):
///   0x01 → direct dispatch (handler 0x537f)
///   0x05 → direct dispatch (handler 0x537f)
///   0x20 → daypart 0–3 + dayTick > 0xf0 + RNG%6==0 gate, then dispatch (0x530d)
///   0x22 → direct dispatch (handler 0x537f)
///
/// For stateCode >= 0x40: if on a carrier queue, delegate to
/// maybeDispatchQueuedRouteAfterWait; otherwise dispatch directly.
pub fn gate_entertainment_guest_state(world: &mut WorldState, ledger: &LedgerState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    if !ENTERTAINMENT_GUEST_FAMILIES.contains(&world.sims[si].family_code) {
        return;
    }

    if !is_sim_in_transit(world.sims[si].state_code) {
        let state = world.sims[si].state_code;

        if state == ENT_STATE_PHASE_CONSUME {
            // Binary handler 0x530d: two independent branches.
            //   if (daypart in 0..3 && dayTick > 0xf0 && rng()%6 == 0): dispatch
            //   if (daypart > 3): sim.stateCode = 0x27 (force-park end-of-day)
            if time.daypart_index >= 0
                && time.daypart_index <= 3
                && time.day_tick > ENT_GATE_DAY_TICK_MIN
                && sample_rng(world) % ENT_GATE_RNG_MODULO == 0
            {
                dispatch_entertainment_guest_state(world, time, sim_index);
            }
            if time.daypart_index > 3 {
                world.sims[si].state_code = ENT_STATE_PARKED;
            }
            return;
        } else if state != ENT_STATE_SERVICE_ACQUIRE && state != ENT_STATE_LINKED_HALF && state != ENT_STATE_VENUE_DWELL {
            return;
        }
    } else if matches!(world.sims[si].route, RouteState::Carrier { .. }) {
        maybe_dispatch_queued_route_after_wait(world, ledger, time, sim_index);
        return;
    }

    dispatch_entertainment_guest_state(world, time, sim_index);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_bucket_rows() {
        // Floors 5..14 are row 0, 15..19 none, 20..29 row 1, ... 95..104 row 6.
        assert_eq!(classify_path_bucket_index(5), 0);
        assert_eq!(classify_path_bucket_index(14), 0);
        assert_eq!(classify_path_bucket_index(15), -1);
        assert_eq!(classify_path_bucket_index(20), 1);
        assert_eq!(classify_path_bucket_index(104), 6);
        assert_eq!(classify_path_bucket_index(110), -1);
        // Below floor 5: Math.trunc gives (-)0 and offset % 15 is negative,
        // so JavaScript returns row 0 (as -0) for floors 0..4.
        assert_eq!(classify_path_bucket_index(0), 0);
    }
}

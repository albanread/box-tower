// ts: sims/index.ts

pub mod commercial;
pub mod condo;
pub mod facility_refunds;
pub mod hotel;
pub mod hotel_facilities;
pub mod housekeeping;
pub mod medical;
pub mod office;
pub mod parking;
pub mod population;
pub mod scoring;
pub mod states;
pub mod trip_counters;

use indexmap::IndexSet;

use crate::cathedral::{handle_cathedral_sim_arrival, process_cathedral_sim};
use crate::families::entertainment::{gate_entertainment_guest_state, handle_entertainment_sim_arrival};
use crate::families::maybe_dispatch_after_wait::maybe_dispatch_queued_route_after_wait;
use crate::ledger::LedgerState;
use crate::resources::{
    FAMILY_CINEMA, FAMILY_CINEMA_LOWER, FAMILY_CINEMA_STAIRS_LOWER, FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN,
    FAMILY_HOUSEKEEPING, FAMILY_OFFICE, FAMILY_PARTY_HALL, FAMILY_PARTY_HALL_LOWER,
    FAMILY_RESTAURANT, FAMILY_RETAIL,
};
use crate::sim_access::state_bits::is_sim_in_transit;
use commercial::{handle_commercial_sim_arrival, process_commercial_sim};
use condo::{handle_condo_sim_arrival, process_condo_sim};

pub use facility_refunds::{
    close_commercial_venues, close_commercial_venues_by_family, rebuild_commercial_venue_runtime,
    rebuild_restaurant_facility_records, refund_unhappy_facilities,
};

use hotel::{handle_hotel_sim_arrival, process_hotel_sim};
use housekeeping::{handle_housekeeping_sim_arrival, process_housekeeping_sim};

pub use hotel_facilities::{
    advance_object_stay_phase_tiers, handle_extended_vacancy_expiry,
    normalize_unit_status_end_of_day, spread_cockroach_infestation,
    update_hotel_operational_and_occupancy,
};

use medical::{
    handle_medical_sim_arrival, process_medical_sim, STATE_MEDICAL_DWELL, STATE_MEDICAL_TRIP,
    STATE_MEDICAL_TRIP_TRANSIT,
};
use office::{handle_office_sim_arrival, process_office_sim};
use states::{
    COMMERCIAL_DWELL_STATE, COMMERCIAL_VENUE_DWELL_TICKS, ENTITY_REFRESH_STRIDE, INVALID_FLOOR,
    STATE_ACTIVE_TRANSIT, STATE_AT_WORK_TRANSIT, STATE_COMMUTE, STATE_COMMUTE_TRANSIT,
    STATE_DEPARTURE_TRANSIT, STATE_DWELL_RETURN_TRANSIT, STATE_MORNING_TRANSIT,
    STATE_VENUE_HOME_TRANSIT, STATE_VENUE_TRIP, STATE_VENUE_TRIP_TRANSIT,
};

pub use parking::{rebuild_parking_coverage, rebuild_parking_demand_log, try_assign_parking_service};
pub use population::{
    cleanup_sims_for_removed_tile, clear_sim_route, find_object_for_sim, find_sibling_sims,
    rebuild_runtime_sims, reset_sim_runtime_state, sim_key,
};
pub use scoring::{
    average_trip_stress_ticks, create_sim_state_record, create_sim_state_records,
    current_trip_stress_level, current_trip_stress_ticks, maybe_apply_distance_feedback,
    recompute_all_object_operational_status, recompute_object_operational_status,
    refresh_occupied_flag_and_trip_counters, SimStateRecord,
};
pub use states::{
    CATHEDRAL_FAMILIES, EVAL_ZONE_FLOOR, LOBBY_FLOOR, NO_EVAL_ENTITY, STATE_ACTIVE, STATE_ARRIVED,
    STATE_DEPARTURE, STATE_EVAL_OUTBOUND, STATE_EVAL_RETURN, STATE_MORNING_GATE, STATE_PARKED,
};
pub use trip_counters::{
    add_delay_to_current_sim, advance_sim_trip_counters, rebase_sim_elapsed_from_clock,
    reset_facility_sim_trip_counters, reset_sim_trip_counters,
};

use crate::time::TimeState;
use crate::world::{
    sample_rng, y_to_floor, CommercialVenueRecord, RouteState, SidecarRecord, SimRecord,
    WorldState, VENUE_CLOSED, VENUE_DORMANT,
};

/// The y of a placed-object key "x,y" (`key.split(",").map(Number)[1]`).
fn key_y(key: &str) -> i64 {
    key.split(',')
        .nth(1)
        .and_then(|y| y.parse().ok())
        .unwrap_or_else(|| panic!("placed-object key {key:?} is not \"x,y\""))
}

/// The commercial-venue record at `sidecars[record_index]` and the sim at
/// `sims[sim_index as usize]`, both writable: what upstream holds as two references
/// into the world at once.  (Not upstream's; a borrow helper for the
/// callers of `try_acquire_office_venue_slot` / `release_office_venue_slot`.
/// The index must name a commercial-venue record, as every caller has
/// checked.)
pub(crate) fn venue_and_sim_mut(
    world: &mut WorldState,
    record_index: usize,
    sim_index: i64,
) -> (&mut CommercialVenueRecord, &mut SimRecord) {
    let WorldState { sidecars, sims, .. } = world;
    match &mut sidecars[record_index] {
        SidecarRecord::CommercialVenue(record) => (record, &mut sims[sim_index as usize]),
        _ => panic!("sidecar {record_index} is not a commercial venue"),
    }
}

pub fn release_service_request(world: &mut WorldState, sim_index: i64) {
    let sim = &mut world.sims[sim_index as usize];
    sim.destination_floor = -1;
    clear_sim_route(sim);
}

fn recompute_routes_viable_flag(world: &mut WorldState) {
    // Binary-grounded: rebuild_path_seed_bucket_table unconditionally latches
    // routesViable = 1 whenever star_count > 2; no route-scoring predicate found.
    world.gate_flags.routes_viable = if world.star_count > 2 { 1 } else { 0 };
}

/// Upstream's `record` is the CommercialVenueRecord itself; here it is the
/// record's index in `world.sidecars`.
struct VenueSelection {
    record: usize,
    floor: i64,
    #[allow(dead_code)] // upstream records it and never reads it
    height_metric: i64,
    /// PlacedObjectRecord.objectTypeCode of the venue's owning facility.
    /// Mirrors binary `record[+0]` (sim_type) for the type/variant gate in
    /// `acquire_commercial_venue_slot` (11b0:0d92).
    owner_family: i64,
}

/// The commercial-venue record at `world.sidecars[index]`, if there is one
/// there (`world.sidecars[i] as CommercialVenueRecord | undefined` with the
/// `kind` check).
fn commercial_venue_at(world: &WorldState, index: i64) -> Option<&CommercialVenueRecord> {
    if index < 0 {
        return None;
    }
    match world.sidecars.get(index as usize) {
        Some(SidecarRecord::CommercialVenue(record)) => Some(record),
        _ => None,
    }
}

/// Mirror of binary helper that finds an office worker's chosen commercial
/// venue at a specific floor by family. The binary keeps the picked venue
/// record index in entity[+6] and re-resolves through that on each
/// dispatch; we don't currently store the index, so we look up by
/// `(arrival floor, family set)`. For the current fixtures (one venue per
/// floor of each family), this is unambiguous and matches the binary.
///
/// Returns the first venue found at `floor` whose family is in `families`
/// and whose record is non-INVALID; null otherwise.
///
/// (Here: the venue record's index in `world.sidecars`.)
pub fn find_commercial_venue_at_floor(world: &WorldState, floor: i64, families: &[i64]) -> Option<usize> {
    for (key, object) in world.placed_objects.iter() {
        if !families.contains(&object.object_type_code) {
            continue;
        }
        if object.linked_record_index < 0 {
            continue;
        }
        let y = key_y(key);
        if y_to_floor(y) != floor {
            continue;
        }
        let Some(record) = commercial_venue_at(world, object.linked_record_index) else {
            continue;
        };
        if record.owner_subtype_index == INVALID_FLOOR {
            continue;
        }
        return Some(object.linked_record_index as usize);
    }
    None
}

/// Result code mirroring binary `acquire_commercial_venue_slot` (11b0:0d92).
pub const VENUE_SLOT_ACQUIRED: i64 = 3;
pub const VENUE_SLOT_FULL: i64 = 2;
pub const VENUE_SLOT_UNAVAILABLE: i64 = -1;

/// Mirror of binary `acquire_commercial_venue_slot` (11b0:0d92) for an
/// office worker arriving at a venue. Returns the binary's service-route
/// state code (3=acquired, 2=full, -1=unavailable). Increments the venue's
/// `currentPopulation` (binary record+9) on the success path and stamps
/// the sim's `lastDemandTick` with the current dayTick (binary entity+0xa).
///
/// Capacity gate: binary uses `if ('\'' < currentPopulation)` i.e. > 39.
/// Match that exactly (rather than the off-by-one `< 39` used in
/// commercial.ts:135 for venue owners' own arrivals) so visiting workers
/// see the same threshold the binary's elevator-arrival handler does.
///
/// Per binary, on the success path `record[+0x10]` (acquireCount) is
/// also incremented when the visitor's type/variant differs from the
/// venue owner's. For office workers visiting fast-food the gate fires
/// (office=family 7, fast-food=family 12); for a venue owner sim
/// arriving at their own venue the gate suppresses the bump (same family).
/// Pass `venueOwnerFamily` so the helper can apply the gate correctly.
///
/// Note: the binary does NOT touch `record[+7]` (todayVisitCount,
/// staff-emit count) at acquire time — that counter is only bumped by
/// `try_consume_commercial_venue_capacity` (11b0:1150) at MORNING_GATE
/// and is what feeds the population/star ledger after the daily roll.
pub fn try_acquire_office_venue_slot(
    venue: &mut CommercialVenueRecord,
    sim: &mut SimRecord,
    time: &TimeState,
    venue_owner_family: i64,
) -> i64 {
    if venue.owner_subtype_index == 0xff
        || venue.availability_state == VENUE_DORMANT
        || venue.availability_state == VENUE_CLOSED
    {
        return VENUE_SLOT_UNAVAILABLE;
    }
    if venue.current_population > 39 {
        // Binary 11b0:0f6c+: writes entity[+0xa] = g_day_tick on the full path
        // before returning 2. We don't mirror that latch in TS because our
        // per-stride office handler doesn't gate on lastDemandTick (it gates
        // on queueTick, set only on successful 0x22 transition); stamping
        // lastDemandTick on the rejection path here would interfere with the
        // stress-rebase pipeline without a corresponding gating use.
        return VENUE_SLOT_FULL;
    }
    venue.current_population += 1;
    if sim.family_code != venue_owner_family {
        // Binary 11b0:0f3a–0f55: bump record+0x10 (acquireCount) only when
        // the visitor's type/variant differ from the venue owner's. This is
        // the gate that prevents the venue's own owner sim (arriving at their
        // home venue via MORNING_GATE) from double-counting today's visitor
        // acquisitions. acquireCount feeds closure cashflow bands, NOT the
        // population/star ledger — that path reads `todayVisitCount`
        // (staff-emit count, binary +0x7) after the daily roll.
        venue.acquire_count += 1;
    }
    sim.last_demand_tick = time.day_tick;
    VENUE_SLOT_ACQUIRED
}

/// Mirror of binary `release_commercial_venue_slot` (11b0:0fae) for an
/// office worker leaving a venue. Decrements `currentPopulation` if the
/// venue is non-dormant. Returns true if the slot was released (and the
/// caller may proceed with the return route), false if the dwell timer
/// has not expired yet (binary returns 0 in that case).
///
/// The dwell gate is the standard `dayTick - lastAcquireTick >=
/// COMMERCIAL_VENUE_DWELL_TICKS` (60 ticks) check. Office callers that
/// already enforce the dwell gate themselves can pass `skipDwellGate=true`.
pub fn release_office_venue_slot(
    venue: &mut CommercialVenueRecord,
    sim: &SimRecord,
    time: &TimeState,
    skip_dwell_gate: Option<bool>,
) -> bool {
    let skip_dwell_gate = skip_dwell_gate.unwrap_or(false);
    if venue.owner_subtype_index == 0xff
        || venue.availability_state == VENUE_DORMANT
        || venue.availability_state == VENUE_CLOSED
    {
        return true;
    }
    if !skip_dwell_gate && time.day_tick - sim.last_demand_tick < COMMERCIAL_VENUE_DWELL_TICKS {
        return false;
    }
    if venue.current_population > 0 {
        venue.current_population -= 1;
    }
    true
}

fn pick_available_venue(world: &mut WorldState, _from_floor: i64, allowed_families: &[i64]) -> Option<VenueSelection> {
    // Binary select_random_commercial_venue_record_from_bucket (11b0:1361):
    // consults the per-family zone bucket built at placement time and picks
    // uniformly at random. The bucket contains every placed venue of the
    // family — availability, capacity, and route viability are all checked
    // AFTER the RNG call. To match PRNG parity we mirror that shape: call
    // sampleRng whenever the bucket has any entry, even if the chosen venue
    // ends up rejected.
    let mut bucket: Vec<VenueSelection> = Vec::new();
    for (key, object) in world.placed_objects.iter() {
        if !allowed_families.contains(&object.object_type_code) {
            continue;
        }
        if object.linked_record_index < 0 {
            continue;
        }
        let Some(record) = commercial_venue_at(world, object.linked_record_index) else {
            continue;
        };
        if record.owner_subtype_index == INVALID_FLOOR {
            continue;
        }

        let y = key_y(key);
        bucket.push(VenueSelection {
            record: object.linked_record_index as usize,
            floor: y_to_floor(y),
            height_metric: object.left_tile_index,
            owner_family: object.object_type_code,
        });
    }

    if bucket.is_empty() {
        return None;
    }
    let pick = (sample_rng(world) % bucket.len() as i64) as usize;
    let picked = bucket.swap_remove(pick);

    let availability_state = commercial_venue_at(world, picked.record as i64)
        .expect("picked venue record")
        .availability_state;
    if availability_state == VENUE_CLOSED || availability_state == VENUE_DORMANT {
        return None;
    }
    // Binary `select_random_commercial_venue_record_from_bucket` (11b0:1361)
    // does NOT filter on capacity or route viability at pick time. Both are
    // enforced later: capacity by `acquire_commercial_venue_slot` (11b0:0d92)
    // at arrival, and route viability by `resolve_sim_route_between_floors`
    // (which fires the 300-tick failure delay + advance_sim_trip_counters on
    // rc=-1). The caller's resolve call therefore needs to run for unreachable
    // venues so those side effects fire.
    Some(picked)
}

// Phase 7: `reduceElapsedForLobbyBoarding` was promoted into the inline
// boarding path in `queue/process-travel.ts`, where
// `accumulate_elapsed_delay_into_current_sim` runs as part of the boarding
// loop (matching the binary's 1218:0d4e `assign_request_to_runtime_route`).

// NOTE: there is no `completeSimTransitEvent` in the binary. The 6
// `add_or_update_sim_trip_counters` (== `advance_sim_trip_counters` 11e0:0000)
// call sites are:
//   1218:0046  resolve_sim_route_between_floors rc=3 (same-floor arrival)
//                — modeled in queue/resolve.ts:171 + same-floor short-circuit
//                  paths (sims/index.ts:364, sims/hotel.ts:564)
//   1218:00a4  resolve_sim_route_between_floors rc=-1 (no-route failure)
//                — modeled in queue/resolve.ts:189 + 247 (no-carrier failure)
//   1228:18dc  dispatch_sim_behavior (per-tick refresh; HK family 0x0f bypassed)
//                — modeled via families/maybe-dispatch-after-wait.ts:104
//   1228:1592  finalize_runtime_route_state (called from update_sim_tile_span)
//                — modeled in families/finalize.ts:36
//   11b0:0e0e  acquire_commercial_venue_slot venue-unavailable failure branch
//                — modeled in dispatchCommercialVenueVisit no-venue path
//                  (sims/index.ts:332). The advance fires ONLY in the failure
//                  branch (owner_subtype==0xff || venue offset+2 in {-1,3});
//                  the retry-overflow (rc=2) and success (rc=3) paths do NOT
//                  advance.
//   1178:02e1  office_sim_check_medical_service_slot target-gone failure branch
//                — modeled in sims/medical.ts processMedicalSim targetGone
//                  branch. Advance fires ONLY when slot.target == -1; the
//                  retry-overflow and success paths do NOT advance.
//
// The arrival dispatcher `dispatch_destination_queue_entries` (1218:0883)
// does NOT invoke `dispatch_sim_behavior` and does NOT advance trip counters
// — it writes sim+7 to the destination floor and jumps directly into the
// family-specific state handler. Mirrors of that path therefore must NOT
// advance counters at arrival.

fn begin_commercial_venue_dwell(sim: &mut SimRecord, arrival_floor: i64, return_state: i64, time: &TimeState) {
    sim.destination_floor = -1;
    sim.selected_floor = arrival_floor;
    clear_sim_route(sim);
    sim.venue_return_state = return_state;
    sim.state_code = COMMERCIAL_DWELL_STATE;
    sim.last_demand_tick = time.day_tick;
}

fn begin_commercial_venue_trip(sim: &mut SimRecord, destination_floor: i64, trip_state: i64) {
    sim.destination_floor = destination_floor;
    // For office + hotel + condo families the prior resolve already wrote
    // selectedFloor to the next leg endpoint; preserve it. For other families
    // that don't yet do per-tick re-resolution, reset to floorAnchor
    // (legacy behavior).
    let uses_per_leg = sim.family_code == FAMILY_OFFICE
        || sim.family_code == FAMILY_CONDO
        || sim.family_code == FAMILY_HOTEL_SINGLE
        || sim.family_code == FAMILY_HOTEL_TWIN
        || sim.family_code == FAMILY_HOTEL_SUITE;
    if !uses_per_leg {
        sim.selected_floor = sim.floor_anchor;
    }
    sim.state_code = trip_state;
}

pub fn finish_commercial_venue_dwell(sim: &mut SimRecord, time: &TimeState, default_state: i64) -> bool {
    if sim.state_code != COMMERCIAL_DWELL_STATE {
        return false;
    }
    if time.day_tick - sim.last_demand_tick < COMMERCIAL_VENUE_DWELL_TICKS {
        return true;
    }
    sim.selected_floor = sim.floor_anchor;
    // `sim.venueReturnState || defaultState`: 0 is falsy.
    sim.state_code = if sim.venue_return_state != 0 {
        sim.venue_return_state
    } else {
        default_state
    };
    sim.venue_return_state = 0;
    true
}

pub fn finish_commercial_venue_trip(sim: &mut SimRecord, return_state: i64) -> bool {
    if sim.state_code != STATE_VENUE_TRIP {
        return false;
    }
    if sim.selected_floor != sim.destination_floor {
        return true;
    }
    sim.destination_floor = -1;
    sim.selected_floor = sim.floor_anchor;
    sim.state_code = return_state;
    true
}

/// Upstream's `options` object literal is its fields as parameters, in
/// upstream's order; `onVenueReserved` is given the world, through which it
/// reaches what upstream's closure captured.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_commercial_venue_visit(
    world: &mut WorldState,
    time: &TimeState,
    sim_index: i64,
    venue_families: &[i64],
    return_state: i64,
    trip_state: Option<i64>,
    unavailable_state: Option<i64>,
    skip_penalty_on_unavailable: Option<bool>,
    advance_before_same_floor_dwell: Option<bool>,
    on_venue_reserved: Option<&mut dyn FnMut(&mut WorldState)>,
) -> bool {
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let Some(venue) = pick_available_venue(world, floor_anchor, venue_families) else {
        if !skip_penalty_on_unavailable.unwrap_or(false) {
            add_delay_to_current_sim(&mut world.sims[sim_index as usize], 300);
            advance_sim_trip_counters(&mut world.sims[sim_index as usize]);
        }
        if let Some(unavailable_state) = unavailable_state {
            world.sims[sim_index as usize].state_code = unavailable_state;
        }
        return false;
    };

    // Route requirement: resolve route before reserving venue.
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    if venue.floor != floor_anchor {
        let dir_flag = if venue.floor > floor_anchor { 1 } else { 0 };
        let route_result =
            resolve_sim_route_between_floors(world, sim_index, floor_anchor, venue.floor, dir_flag, Some(time), None);
        if route_result == -1 {
            // Binary `route_sim_to_commercial_venue` (1238:0000) hides the resolve
            // failure when called from state 0x01: writes sim+5=tripState (0x41),
            // sim+6=0xff sentinel, sim+7=origin, sim+8=0xff and returns success
            // to the caller. The resolve has already cleared the route and
            // applied the 300-tick failure delay + trip-counter advance. On the
            // next-stride state-0x41 alias re-entry, the binary skips selection
            // and `get_current_commercial_venue_destination_floor` reads the
            // 0xff sentinel and returns floor 10 (lobby) — so the recovery
            // resolve targets the lobby. We model that by writing the lobby
            // destination directly here, so the standard transit handler at
            // the next stride simply re-resolves anchor → lobby and assigns a
            // carrier route without re-running the venue dispatch (which would
            // fail again and incorrectly stack another 300-tick delay).
            let sim = &mut world.sims[sim_index as usize];
            sim.state_code = trip_state.unwrap_or(STATE_VENUE_TRIP);
            sim.selected_floor = sim.floor_anchor;
            sim.destination_floor = LOBBY_FLOOR;
            return true;
        }
        if route_result == 0 {
            if let Some(unavailable_state) = unavailable_state {
                world.sims[sim_index as usize].state_code = unavailable_state;
            }
            return false;
        }

        // Binary `route_sim_to_commercial_venue` (1238:0000) state-0x01 cross-
        // floor branch: resolve returns 0/1/2 (in transit) → state 0x41
        // (tripState). Acquire is NOT called here; it fires when the per-
        // stride state-0x41 handler re-resolves the next leg and gets rc=3
        // at carrier arrival (modeled in office.ts handleOfficeSimArrival
        // STATE_ACTIVE_TRANSIT branch). The `onVenueReserved` callback fires
        // here so per-family bookkeeping (e.g. hotel activationTickCount) can
        // register the trip start.
        if let Some(on_venue_reserved) = on_venue_reserved {
            on_venue_reserved(world);
        }
        begin_commercial_venue_trip(
            &mut world.sims[sim_index as usize],
            venue.floor,
            trip_state.unwrap_or(STATE_VENUE_TRIP),
        );
        return true;
    }

    // Same-floor branch: binary `route_sim_to_commercial_venue` (1238:0000)
    // state-0x01 same-floor path takes resolve rc=3 and immediately calls
    // `acquire_commercial_venue_slot` (11b0:0d92). Mirror that here so the
    // pick-time bump of `acquireCount` is gated on capacity (currentPopulation
    // > 39 → SLOT_FULL → no bump) and on the visitor-vs-owner type gate.
    if advance_before_same_floor_dwell.unwrap_or(false) {
        // Binary: resolve_sim_route(floor→floor) returns 3 → advanceSimTripCounters
        advance_sim_trip_counters(&mut world.sims[sim_index as usize]);
    }
    let acquire_result = {
        let (record, sim) = venue_and_sim_mut(world, venue.record, sim_index);
        try_acquire_office_venue_slot(record, sim, time, venue.owner_family)
    };
    if acquire_result == VENUE_SLOT_FULL {
        // Binary state-0x01 same-floor + acquire=2: writes state=0x41 (tripState)
        // — the sim stays in the in-transit alias and the per-stride 0x41 handler
        // re-attempts acquire next stride. Restore route fields to the same-floor
        // shape so the retry loop is well-formed.
        let sim = &mut world.sims[sim_index as usize];
        sim.state_code = trip_state.unwrap_or(STATE_VENUE_TRIP);
        sim.selected_floor = sim.floor_anchor;
        sim.destination_floor = sim.floor_anchor;
        sim.venue_return_state = return_state;
        return true;
    }
    if let Some(on_venue_reserved) = on_venue_reserved {
        on_venue_reserved(world);
    }
    begin_commercial_venue_dwell(&mut world.sims[sim_index as usize], venue.floor, return_state, time);
    true
}

pub fn handle_commercial_venue_arrival(
    sim: &mut SimRecord,
    arrival_floor: i64,
    return_state: i64,
    time: &TimeState,
    trip_state: Option<i64>,
) -> bool {
    let trip_state = trip_state.unwrap_or(STATE_VENUE_TRIP);
    if sim.state_code != trip_state || sim.destination_floor != arrival_floor {
        return false;
    }
    begin_commercial_venue_dwell(sim, arrival_floor, return_state, time);
    true
}

// Phase 5b: the office wait-timeout logic moved to
// `families/maybe-dispatch-after-wait.ts` (1228:15a0
// maybe_dispatch_queued_route_after_wait). The stride refresh calls
// `maybeDispatchQueuedRouteAfterWait` directly now.

pub fn advance_sim_refresh_stride(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    if world.sims.is_empty() {
        return;
    }

    let stride = time.day_tick % ENTITY_REFRESH_STRIDE;
    // `for (let index = 0; index < world.sims.length; index++)`: the length
    // is read each time round.
    let mut index = 0usize;
    while index < world.sims.len() {
        let sim_index = index as i64;
        index += 1;
        if sim_index as i64 % ENTITY_REFRESH_STRIDE != stride {
            continue;
        }
        // BOX: emergency -- a collapsed patient does nothing until released
        // (false at once while nobody is held).
        if crate::box_ext::emergency::is_held(world, sim_index) {
            continue;
        }
        // BOX: car elevator -- a commuter driving in waits for the car to park.
        if crate::box_ext::car_elevator::is_driving(world, sim_index) {
            continue;
        }
        // BOX: fire -- someone sent out by the fire alarm waits outside.
        if crate::box_ext::fire::is_out(world, sim_index) {
            continue;
        }
        // Binary parity: `refresh_runtime_entities_for_tick_stride` (1228:0d64)
        // does NOT call `rebase_sim_elapsed_from_clock` at all. The only stride-
        // time rebase points in the binary are inside `dispatch_sim_behavior`
        // (1228:186c) — invoked by `dispatch_queued_route_until_request` when a
        // queued carrier route pops — and `cancel_runtime_route_request`
        // (1218:1b85) when a request is canceled. Per-family refresh handlers
        // (1228:1cb5 office, 1228:2aec hotel, 1228:3a2f condo, etc.) drive the
        // state machine directly without rebasing.
        //
        // Sim+0xa (`lastDemandTick`) is multipurpose in the binary: it serves as
        // the carrier-queue enqueue stamp (set by `resolve_sim_route_between_floors`
        // rc=2), the commercial-venue acquire/dwell stamp (set by
        // `acquire_commercial_venue_slot` rc=2/3), and the medical-slot acquire
        // stamp (set by `office_sim_check_medical_service_slot`). A per-stride
        // rebase would corrupt those stamps — most visibly, after an office
        // worker acquires a fast-food slot, the next stride would add the
        // 16-tick delta into elapsedTicks, inflating stress by ~6 per visit.
        finalize_pending_route_leg(&mut world.sims[sim_index as usize]);
        // Binary: refresh_object_family_office_state_handler (1228:1cb5) for
        // state >= 0x40 + on-carrier sims calls maybe_dispatch_queued_route_after_wait
        // (1228:15a0). After 300 ticks (g_route_delay_table_base, loaded from
        // resource type 0xff05 id 1000 word 0), it dispatches to the family-7
        // state-0x60 handler at 1228:193d which writes sim[+5] = 0x26 (NIGHT_B).
        // Phase 5b: delegated to families/maybe-dispatch-after-wait.ts.
        // Capture the binary's gate (state_code & 0x40 && sim+8 >= 0x40, the
        // carrier-queued alias) BEFORE invoking the helper: on timeout
        // `dispatchTimedOutOfficeQueueEntry` rewrites the state and clears the
        // route, so a post-helper check would let the per-state handler run
        // this stride and double-sample RNG. Binary 1228:1d59-1d6e takes a
        // hard either/or — when the helper runs, per-state dispatch is skipped.
        let sim = &world.sims[sim_index as usize];
        let was_queued_carrier_alias =
            is_sim_in_transit(sim.state_code) && matches!(sim.route, RouteState::Carrier { .. });
        maybe_dispatch_queued_route_after_wait(world, ledger, time, sim_index);
        if was_queued_carrier_alias {
            continue;
        }
        // Binary: sims with carrier route (sim+8 >= 0x40) are gated by
        // maybe_dispatch_queued_route_after_wait above; the family state handler
        // is NOT re-invoked. Sims with segment route (sim+8 < 0x40) DO go through
        // the family handler every stride — that's how segment legs progress
        // (each call to resolve writes one leg into sim+7, sim+8). Idle sims
        // also dispatch the family handler.
        // Other families have not migrated to per-leg progression yet; skip the
        // state handler for any non-idle route to preserve their legacy behavior.
        let sim = &world.sims[sim_index as usize];
        if matches!(sim.route, RouteState::Carrier { .. }) {
            continue;
        }
        // Phase 4 + housekeeping + commercial: office + condo + hotel +
        // housekeeping + restaurant/fast-food/retail have migrated to per-stride
        // per-leg re-resolution. Other families still rely on the legacy
        // whole-trip finalizer in `reconcileSimTransport`.
        if matches!(sim.route, RouteState::Segment { .. })
            && sim.family_code != FAMILY_OFFICE
            && sim.family_code != FAMILY_CONDO
            && sim.family_code != FAMILY_HOTEL_SINGLE
            && sim.family_code != FAMILY_HOTEL_TWIN
            && sim.family_code != FAMILY_HOTEL_SUITE
            && sim.family_code != FAMILY_HOUSEKEEPING
            && sim.family_code != FAMILY_RESTAURANT
            && sim.family_code != FAMILY_FAST_FOOD
            && sim.family_code != FAMILY_RETAIL
            && sim.family_code != FAMILY_CINEMA
            && sim.family_code != FAMILY_CINEMA_LOWER
            && sim.family_code != FAMILY_CINEMA_STAIRS_UPPER
            && sim.family_code != FAMILY_CINEMA_STAIRS_LOWER
            && sim.family_code != FAMILY_PARTY_HALL
            && sim.family_code != FAMILY_PARTY_HALL_LOWER
        {
            continue;
        }
        match sim.family_code {
            FAMILY_HOTEL_SINGLE | FAMILY_HOTEL_TWIN | FAMILY_HOTEL_SUITE => {
                process_hotel_sim(world, ledger, time, sim_index);
            }
            FAMILY_OFFICE => {
                if sim.state_code == STATE_MEDICAL_TRIP
                    || sim.state_code == STATE_MEDICAL_TRIP_TRANSIT
                    || sim.state_code == STATE_MEDICAL_DWELL
                {
                    process_medical_sim(world, time, sim_index);
                } else {
                    process_office_sim(world, ledger, time, sim_index);
                }
            }
            FAMILY_CONDO => {
                process_condo_sim(world, ledger, time, sim_index);
            }
            FAMILY_RESTAURANT | FAMILY_FAST_FOOD | FAMILY_RETAIL => {
                process_commercial_sim(world, ledger, time, sim_index);
            }
            FAMILY_HOUSEKEEPING => {
                process_housekeeping_sim(world, time, sim_index);
            }
            FAMILY_CINEMA
            | FAMILY_CINEMA_LOWER
            | FAMILY_CINEMA_STAIRS_UPPER
            | FAMILY_CINEMA_STAIRS_LOWER
            | FAMILY_PARTY_HALL
            | FAMILY_PARTY_HALL_LOWER => {
                gate_entertainment_guest_state(world, ledger, time, sim_index);
            }
            family_code => {
                if CATHEDRAL_FAMILIES.contains(&family_code) {
                    process_cathedral_sim(world, time, sim_index);
                }
            }
        }
    }

    recompute_routes_viable_flag(world);
}

// 1228:0d64 refresh_runtime_entities_for_tick_stride — binary-named alias
// for advanceSimRefreshStride. Used by tick/carrier-tick.ts. Existing callers
// (tests, TowerSim.step via re-export) continue using the old name.
pub use self::advance_sim_refresh_stride as refresh_runtime_entities_for_tick_stride;

pub use crate::queue::resolve::{resolve_sim_route_between_floors, RouteResolution};

fn should_finalize_segment_trip(sim: &SimRecord) -> bool {
    // Phase 1d-ii / Phase 4: office + condo + hotel families are now driven by
    // per-stride per-leg re-resolution; arrivals fire from inside the
    // per-state handler when resolve returns 3 (same-floor). Skip the
    // legacy whole-trip finalizer for these segment sims.
    if sim.family_code == FAMILY_OFFICE {
        return false;
    }
    if sim.family_code == FAMILY_CONDO {
        return false;
    }
    if sim.family_code == FAMILY_HOTEL_SINGLE
        || sim.family_code == FAMILY_HOTEL_TWIN
        || sim.family_code == FAMILY_HOTEL_SUITE
    {
        return false;
    }
    if sim.family_code == FAMILY_RESTAURANT
        || sim.family_code == FAMILY_FAST_FOOD
        || sim.family_code == FAMILY_RETAIL
    {
        // Commercial families (restaurant/fast-food/retail) migrated to
        // per-stride per-leg re-resolution via processCommercialSim's
        // MORNING_TRANSIT (0x60) and DEPARTURE_TRANSIT (0x45) handlers.
        // The legacy whole-trip teleport here would short-circuit the binary's
        // 16-tick stride wait between MORNING_GATE -> MORNING_TRANSIT -> arrival.
        return false;
    }
    if sim.family_code == FAMILY_HOUSEKEEPING {
        // Phase 5b/c: HK migrated to per-stride per-leg re-resolution like
        // office/condo/hotel. The whole-trip teleport here would race with the
        // per-stride state-3 dispatch — for a 1-floor route the same-tick
        // teleport fires `dispatchSimArrival` → `handleHousekeepingSimArrival`
        // → `tryClaimOnCurrentFloor` → `promoteClaim`, advancing the HK helper
        // to state 2 in the same tick the route is initiated. The binary
        // requires a separate stride for the state-3 re-resolve to return 3
        // (same-floor) before the claim fires. Skip the legacy finalizer.
        return false;
    }
    if sim.family_code == FAMILY_CINEMA
        || sim.family_code == FAMILY_CINEMA_LOWER
        || sim.family_code == FAMILY_CINEMA_STAIRS_UPPER
        || sim.family_code == FAMILY_CINEMA_STAIRS_LOWER
        || sim.family_code == FAMILY_PARTY_HALL
        || sim.family_code == FAMILY_PARTY_HALL_LOWER
    {
        return false;
    }
    sim.state_code == STATE_COMMUTE
        || sim.state_code == STATE_COMMUTE_TRANSIT
        || sim.state_code == STATE_ACTIVE_TRANSIT
        || sim.state_code == STATE_VENUE_TRIP
        || sim.state_code == STATE_VENUE_TRIP_TRANSIT
        || sim.state_code == STATE_DEPARTURE
        || sim.state_code == STATE_DEPARTURE_TRANSIT
        || sim.state_code == STATE_MORNING_TRANSIT
        || sim.state_code == STATE_AT_WORK_TRANSIT
        || sim.state_code == STATE_VENUE_HOME_TRANSIT
        || sim.state_code == STATE_DWELL_RETURN_TRANSIT
}

fn finalize_pending_route_leg(sim: &mut SimRecord) {
    let RouteState::Segment { destination, .. } = sim.route else {
        return;
    };
    if sim.transit_ticks_remaining > 0 {
        return;
    }
    // Phase 1d-ii: office family is now driven by per-stride per-leg re-resolution
    // (resolveSimRouteBetweenFloors writes sim.selectedFloor to the next leg
    // endpoint). Skip the legacy whole-trip teleport for office sims so it
    // doesn't race with per-leg progression. Other families still rely on
    // this finalizer to advance segment trips until they migrate to the
    // per-tick model.
    if sim.family_code == FAMILY_OFFICE {
        return;
    }
    if sim.family_code == FAMILY_CONDO {
        return;
    }
    if sim.family_code == FAMILY_HOTEL_SINGLE
        || sim.family_code == FAMILY_HOTEL_TWIN
        || sim.family_code == FAMILY_HOTEL_SUITE
    {
        return;
    }
    if sim.family_code == FAMILY_RESTAURANT
        || sim.family_code == FAMILY_FAST_FOOD
        || sim.family_code == FAMILY_RETAIL
    {
        return;
    }
    if sim.family_code == FAMILY_CINEMA
        || sim.family_code == FAMILY_CINEMA_LOWER
        || sim.family_code == FAMILY_CINEMA_STAIRS_UPPER
        || sim.family_code == FAMILY_CINEMA_STAIRS_LOWER
        || sim.family_code == FAMILY_PARTY_HALL
        || sim.family_code == FAMILY_PARTY_HALL_LOWER
    {
        return;
    }
    // Transit countdown is handled by reconcileSimTransport; here we just
    // sync selectedFloor once the leg is complete so processXxxSim sees it.
    sim.selected_floor = destination;
}

// Phase 7: `dispatchSimArrival` is now called inline from
// `queue/dispatch-arrivals.ts` (1218:0883 dispatch_destination_queue_entries),
// mirroring the binary's inline family-dispatch at arrival time. The former
// `onCarrierArrival` callback trampoline has been removed.
//
// NOTE: arrival does NOT advance trip counters in the binary —
// `dispatch_destination_queue_entries` writes sim+7 then calls the family
// handler directly (no `dispatch_sim_behavior`, no `advance_sim_trip_counters`).
// All advance sites that would naturally fire as part of an arrival are
// covered by `resolve_sim_route_between_floors` rc=3 inside the per-stride
// state handlers, which is the only legitimate "trip ended" advance for
// segment-based progression.
pub fn dispatch_sim_arrival(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
    arrival_floor: i64,
) {
    let sim = &mut world.sims[sim_index as usize];
    let arrived_via_carrier = matches!(sim.route, RouteState::Carrier { .. });
    sim.selected_floor = arrival_floor;
    clear_sim_route(sim);
    match sim.family_code {
        FAMILY_HOTEL_SINGLE | FAMILY_HOTEL_TWIN | FAMILY_HOTEL_SUITE => {
            handle_hotel_sim_arrival(world, ledger, time, sim_index, arrival_floor);
        }
        FAMILY_OFFICE => {
            if sim.state_code == STATE_MEDICAL_TRIP_TRANSIT {
                handle_medical_sim_arrival(world, sim_index, arrival_floor);
                return;
            }
            handle_office_sim_arrival(world, time, sim_index, arrival_floor, None);
        }
        FAMILY_CONDO => {
            handle_condo_sim_arrival(sim, arrival_floor, time, arrived_via_carrier);
        }
        FAMILY_RESTAURANT | FAMILY_FAST_FOOD | FAMILY_RETAIL => {
            handle_commercial_sim_arrival(world, sim_index, arrival_floor, time);
        }
        FAMILY_HOUSEKEEPING => {
            handle_housekeeping_sim_arrival(world, time, sim_index, arrival_floor);
        }
        FAMILY_CINEMA
        | FAMILY_CINEMA_LOWER
        | FAMILY_CINEMA_STAIRS_UPPER
        | FAMILY_CINEMA_STAIRS_LOWER
        | FAMILY_PARTY_HALL
        | FAMILY_PARTY_HALL_LOWER => {
            handle_entertainment_sim_arrival(world, time, sim_index, arrival_floor);
        }
        family_code => {
            if CATHEDRAL_FAMILIES.contains(&family_code) {
                handle_cathedral_sim_arrival(world, time, sim_index, arrival_floor);
            }
        }
    }
}

// Arrival dispatches inline inside
// `queue/dispatch-arrivals.ts` (`dispatchDestinationQueueEntries` calls
// `dispatchSimArrival` directly on each unloaded slot, matching the binary's
// inline call into `dispatch_object_family_*_state_handler`). Boarding stress
// accumulation was promoted into `queue/process-travel.ts` so
// `accumulate_elapsed_delay_into_current_sim` runs inside the boarding loop
// at 1218:0d4e's binary-equivalent site.

pub fn reconcile_sim_transport(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    // `for (const sim of world.sims)`: by index, the length read each time.
    let mut index = 0usize;
    while index < world.sims.len() {
        let sim_index = index as i64;
        index += 1;
        let sim = &mut world.sims[sim_index as usize];
        let RouteState::Segment { destination, .. } = sim.route else {
            continue;
        };
        if !should_finalize_segment_trip(sim) {
            continue;
        }
        if sim.transit_ticks_remaining > 0 {
            sim.transit_ticks_remaining -= 1;
            continue;
        }
        dispatch_sim_arrival(world, ledger, time, sim_index, destination);
    }

    let mut completed: IndexSet<String> = IndexSet::new();
    for carrier in world.carriers.iter_mut() {
        for route_id in &carrier.completed_route_ids {
            completed.insert(route_id.clone());
        }
        carrier.completed_route_ids = Vec::new();
    }

    let mut index = 0usize;
    while index < world.sims.len() {
        let sim_index = index as i64;
        index += 1;
        let sim = &world.sims[sim_index as usize];
        if sim.destination_floor < 0 {
            continue;
        }
        if !completed.contains(&sim_key(sim)) {
            continue;
        }
        // Office FULL-retry path: `handleOfficeSimArrival` may intentionally
        // leave `destinationFloor` set when `acquire_commercial_venue_slot`
        // returns 2 (capacity FULL), so the per-stride 0x41 handler can
        // re-attempt next stride. The binary mirrors this by NOT re-dispatching
        // the same arrival — `dispatch_destination_queue_entries` (1218:0883)
        // fires the family handler exactly once per arrival. Mirror that here:
        // skip the re-fire if the sim's route was already cleared (mode=idle)
        // OR if the inline handler re-enqueued the sim onto a new carrier for
        // the next transfer leg (mode=carrier). Re-firing in the carrier-mode
        // case would teleport the sim to its FINAL destination, bypassing the
        // next leg entirely (e.g., sky-office sims arriving at the sky lobby
        // would skip the local elevator transfer).
        if matches!(sim.route, RouteState::Idle | RouteState::Carrier { .. }) {
            continue;
        }
        let destination_floor = sim.destination_floor;
        dispatch_sim_arrival(world, ledger, time, sim_index, destination_floor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::create_time_state;
    use crate::world::{RouteDirection, VENUE_AVAILABLE};

    fn sim() -> SimRecord {
        SimRecord {
            floor_anchor: 12,
            home_column: 40,
            base_offset: 1,
            facility_slot: 0,
            family_code: FAMILY_OFFICE,
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
            spawn_floor: 12,
            post_claim_countdown: 0,
            encoded_target_floor: 0,
            commercial_venue_slot: -1,
        }
    }

    fn venue() -> CommercialVenueRecord {
        CommercialVenueRecord {
            owner_subtype_index: 0,
            capacity: 0,
            visit_count: 0,
            today_visit_count: 0,
            yesterday_visit_count: 0,
            acquire_count: 0,
            availability_state: VENUE_AVAILABLE,
            current_population: 0,
            last_acquire_tick: 0,
            eligibility_threshold: 0,
            remaining_capacity: 0,
            phase_a_seed: 0,
            phase_b_seed: 0,
            override_seed: 0,
        }
    }

    #[test]
    fn venue_slot_acquire_and_release() {
        let mut t = create_time_state();
        t.day_tick = 500;
        let mut v = venue();
        let mut s = sim();
        assert_eq!(try_acquire_office_venue_slot(&mut v, &mut s, &t, FAMILY_FAST_FOOD), VENUE_SLOT_ACQUIRED);
        assert_eq!((v.current_population, v.acquire_count, s.last_demand_tick), (1, 1, 500));
        // Same family as the owner: no acquireCount bump.
        assert_eq!(try_acquire_office_venue_slot(&mut v, &mut s, &t, FAMILY_OFFICE), VENUE_SLOT_ACQUIRED);
        assert_eq!((v.current_population, v.acquire_count), (2, 1));
        v.current_population = 40;
        assert_eq!(try_acquire_office_venue_slot(&mut v, &mut s, &t, FAMILY_FAST_FOOD), VENUE_SLOT_FULL);
        v.current_population = 39;
        assert_eq!(try_acquire_office_venue_slot(&mut v, &mut s, &t, FAMILY_FAST_FOOD), VENUE_SLOT_ACQUIRED);
        v.availability_state = VENUE_CLOSED;
        assert_eq!(try_acquire_office_venue_slot(&mut v, &mut s, &t, FAMILY_FAST_FOOD), VENUE_SLOT_UNAVAILABLE);
        assert!(release_office_venue_slot(&mut v, &s, &t, None));
        assert_eq!(v.current_population, 40);
        v.availability_state = VENUE_AVAILABLE;
        // Dwell gate: 500 - 500 < 60.
        assert!(!release_office_venue_slot(&mut v, &s, &t, None));
        assert!(release_office_venue_slot(&mut v, &s, &t, Some(true)));
        assert_eq!(v.current_population, 39);
        v.current_population = 0;
        t.day_tick = 560;
        assert!(release_office_venue_slot(&mut v, &s, &t, None));
        assert_eq!(v.current_population, 0);
    }

    #[test]
    fn venue_dwell_and_trip() {
        let mut t = create_time_state();
        t.day_tick = 100;
        let mut s = sim();
        assert!(!finish_commercial_venue_dwell(&mut s, &t, STATE_ACTIVE));
        s.route = RouteState::Carrier { carrier_id: 1, direction: RouteDirection::Up, source: 12 };
        s.destination_floor = 30;
        s.state_code = STATE_VENUE_TRIP;
        // Not the trip state, or not its destination: no arrival.  (The
        // arrival itself calls clear_sim_route, a stub until population.rs
        // is translated, so the dwell is set up by hand below.)
        assert!(!handle_commercial_venue_arrival(&mut s, 29, 0, &t, None));
        assert!(!handle_commercial_venue_arrival(&mut s, 30, 0, &t, Some(STATE_ACTIVE_TRANSIT)));
        s.state_code = COMMERCIAL_DWELL_STATE;
        s.selected_floor = 30;
        s.destination_floor = -1;
        s.last_demand_tick = 100;
        t.day_tick = 159;
        assert!(finish_commercial_venue_dwell(&mut s, &t, STATE_ACTIVE));
        assert_eq!(s.state_code, COMMERCIAL_DWELL_STATE);
        t.day_tick = 160;
        // venueReturnState 0 is falsy: the default state is taken.
        assert!(finish_commercial_venue_dwell(&mut s, &t, STATE_ACTIVE));
        assert_eq!((s.state_code, s.selected_floor), (STATE_ACTIVE, 12));

        s.state_code = STATE_VENUE_TRIP;
        s.selected_floor = 20;
        s.destination_floor = 30;
        assert!(finish_commercial_venue_trip(&mut s, STATE_DEPARTURE));
        assert_eq!(s.state_code, STATE_VENUE_TRIP);
        s.selected_floor = 30;
        assert!(finish_commercial_venue_trip(&mut s, STATE_DEPARTURE));
        assert_eq!((s.state_code, s.selected_floor, s.destination_floor), (STATE_DEPARTURE, 12, -1));
    }

    #[test]
    fn segment_finalizer_families() {
        let mut s = sim();
        s.route = RouteState::Segment { segment_id: 3, destination: 20 };
        s.family_code = 0x24; // a cathedral guest: legacy whole-trip finalizer
        s.state_code = STATE_COMMUTE;
        assert!(should_finalize_segment_trip(&s));
        finalize_pending_route_leg(&mut s);
        assert_eq!(s.selected_floor, 20);
        s.selected_floor = 12;
        s.family_code = FAMILY_OFFICE;
        assert!(!should_finalize_segment_trip(&s));
        finalize_pending_route_leg(&mut s);
        assert_eq!(s.selected_floor, 12);
    }
}

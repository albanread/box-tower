// ts: sims/office.ts

use crate::ledger::{add_cashflow_from_family_resource, LedgerState};
use crate::progression::add_to_population_bucket;
use crate::queue::resolve::ResolveSimRouteOptions;
use crate::resources::{FAMILY_FAST_FOOD, FAMILY_OFFICE};
use crate::time::TimeState;
use crate::world::{sample_rng, PlacedObjectRecord, RouteState, SimNotification, SimNotificationKind, SimRecord, WorldState};

use super::medical::try_start_medical_trip;
use super::states::{
    COMMERCIAL_DWELL_STATE, COMMERCIAL_VENUE_DWELL_TICKS, LOBBY_FLOOR, NO_EVAL_ENTITY, STATE_ACTIVE,
    STATE_ACTIVE_ALT, STATE_ACTIVE_TRANSIT, STATE_AT_WORK, STATE_AT_WORK_TRANSIT, STATE_COMMUTE,
    STATE_COMMUTE_TRANSIT, STATE_DEPARTURE, STATE_DEPARTURE_TRANSIT, STATE_DWELL_RETURN_TRANSIT,
    STATE_MORNING_GATE, STATE_MORNING_TRANSIT, STATE_NIGHT_A, STATE_NIGHT_B, STATE_PARKED,
    STATE_VENUE_HOME_TRANSIT, STATE_VENUE_TRIP, STATE_VENUE_TRIP_TRANSIT, UNIT_STATUS_OFFICE_OCCUPIED,
};
use super::{
    advance_sim_trip_counters, clear_sim_route, dispatch_commercial_venue_visit,
    find_commercial_venue_at_floor, find_object_for_sim, release_office_venue_slot,
    release_service_request, reset_facility_sim_trip_counters, resolve_sim_route_between_floors,
    try_acquire_office_venue_slot, try_assign_parking_service, venue_and_sim_mut, VENUE_SLOT_FULL,
};

/// The placed object at `key` (a key `find_object_for_sim` gave).
fn object_mut<'a>(world: &'a mut WorldState, key: &str) -> &'a mut PlacedObjectRecord {
    world
        .object_mut(key)
        .unwrap_or_else(|| panic!("no placed object at {key:?}"))
}

/// `{ emitDistanceFeedback: false }`.
fn no_distance_feedback() -> Option<ResolveSimRouteOptions> {
    Some(ResolveSimRouteOptions {
        emit_distance_feedback: Some(false),
        ..Default::default()
    })
}

pub fn advance_office_presence_counter(object: &mut PlacedObjectRecord) {
    if object.object_type_code != FAMILY_OFFICE {
        return;
    }
    if object.unit_status > UNIT_STATUS_OFFICE_OCCUPIED {
        return;
    }
    object.unit_status = if object.unit_status >= 8 { 1 } else { object.unit_status + 1 };
}

fn decrement_office_presence_counter(object: &mut PlacedObjectRecord, time: &TimeState) {
    if object.object_type_code != FAMILY_OFFICE {
        return;
    }
    if object.unit_status > UNIT_STATUS_OFFICE_OCCUPIED {
        return;
    }
    object.unit_status = 0.max(object.unit_status - 1);
    if object.unit_status == 0 && time.daypart_index >= 4 {
        object.unit_status = 8;
    }
}

/// Mirrors binary `activate_office_cashflow` (1180:0d2e) for the
/// `is_reopening==0` path called from the per-tick state-0x20 handler at
/// 1228:2329 / 1228:23a5. The cashflow side (`add_cashflow_from_family_resource`)
/// is owned in TS by the 3-day rollover guard in `handleOfficeMorningGate`,
/// so this routine only flips `unitStatus`/`dirtyFlag` and contributes
/// +6 to the primary family ledger total used by star advancement.
fn activate_office_cashflow(world: &mut WorldState, object: &str, sim_index: i64) {
    let o = object_mut(world, object);
    if o.unit_status <= UNIT_STATUS_OFFICE_OCCUPIED {
        return;
    }
    o.unit_status = 0;
    // Binary 1180:0d93: MOV byte ptr ES:[BX+0x13],1 (dirty flag set).
    o.dirty_flag = 1;
    // Binary: add_to_primary_family_ledger_bucket(7, 6) — population
    // contribution that feeds compute_tower_tier_from_ledger and gates
    // star advancement (e.g. 1→2 at total >= 300).
    add_to_population_bucket(world, FAMILY_OFFICE, 6);

    reset_facility_sim_trip_counters(world, sim_index);
}

fn route_failure_state_for_office(object: &PlacedObjectRecord) -> i64 {
    if object.unit_status > UNIT_STATUS_OFFICE_OCCUPIED {
        STATE_MORNING_GATE
    } else {
        STATE_NIGHT_A
    }
}

fn fail_office_route(world: &mut WorldState, sim_index: i64, failure_state: i64) {
    release_service_request(world, sim_index);
    world.sims[sim_index as usize].state_code = failure_state;
}

fn finalize_office_floor_arrival(sim: &mut SimRecord, object: Option<&mut PlacedObjectRecord>, next_state: i64) {
    if let Some(object) = object {
        advance_office_presence_counter(object);
    }
    sim.destination_floor = -1;
    sim.selected_floor = sim.floor_anchor;
    sim.venue_return_state = 0;
    sim.state_code = next_state;
}

/// `finalizeOfficeFloorArrival(sim, object, nextState)` with the sim and its
/// (optional) object reached through the world.
fn finalize_office_floor_arrival_in(world: &mut WorldState, sim_index: i64, object: Option<&str>, next_state: i64) {
    let WorldState { sims, placed_objects, detached_objects, .. } = world;
    let object = object.map(|key| {
        // world.object_mut, with the sims borrowed beside it
        if key.starts_with(crate::world::DETACHED_PREFIX) { detached_objects.get_mut(key) } else { placed_objects.get_mut(key) }
            .unwrap_or_else(|| panic!("no placed object at {key:?}"))
    });
    finalize_office_floor_arrival(&mut sims[sim_index as usize], object, next_state);
}

fn next_office_morning_state(world: &mut WorldState, sim_index: i64) -> i64 {
    // Binary 1228:213c AX=3 branch (same-floor arrival) at 1228:23bb:
    //   if (base_offset == 0) state := 0x00 (STATE_COMMUTE)   ; 1228:2618+2639
    //   else if roll_office_sim_medical_trip_today() → 0x02    ; 1228:23f5
    //   else → 0x01                                            ; 1228:241e
    // roll is `starCount >= 3 && sample_lcg15() % 10 == 0` (1178:0635).
    // RNG is only consumed on the non-base-0 branch, mirroring the binary.
    if world.sims[sim_index as usize].base_offset == 0 {
        return STATE_COMMUTE;
    }
    if world.star_count >= 3 && sample_rng(world) % 10 == 0 {
        return STATE_ACTIVE_ALT;
    }
    STATE_ACTIVE
}

pub fn next_office_return_state(sim: &SimRecord) -> i64 {
    if sim.base_offset == 1 {
        STATE_COMMUTE
    } else {
        STATE_DEPARTURE
    }
}

fn run_office_service_evaluation(world: &mut WorldState, time: &TimeState, sim_index: Option<i64>, object: Option<&str>) {
    if world.star_count != 3 || time.day_counter % 9 != 3 {
        return;
    }
    if world.gate_flags.office_service_ok != 0 {
        return;
    }
    if world.gate_flags.eval_sim_index >= 0 && world.gate_flags.eval_sim_index != NO_EVAL_ENTITY {
        return;
    }
    let (Some(sim_index), Some(object)) = (sim_index, object) else {
        return;
    };
    let sim = &world.sims[sim_index as usize];
    if sim.family_code != FAMILY_OFFICE || sim.state_code != STATE_ACTIVE {
        return;
    }
    if world.object(object).unwrap().eval_level <= 0 {
        return;
    }
    world.gate_flags.office_service_ok = 1;
}

// --- Per-state handlers (1228:1e45 etc.) ---

/// 1228:1e45 office_refresh_0x00 — STATE_COMMUTE handler.
///
/// Binary state-0 dispatch issues an INBOUND (lobby → anchor) route via
/// 1228:2644 with direction=1, but the workaround for baseOffset=1 keeps
/// an OUTBOUND path here for post-lunch returns (treating state 0x00 via
/// the 0x62 alias that reaches this refresh). The specific sky_office
/// regression was the OUTBOUND path firing for baseOffset=0 at daypart 0
/// (sim on floor 11 enqueuing src=11 → dst=10). Restrict the OUTBOUND
/// shortcut to baseOffset=1 only; baseOffset=0 takes the INBOUND path,
/// matching the binary's state-0 handler.
fn handle_office_commute(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, facility: &str) {
    if time.daypart_index >= 4 {
        world.sims[sim_index as usize].state_code = STATE_DEPARTURE;
        return;
    }
    if world.sims[sim_index as usize].base_offset == 0 {
        if time.daypart_index == 0 && sample_rng(world) % 12 != 0 {
            return;
        }
    } else {
        if time.daypart_index < 3 {
            return;
        }
        if sample_rng(world) % 12 != 0 {
            return;
        }
    }
    // Binary 1228:1e45 / 1228:266b: state-0 dispatch calls 1218:0000 with
    // source=floor_anchor, target=LOBBY (hardcoded 10) — i.e. OUTBOUND.
    // Confirmed via dynamic trace of dense_office tick=150 sim=150 fA=13
    // (state_before=0x00 family=7 source_floor=13 target_floor=10).
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        floor_anchor,
        LOBBY_FLOOR,
        if LOBBY_FLOOR > floor_anchor { 1 } else { 0 },
        Some(time),
        None,
    );
    if route_result == -1 {
        world.sims[sim_index as usize].state_code = STATE_NIGHT_B;
        return;
    }
    decrement_office_presence_counter(object_mut(world, facility), time);
    if route_result == 3 {
        advance_office_presence_counter(object_mut(world, facility));
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_AT_WORK;
    } else {
        world.sims[sim_index as usize].state_code = STATE_COMMUTE_TRANSIT;
    }
}

/// 1228:1ed5 office_refresh_0x01/0x02 — venue selection (STATE_ACTIVE / STATE_ACTIVE_ALT).
fn handle_office_active(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, facility: &str) {
    run_office_service_evaluation(world, time, Some(sim_index), Some(facility));
    if time.daypart_index >= 4 {
        if try_start_medical_trip(world, time, sim_index) {
            return;
        }
        let sim = &mut world.sims[sim_index as usize];
        sim.state_code = STATE_DEPARTURE;
        sim.destination_floor = LOBBY_FLOOR;
        sim.selected_floor = sim.floor_anchor;
        return;
    }
    if time.daypart_index == 0 {
        return;
    }
    if time.daypart_index == 1 && sample_rng(world) % 12 != 0 {
        return;
    }

    let dispatched = dispatch_commercial_venue_visit(
        world,
        time,
        sim_index,
        &[FAMILY_FAST_FOOD],
        STATE_AT_WORK,
        Some(STATE_ACTIVE_TRANSIT),
        None,
        Some(true),
        Some(true),
        None,
    );
    if dispatched && world.sims[sim_index as usize].state_code == COMMERCIAL_DWELL_STATE {
        // Binary route_sim_to_commercial_venue (1238:022a) writes state 0x22
        // (STATE_VENUE_TRIP) when same-floor route + acquire_slot both succeed.
        // dispatchCommercialVenueVisit's same-floor success path now calls
        // `tryAcquireOfficeVenueSlot` itself (mirroring the binary state-0x01
        // resolve-rc=3 acquire), and on FULL it writes STATE_ACTIVE_TRANSIT
        // directly. Here we only need to override the post-acquire dwell state
        // from 0x62 to 0x22 to match the binary's same-floor success target.
        let sim = &mut world.sims[sim_index as usize];
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        return;
    }
    if !dispatched {
        // Spec §No Fast Food Available: route to lobby for fake lunch round-trip.
        // Worker travels to lobby, dwells, returns to office — never gets stuck.
        let floor_anchor = world.sims[sim_index as usize].floor_anchor;
        let route_result = resolve_sim_route_between_floors(
            world,
            sim_index,
            floor_anchor,
            LOBBY_FLOOR,
            if LOBBY_FLOOR > floor_anchor { 1 } else { 0 },
            Some(time),
            None,
        );
        if route_result == -1 {
            // Spec §Route to Lobby Fails: fake-transit sentinel → eventually
            // advance_office_presence_counter → STATE_DEPARTURE (0x05).
            advance_office_presence_counter(object_mut(world, facility));
            world.sims[sim_index as usize].state_code = STATE_DEPARTURE;
            return;
        }
        // Phase 1d-ii: resolve owns sim.selectedFloor/destinationFloor.
        let sim = &mut world.sims[sim_index as usize];
        if route_result == 3 {
            // Office on lobby floor — start venue dwell immediately.
            sim.venue_return_state = 0;
            sim.state_code = STATE_VENUE_TRIP;
            sim.selected_floor = LOBBY_FLOOR;
            sim.destination_floor = -1;
            sim.last_demand_tick = time.day_tick;
            clear_sim_route(sim);
        } else {
            // In-transit to lobby for fake lunch; arrival promotes to 0x22.
            sim.state_code = STATE_ACTIVE_TRANSIT;
        }
    }
}

/// 1228:1fac office_refresh_0x05 — evening departure (STATE_DEPARTURE).
fn handle_office_departure(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, facility: &str) {
    if time.daypart_index < 4 {
        return;
    }
    if time.daypart_index == 4 && sample_rng(world) % 6 != 0 {
        return;
    }
    decrement_office_presence_counter(object_mut(world, facility), time);
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    // BOX: metro -- under BOX RULES a rider goes home by the station.
    let exit = crate::box_ext::locals::exit_floor(world, sim_index)
        .or_else(|| crate::box_ext::metro::exit_floor(world, sim_index))
        .unwrap_or(LOBBY_FLOOR);
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        floor_anchor,
        exit,
        if exit != LOBBY_FLOOR && exit > floor_anchor { 1 } else { 0 },
        Some(time),
        None,
    );
    if route_result == -1 {
        fail_office_route(world, sim_index, STATE_NIGHT_B);
        return;
    }
    // Phase 1d-ii: resolve owns sim.selectedFloor/destinationFloor.
    if route_result == 3 {
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_PARKED;
        release_service_request(world, sim_index);
    } else {
        world.sims[sim_index as usize].state_code = STATE_DEPARTURE_TRANSIT;
    }
}

/// 1228:1dc1 office_refresh_0x20 — morning activation gate (STATE_MORNING_GATE).
fn handle_office_morning_gate(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, facility: &str) {
    if time.weekend_flag != 0 {
        return;
    }
    // Binary 1228:1df3: CMP byte ptr ES:[BX+0x14],0x0 — reads the occupied
    // flag (set only by the scoring sweep's first eval_level>0 pass). Sims
    // whose home facility has never been scored must NOT sample RNG here.
    if world.object(facility).unwrap().occupied_flag == 0 {
        return;
    }

    if time.daypart_index >= 3 {
        return;
    }
    if time.daypart_index == 0 && sample_rng(world) % 12 != 0 {
        return;
    }

    let sim = &world.sims[sim_index as usize];
    let mut by_car = false;
    // BOX: car elevator -- under BOX RULES a driver drives every working day
    // (upstream: only while the office's stay phase is 2, some days and
    // not others).
    let drives_today = if crate::box_ext::box_rules(world) {
        world.object(facility).unwrap().unit_status <= UNIT_STATUS_OFFICE_OCCUPIED
    } else {
        world.object(facility).unwrap().unit_status == 2
    };
    if world.star_count > 2 && (sim.floor_anchor + sim.home_column) % 4 == 1 && drives_today {
        if try_assign_parking_service(world, time, sim_index) {
            by_car = true;
        } else {
            world.pending_notifications.push(SimNotification {
                kind: SimNotificationKind::RouteFailure,
                message: Some("Office workers demand Parking".to_string()),
            });
        }
    }
    // BOX: car elevator -- under BOX RULES a commuter whose car has a space
    // drives in first: held until the car has parked, then sets off from its
    // floor (box_ext::car_elevator::commute).
    if by_car && crate::box_ext::car_elevator::hold_commuter(world, time, sim_index, facility) {
        return;
    }
    // BOX: locals, metro -- under BOX RULES those who live in the tower
    // come from home, and some come by train, from the station.
    let from = crate::box_ext::locals::arrival_floor(world, sim_index)
        .or_else(|| crate::box_ext::metro::arrival_floor(world, sim_index))
        .unwrap_or(LOBBY_FLOOR);
    office_set_off(world, ledger, time, sim_index, facility, from);
}

/// The rest of the morning gate: the commuter sets off from `source_floor`
/// (the lobby; under BOX RULES their car's floor) to the office.
pub(crate) fn office_set_off(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
    facility: &str,
    source_floor: i64,
) {
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        floor_anchor,
        if floor_anchor > source_floor { 1 } else { 0 },
        Some(time),
        None,
    );
    if route_result == -1 {
        // Binary 1228:225f/2285/22ac: the office state-0x20 handler's rc=-1
        // branch zeroes tripCount / elapsedTicks / accumulatedTicks before
        // exiting. `resolve_sim_route_between_floors` on no-route fires the
        // 300-tick penalty + advanceSimTripCounters inside, and the handler
        // immediately wipes the side effects. Without this reset, stuck
        // morning-gate sims accrue spurious stress.
        let state = route_failure_state_for_office(&world.object(facility).unwrap());
        let sim = &mut world.sims[sim_index as usize];
        sim.trip_count = 0;
        sim.elapsed_ticks = 0;
        sim.accumulated_ticks = 0;
        sim.state_code = state;
        return;
    }
    // Binary 1228:213c/22de+2329 (cases 1/2/3) and 235a+23a5 (case 4): after
    // a non-failure route result, call activate_office_cashflow (1180:0d2e)
    // which pays rent when stayPhase (unitStatus) >= 0x10 and zeros it.
    // The 3-day cycle gate is implicit in the stayPhase counter: rent zeroes
    // it, and sync_stay_phase_if_all_siblings_ready brings it back to 0x10
    // over the following days. The auxValueOrTimer + dayCounter%3 guard on
    // the paired activateThreeDayCashflow (ledger.ts) still catches facilities
    // that skipped rent here because their per-sim route failed earlier in
    // the cycle.
    let f = object_mut(world, facility);
    if f.aux_value_or_timer != time.day_counter + 1 && time.day_counter % 3 == 0 {
        f.aux_value_or_timer = time.day_counter + 1;
        // Binary activate_office_cashflow 1180:0d93 sets +0x13 (dirty).
        f.dirty_flag = 1;
        add_cashflow_from_family_resource(ledger, "office", f.rent_level, f.object_type_code);
    }
    activate_office_cashflow(world, facility, sim_index);
    // Phase 1d-ii: resolve owns sim+7/sim+0x12. Handler sets state only.
    if route_result == 0 || route_result == 1 || route_result == 2 {
        world.sims[sim_index as usize].state_code = STATE_MORNING_TRANSIT;
        return;
    }
    advance_office_presence_counter(object_mut(world, facility));
    let sim = &mut world.sims[sim_index as usize];
    sim.destination_floor = -1;
    sim.selected_floor = sim.floor_anchor;
    let next_state = next_office_morning_state(world, sim_index);
    world.sims[sim_index as usize].state_code = next_state;
}

/// 1228:1f33 office_refresh_0x21 — at office, ready for venue visits (STATE_AT_WORK).
fn handle_office_at_work(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, facility: &str) {
    if time.daypart_index >= 4 {
        let sim = &mut world.sims[sim_index as usize];
        sim.state_code = STATE_PARKED;
        sim.destination_floor = -1;
        clear_sim_route(sim);
        release_service_request(world, sim_index);
        return;
    }
    if time.daypart_index == 3 {
        if sample_rng(world) % 12 != 0 {
            return;
        }
    } else {
        return;
    }

    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        LOBBY_FLOOR,
        floor_anchor,
        if floor_anchor > LOBBY_FLOOR { 1 } else { 0 },
        Some(time),
        None,
    );
    if route_result == -1 {
        fail_office_route(world, sim_index, STATE_NIGHT_B);
        return;
    }
    // Phase 1d-ii: resolve owns sim+7/sim+0x12.
    if route_result == 3 {
        finalize_office_floor_arrival_in(world, sim_index, Some(facility), STATE_DEPARTURE);
    } else {
        world.sims[sim_index as usize].state_code = STATE_AT_WORK_TRANSIT;
    }
}

/// 1228:1f62 office_refresh_0x22/0x23 — at venue / routing home
/// (STATE_VENUE_TRIP / STATE_VENUE_HOME_TRANSIT).
fn handle_office_venue_trip(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, facility: &str) {
    if time.daypart_index >= 4 {
        // Late-day forced park: release the venue slot so the venue's
        // currentPopulation tracks workers leaving (binary
        // release_commercial_venue_slot, 11b0:0fae, called from the same
        // state handler before the park transition).
        let selected_floor = world.sims[sim_index as usize].selected_floor;
        if let Some(venue) = find_commercial_venue_at_floor(world, selected_floor, &[FAMILY_FAST_FOOD]) {
            let (venue, sim) = venue_and_sim_mut(world, venue, sim_index);
            release_office_venue_slot(venue, sim, time, Some(true));
        }
        world.sims[sim_index as usize].state_code = STATE_PARKED;
        release_service_request(world, sim_index);
        return;
    }
    if time.daypart_index < 2 {
        return;
    }
    let sim = &world.sims[sim_index as usize];
    let is_fake_lunch = sim.selected_floor == LOBBY_FLOOR;
    if sim.state_code == STATE_VENUE_TRIP && !is_fake_lunch && time.day_tick - sim.queue_tick < COMMERCIAL_VENUE_DWELL_TICKS {
        return;
    }
    // Binary route_sim_back_from_commercial_venue (1238:0244) calls
    // release_commercial_venue_slot (11b0:0fae) BEFORE resolving the return
    // route. Mirror the decrement here. Skip the dwell gate inside
    // releaseOfficeVenueSlot because the caller's `dayTick - queueTick >=
    // COMMERCIAL_VENUE_DWELL_TICKS` check above already enforces it.
    if sim.state_code == STATE_VENUE_TRIP && !is_fake_lunch {
        let selected_floor = sim.selected_floor;
        if let Some(venue) = find_commercial_venue_at_floor(world, selected_floor, &[FAMILY_FAST_FOOD]) {
            let (venue, sim) = venue_and_sim_mut(world, venue, sim_index);
            release_office_venue_slot(venue, sim, time, Some(true));
        }
    }
    let sim = &world.sims[sim_index as usize];
    let (selected_floor, floor_anchor) = (sim.selected_floor, sim.floor_anchor);
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        selected_floor,
        floor_anchor,
        if floor_anchor > selected_floor { 1 } else { 0 },
        Some(time),
        None,
    );
    if route_result == -1 {
        fail_office_route(world, sim_index, STATE_NIGHT_B);
        return;
    }
    // Phase 1d-ii: resolve owns sim.selectedFloor/destinationFloor.
    // Binary 1238:0244 (route_sim_back_from_commercial_venue): after the
    // resolve_sim_route_between_floors call, only the state byte (sim+5) is
    // written; sim+0xa (last_trip_tick, our `lastDemandTick`) is left at the
    // dayTick value the resolver's carrier-enqueue branch (returning 2)
    // stamped. The boarding-time `accumulate_elapsed_delay_into_current_sim`
    // later folds (boardingTick - dayTick) into elapsed_packed so the wait
    // time on the carrier queue is counted into stress. Don't clobber lt
    // here — the previous `sim.lastDemandTick = -1;` made the boarding rebase
    // a no-op and lost ~4 ticks per return trip on dense_office (sim 2 at
    // d0t802→d0t806 was missing 4 ticks vs the binary trace).
    if route_result == 3 {
        let next_state = next_office_return_state(&world.sims[sim_index as usize]);
        finalize_office_floor_arrival_in(world, sim_index, Some(facility), next_state);
    } else {
        world.sims[sim_index as usize].state_code = STATE_VENUE_HOME_TRANSIT;
    }
}

/// 1228:1d8e office_refresh_0x25/0x26/0x27 — night/failure park states.
fn handle_office_night_park(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _facility: &str) {
    if time.day_tick > 2300 {
        world.sims[sim_index as usize].state_code = STATE_MORNING_GATE;
    }
}

/// Per-stride in-transit handler. Binary state-handler jump table for the +0x40
/// transit aliases (0x40/0x42/0x45/0x61/0x63) reuses the base handler (0x00/0x02/
/// 0x05/0x21/0x23), each of which calls resolve_sim_route_between_floors per
/// stride. For an in-segment sim, that resolve advances sim+7 by one leg; when
/// sim+7 reaches the target, resolve returns 3 (same-floor, advance fires inside
/// resolve) and the handler triggers state transition. We pass
/// `alreadyAdvanced=true` to `handleOfficeSimArrival` because the resolve call
/// above already invoked `advance_sim_trip_counters`.
fn handle_office_transit(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _facility: &str) {
    let sim = &world.sims[sim_index as usize];
    if matches!(sim.route, RouteState::Carrier { .. }) {
        return;
    }
    if sim.destination_floor < 0 {
        return;
    }
    let source_floor = sim.selected_floor;
    let target_floor = sim.destination_floor;
    // Alias state (+0x40 transit handler): in the binary `emit_distance_feedback`
    // is `0` (the comparison `current_state == base_state` is false here), so
    // distance feedback was already applied by the base state when the trip
    // began. Suppress here to avoid double-counting per-leg. `is_passenger_route`
    // is still `1` for office.
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        no_distance_feedback(),
    );
    if route_result == 3 {
        // Arrived. Trip counter already advanced inside resolve (same-floor
        // branch at 1218:0046). Skip `dispatchSimArrival` so we can pass
        // `alreadyAdvanced=true` and avoid double-counting; mirror the small
        // pieces dispatchSimArrival does (selectedFloor, clearSimRoute) inline.
        let sim = &mut world.sims[sim_index as usize];
        sim.selected_floor = target_floor;
        clear_sim_route(sim);
        handle_office_sim_arrival(world, time, sim_index, target_floor, Some(true));
    }
}

/// 1228:213c morning-transit handler (STATE_MORNING_TRANSIT 0x60). Alias of
/// state 0x20 in the binary jump table, but with `variantFlag=0` (distance
/// feedback off). Per binary AX=3 branch (1228:23bb), the same medical-roll
/// dispatch fires for both 0x20 and 0x60: rc=3 → ACTIVE_ALT or ACTIVE (or
/// STATE_COMMUTE for baseOffset==0).
///   rc=-1 → release + state per routeFailureStateForOffice
///   rc=0/1/2 → state 0x60 (stays in transit; next stride re-resolves)
///   rc=3 → nextOfficeMorningState
/// The variant flag affects `add_delay_to_current_sim` distance feedback, but
/// our resolver applies the penalty unconditionally — accept the slight stress
/// mismatch for now (Phase 4 task constraint).
fn handle_office_morning_transit_retry(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, facility: &str) {
    // Source is the sim's current floor (binary sim+7). For idle/queue-full sims,
    // selectedFloor is the home floor; for in-segment sims, selectedFloor is the
    // last leg endpoint.
    let source_floor = world.sims[sim_index as usize].selected_floor;
    let target_floor = world.sims[sim_index as usize].floor_anchor;
    // Alias state 0x60 (MORNING_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here (current_state 0x60 != base_state 0x20). Distance feedback was
    // already applied by the base state 0x20 dispatch.
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        no_distance_feedback(),
    );
    if route_result == -1 {
        world.sims[sim_index as usize].state_code = route_failure_state_for_office(&world.object(facility).unwrap());
        release_service_request(world, sim_index);
        return;
    }
    if route_result == 3 {
        // Trip arrived at target floor; resolve already advanced trip counters.
        advance_office_presence_counter(object_mut(world, facility));
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        let next_state = next_office_morning_state(world, sim_index);
        world.sims[sim_index as usize].state_code = next_state;
    }
    // rc 0/1/2: stay in STATE_MORNING_TRANSIT; next stride will re-resolve.
}

/// Upstream's `(world, ledger, time, sim, facility)`: the sim by its index
/// in `world.sims`, the facility by its key in `world.placed_objects`.
pub type OfficeHandler = fn(&mut WorldState, &mut LedgerState, &TimeState, i64, &str);

/// cs:2005 refresh-state dispatch table (state_code → handler).
///
/// (A constant `Map`: pairs in insertion order; `js::record_get` looks one up.)
pub const OFFICE_REFRESH_HANDLER_TABLE: &[(i64, OfficeHandler)] = &[
    (STATE_COMMUTE, handle_office_commute),             // 0x00 → 1228:1e45
    (STATE_ACTIVE, handle_office_active),               // 0x01 → 1228:1ed5
    (STATE_ACTIVE_ALT, handle_office_active),           // 0x02 → 1228:1ed5 (same handler)
    (STATE_DEPARTURE, handle_office_departure),         // 0x05 → 1228:1fac
    (STATE_MORNING_GATE, handle_office_morning_gate),   // 0x20 → 1228:1dc1
    (STATE_AT_WORK, handle_office_at_work),             // 0x21 → 1228:1f33
    (STATE_VENUE_TRIP, handle_office_venue_trip),       // 0x22 → 1228:1f62
    (STATE_NIGHT_A, handle_office_night_park),          // 0x25 → 1228:1d8e
    (STATE_NIGHT_B, handle_office_night_park),          // 0x26 → 1228:1d8e (same handler)
    (STATE_PARKED, handle_office_night_park),           // 0x27 → 1228:1d8e (same handler)
    (STATE_COMMUTE_TRANSIT, handle_office_transit),     // 0x40
    (STATE_ACTIVE_TRANSIT, handle_office_transit),      // 0x41
    (STATE_VENUE_TRIP_TRANSIT, handle_office_transit),  // 0x42
    (STATE_DEPARTURE_TRANSIT, handle_office_transit),   // 0x45
    (STATE_MORNING_TRANSIT, handle_office_morning_transit_retry), // 0x60 → 1228:213c alias
    (STATE_AT_WORK_TRANSIT, handle_office_transit),     // 0x61
    (STATE_VENUE_HOME_TRANSIT, handle_office_transit),  // 0x62
    (STATE_DWELL_RETURN_TRANSIT, handle_office_transit), // 0x63
];

pub fn process_office_sim(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64) {
    let Some(facility) = find_object_for_sim(world, sim_index) else {
        return;
    };

    let handler = crate::js::record_get(OFFICE_REFRESH_HANDLER_TABLE, world.sims[sim_index as usize].state_code);
    if let Some(handler) = handler {
        handler(world, ledger, time, sim_index, &facility);
    }
}

/// Carrier/segment arrival dispatch for office sims (called from
/// `dispatch_destination_queue_entries` 1218:0883 → office state handler, and
/// also from `handleOfficeTransit` after a per-stride segment-leg arrival).
///
/// Binary behavior: at carrier arrival, the office state-code dispatcher
/// (1228:2031) jumps to the per-state handler for the alias state (0x40-0x45,
/// 0x60-0x63). Each handler invokes `resolve_sim_route_between_floors` with
/// `is_passenger_route=1` (every binary call site pushes 0x1). For alias
/// states, `source = sim+7 = arrival_floor` and `target = anchor / lobby /
/// venue floor` matching the trip's destination. When source == target, the
/// resolve same-floor branch (1218:0046) calls `advance_sim_trip_counters`
/// because is_passenger_route != 0. We mirror that advance directly here
/// for the carrier-arrival path (which bypasses the per-state-handler
/// resolve).
///
/// `alreadyAdvanced=true` is passed by `handleOfficeTransit` because the
/// per-stride resolve already invoked `advance_sim_trip_counters` inside its
/// same-floor (rc=3) branch. The caller passes `true` to avoid a double
/// advance. The carrier-arrival path leaves it `false` so this routine
/// fires the advance itself.
pub fn handle_office_sim_arrival(
    world: &mut WorldState,
    time: &TimeState,
    sim_index: i64,
    arrival_floor: i64,
    already_advanced: Option<bool>,
) {
    let already_advanced = already_advanced.unwrap_or(false);
    let object = find_object_for_sim(world, sim_index);
    let advance_if_needed = |sim: &mut SimRecord| {
        if !already_advanced {
            advance_sim_trip_counters(sim);
        }
    };

    let sim = &world.sims[sim_index as usize];
    if sim.state_code == STATE_MORNING_TRANSIT && arrival_floor == sim.floor_anchor {
        // 1228:213c alias 0x60 of 0x20 — resolve same-floor advance fires.
        advance_if_needed(&mut world.sims[sim_index as usize]);
        let next_state = next_office_morning_state(world, sim_index);
        finalize_office_floor_arrival_in(world, sim_index, object.as_deref(), next_state);
        return;
    }

    if sim.state_code == STATE_AT_WORK_TRANSIT && arrival_floor == sim.floor_anchor {
        // 1228:2429 alias 0x61 of 0x21 — resolve same-floor advance fires.
        advance_if_needed(&mut world.sims[sim_index as usize]);
        finalize_office_floor_arrival_in(world, sim_index, object.as_deref(), STATE_DEPARTURE);
        return;
    }

    if (sim.state_code == STATE_VENUE_HOME_TRANSIT || sim.state_code == STATE_DWELL_RETURN_TRANSIT)
        && arrival_floor == sim.floor_anchor
    {
        // 1228:24cd / 1228:2505 alias 0x62/0x63 — route_sim_back_from_commercial_venue
        // (1238:0244) calls resolve with `is_passenger_route=1` (1238:02fa PUSH 0x1).
        advance_if_needed(&mut world.sims[sim_index as usize]);
        let next_state = next_office_return_state(&world.sims[sim_index as usize]);
        finalize_office_floor_arrival_in(world, sim_index, object.as_deref(), next_state);
        return;
    }

    // BOX: metro, locals -- or a rider at the station, a resident at home.
    if sim.state_code == STATE_DEPARTURE_TRANSIT
        && (arrival_floor == LOBBY_FLOOR
            || crate::box_ext::metro::is_exit(world, sim_index, arrival_floor)
            || crate::box_ext::locals::is_exit(world, sim_index, arrival_floor))
    {
        // 1228:2980 alias 0x45 of 0x05 — resolve same-floor advance fires.
        let sim = &mut world.sims[sim_index as usize];
        advance_if_needed(sim);
        sim.state_code = STATE_PARKED;
        sim.selected_floor = LOBBY_FLOOR;
        release_service_request(world, sim_index);
        return;
    }

    if sim.state_code == STATE_COMMUTE_TRANSIT && arrival_floor == sim.floor_anchor {
        // 1228:2644 alias 0x40 of 0x00 — resolve same-floor advance fires when
        // source (sim+7=arrival) == target (lobby for baseOffset==0, anchor for
        // baseOffset>0).
        advance_if_needed(&mut world.sims[sim_index as usize]);
        finalize_office_floor_arrival_in(world, sim_index, object.as_deref(), STATE_AT_WORK);
        return;
    }

    if sim.state_code == STATE_COMMUTE_TRANSIT && sim.base_offset <= 1 && arrival_floor == LOBBY_FLOOR {
        // 1228:2644 alias 0x40 of 0x00 outbound lobby leg for occupants 0/1
        // reaches the post-commute office path.
        let sim = &mut world.sims[sim_index as usize];
        advance_if_needed(sim);
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_AT_WORK;
        return;
    }

    // Arrival while in ACTIVE_TRANSIT (0x41) or VENUE_TRIP_TRANSIT (0x42) —
    // binary re-runs the 0x01/0x41 (or 0x02/0x42) state handler at arrival via
    // `dispatch_destination_queue_entries` (1218:0883), which calls
    // `route_sim_to_commercial_venue` (1238:0000). That helper resolves
    // floor→floor (rc=3 same-floor since arrival floor == venue floor), then
    // calls `acquire_commercial_venue_slot` (11b0:0d92):
    //   - rc=3 (slot acquired): writes entity[+5] = 0x22 (STATE_VENUE_TRIP)
    //   - rc=2 (full, currentPopulation > 39): JMP caseD_0 → entity[+5] = 0x41
    //     (stays in STATE_ACTIVE_TRANSIT; the per-stride 0x41 handler will
    //     re-attempt acquire next stride until a slot opens up)
    //   - rc=-1 (closed/dormant): falls through to writing 0x22, but the
    //     subsequent 0x22 release handler short-circuits via the same gates.
    //
    // queueTick records the arrival time so the 60-tick dwell gate in
    // processOfficeSim can block correctly. We use queueTick rather than
    // lastDemandTick because the stride rebase clears lastDemandTick to -1
    // on every call, which would break the dwell gate.
    //
    // Both aliases advance trip counters at arrival via the resolve same-floor
    // branch (1218:0046, is_passenger_route=1).
    if (sim.state_code == STATE_ACTIVE_TRANSIT || sim.state_code == STATE_VENUE_TRIP_TRANSIT)
        && (sim.destination_floor < 0 || arrival_floor == sim.destination_floor)
    {
        let sim = &mut world.sims[sim_index as usize];
        advance_if_needed(sim);
        sim.selected_floor = arrival_floor;
        // Real fast-food venue lookup. The lobby fallback path (sim being
        // routed to LOBBY_FLOOR because no venue was available) is identified
        // by selectedFloor == LOBBY_FLOOR; for that path the binary's
        // `acquire_commercial_venue_slot` short-circuits at
        // `facility_slot_index < 0` and returns 3 (no capacity check). Mirror
        // that by skipping the lookup and going straight to STATE_VENUE_TRIP.
        let venue = if arrival_floor == LOBBY_FLOOR {
            None
        } else {
            find_commercial_venue_at_floor(world, arrival_floor, &[FAMILY_FAST_FOOD])
        };
        if let Some(venue) = venue {
            // Office worker arriving at a fast-food venue: pass the venue
            // owner's family (FAMILY_FAST_FOOD) so the type/variant gate fires
            // (sim.familyCode = FAMILY_OFFICE differs) and acquireCount is
            // bumped on success — mirroring binary 11b0:0f3a–0f55.
            let (venue, sim) = venue_and_sim_mut(world, venue, sim_index);
            let result = try_acquire_office_venue_slot(venue, sim, time, FAMILY_FAST_FOOD);
            if result == VENUE_SLOT_FULL {
                // Stay in STATE_ACTIVE_TRANSIT; the per-stride 0x41 handler
                // will re-attempt acquire next stride. Don't clear
                // destinationFloor — the binary keeps entity[+6] (venue index)
                // intact and re-runs the handler.
                return;
            }
            // VENUE_SLOT_ACQUIRED or VENUE_SLOT_UNAVAILABLE → state 0x22.
        }
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        return;
    }

    // Carrier dropped the sim at a transfer point (e.g. sky lobby) on the way
    // to the anchor / lobby. Binary 1228:213c (and the analogous handlers for
    // other transit aliases) re-resolves the next leg from the arrival floor.
    // Skip distance feedback — the base state already applied it on trip start.
    if sim.state_code == STATE_MORNING_TRANSIT
        || sim.state_code == STATE_AT_WORK_TRANSIT
        || sim.state_code == STATE_VENUE_HOME_TRANSIT
        || sim.state_code == STATE_DWELL_RETURN_TRANSIT
        || sim.state_code == STATE_DEPARTURE_TRANSIT
        || sim.state_code == STATE_COMMUTE_TRANSIT
        || sim.state_code == STATE_ACTIVE_TRANSIT
        || sim.state_code == STATE_VENUE_TRIP_TRANSIT
    {
        let target_floor = if sim.state_code == STATE_MORNING_TRANSIT
            || sim.state_code == STATE_AT_WORK_TRANSIT
            || sim.state_code == STATE_VENUE_HOME_TRANSIT
            || sim.state_code == STATE_DWELL_RETURN_TRANSIT
        {
            sim.floor_anchor
        } else if sim.state_code == STATE_ACTIVE_TRANSIT || sim.state_code == STATE_VENUE_TRIP_TRANSIT {
            sim.destination_floor
        } else {
            LOBBY_FLOOR
        };
        world.sims[sim_index as usize].selected_floor = arrival_floor;
        let route_result = resolve_sim_route_between_floors(
            world,
            sim_index,
            arrival_floor,
            target_floor,
            if target_floor > arrival_floor { 1 } else { 0 },
            Some(time),
            no_distance_feedback(),
        );
        if route_result == -1 {
            fail_office_route(world, sim_index, STATE_NIGHT_B);
        }
        // rc 0/1/2: stay in transit; next leg's arrival re-enters here.
        // rc 3 can't happen — arrivalFloor != target by precondition.
        return;
    }

    if sim.state_code == STATE_COMMUTE && arrival_floor == sim.floor_anchor {
        finalize_office_floor_arrival_in(world, sim_index, object.as_deref(), STATE_ACTIVE);
        return;
    }

    if sim.state_code == STATE_DEPARTURE && arrival_floor == LOBBY_FLOOR {
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_PARKED;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::create_time_state;

    fn office(status: i64) -> PlacedObjectRecord {
        PlacedObjectRecord {
            left_tile_index: 0,
            right_tile_index: 8,
            object_type_code: FAMILY_OFFICE,
            unit_status: status,
            aux_value_or_timer: 0,
            linked_record_index: -1,
            dirty_flag: 0,
            occupied_flag: 0,
            eval_level: -1,
            eval_score: -1,
            rent_level: 1,
            activation_tick_count: 0,
            housekeeping_claimed_flag: 0,
            vip_flag: None,
        }
    }

    #[test]
    fn presence_counter() {
        let mut o = office(0);
        advance_office_presence_counter(&mut o);
        assert_eq!(o.unit_status, 1);
        o.unit_status = 8;
        advance_office_presence_counter(&mut o);
        assert_eq!(o.unit_status, 1);
        o.unit_status = 0x10;
        advance_office_presence_counter(&mut o);
        assert_eq!(o.unit_status, 0x10);
        let mut t = create_time_state();
        o.unit_status = 1;
        decrement_office_presence_counter(&mut o, &t);
        assert_eq!(o.unit_status, 0);
        decrement_office_presence_counter(&mut o, &t);
        assert_eq!(o.unit_status, 0);
        t.daypart_index = 4;
        decrement_office_presence_counter(&mut o, &t);
        assert_eq!(o.unit_status, 8);
        assert_eq!(route_failure_state_for_office(&office(0x10)), STATE_MORNING_GATE);
        assert_eq!(route_failure_state_for_office(&office(0x0f)), STATE_NIGHT_A);
    }

    #[test]
    fn handler_table_aliases() {
        use crate::js::record_get;
        let a = *record_get(OFFICE_REFRESH_HANDLER_TABLE, STATE_ACTIVE).unwrap() as usize;
        let b = *record_get(OFFICE_REFRESH_HANDLER_TABLE, STATE_ACTIVE_ALT).unwrap() as usize;
        assert_eq!(a, b);
        // STATE_HOTEL_PARKED (0x24) has no office handler.
        assert!(record_get(OFFICE_REFRESH_HANDLER_TABLE, 0x24).is_none());
        assert_eq!(OFFICE_REFRESH_HANDLER_TABLE.len(), 18);
    }
}

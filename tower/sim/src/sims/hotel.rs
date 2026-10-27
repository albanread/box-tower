// ts: sims/hotel.ts

use crate::ledger::{add_cashflow_from_family_resource, LedgerState};
use crate::progression::add_to_population_bucket;
use crate::queue::resolve::ResolveSimRouteOptions;
use crate::resources::{FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN};
use crate::sim_access::state_bits::set_sim_in_transit;
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::time::{TimeState, DAY_TICK_NEW_DAY};
use crate::world::{sample_rng, PlacedObjectRecord, RouteState, WorldState, GRID_HEIGHT};

use super::states::{
    ACTIVATION_TICK_CAP, COMMERCIAL_DWELL_STATE, HOTEL_FAMILIES, HOTEL_ROOM_SELECTOR, LOBBY_FLOOR,
    STATE_ACTIVE, STATE_ACTIVE_TRANSIT, STATE_CHECKOUT_QUEUE, STATE_DEPARTURE, STATE_DEPARTURE_TRANSIT,
    STATE_HOTEL_PARKED, STATE_MORNING_GATE, STATE_MORNING_TRANSIT, STATE_NIGHT_B, STATE_TRANSITION,
    STATE_VENUE_TRIP,
};
use super::{
    dispatch_commercial_venue_visit, find_object_for_sim, finish_commercial_venue_dwell,
    resolve_sim_route_between_floors, try_assign_parking_service,
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

/// `key.split(",").map(Number)` for a placed-object key "x,y".
fn key_xy(key: &str) -> (i64, i64) {
    let mut parts = key.split(',').map(|p| {
        p.parse::<i64>()
            .unwrap_or_else(|_| panic!("placed-object key {key:?} is not \"x,y\""))
    });
    let x = parts.next().unwrap();
    let y = parts.next().unwrap_or_else(|| panic!("placed-object key {key:?} is not \"x,y\""));
    (x, y)
}

/// Binary's `subtype_index` (sim record byte 1) = floor-local ROOM rank in
/// column-ascending order, confirmed via emulator watchpoint on the IDIV at
/// 1228:3493 (every occupant of a given room shares the same subtype; the
/// per-occupant differentiator is the `occupant` word at sim+2 = baseOffset).
/// The IDIV parity therefore splits *rooms*, not individual sims.
fn floor_local_room_rank(world: &WorldState, sim_index: i64) -> i64 {
    let sim = &world.sims[sim_index as usize];
    let floor_y = GRID_HEIGHT - 1 - sim.floor_anchor;
    let mut columns: Vec<i64> = Vec::new();
    for (key, obj) in world.placed_objects.iter() {
        if !HOTEL_FAMILIES.contains(&obj.object_type_code) {
            continue;
        }
        let (x, y) = key_xy(key);
        if y != floor_y {
            continue;
        }
        columns.push(x);
    }
    columns.sort_by(|a, b| a.cmp(b));
    // `Math.max(0, columns.indexOf(sim.homeColumn))`: -1 when absent.
    let index = columns
        .iter()
        .position(|c| *c == sim.home_column)
        .map_or(-1, |i| i as i64);
    0.max(index)
}

fn hotel_arrival_state(world: &WorldState, sim_index: i64) -> i64 {
    if (floor_local_room_rank(world, sim_index) & 1) == 0 {
        STATE_ACTIVE
    } else {
        STATE_CHECKOUT_QUEUE
    }
}

/// activate_family_345_unit @ 1180:0e72: writes occupied-band base 0x00 / 0x08
/// only when the room was in the vacant band (>= 0x18). Also bumps the
/// population bucket by +1 (single) or +2 (twin/suite).
fn activate_hotel_unit(world: &mut WorldState, object: &str, time: &TimeState) {
    let o = object_mut(world, object);
    if o.unit_status >= 0x18 {
        o.unit_status = if time.daypart_index < 4 { 0 } else { 8 };
        o.activation_tick_count = 0;
        let amount = if o.object_type_code == FAMILY_HOTEL_SINGLE { 1 } else { 2 };
        let code = o.object_type_code;
        add_to_population_bucket(world, code, amount);
    }
}

/// increment_stay_phase_345 @ 1228:6a56: rotates the 0x10 sentinel back to
/// 1 / 9, otherwise increments by 1 (mod 256).
fn increment_stay_phase345(object: &mut PlacedObjectRecord, time: &TimeState) {
    if object.unit_status == 0x10 {
        object.unit_status = if time.daypart_index < 4 { 1 } else { 9 };
    } else {
        object.unit_status = (object.unit_status + 1) & 0xff;
    }
}

/// Binary no-venue path for hotel state-0x01 (route_sim_to_commercial_venue
/// 1238:0000): when `pickAvailableVenue` returns null, origin=home floor,
/// destination defaults to lobby. Resolver return drives the state:
///   1/2 → STATE_ACTIVE_TRANSIT (stairs / carrier ride to lobby)
///   3   → STATE_VENUE_TRIP directly (acquire_slot(0xb0)=3 fall-through)
///   0/-1 → STATE_CHECKOUT_QUEUE (Phase 4 will replace with sim+8=0xff retry)
/// `venueReturnState = STATE_CHECKOUT_QUEUE` acts as the no-venue marker so
/// the arrival + dwell handlers steer through 0x22 → 0x62 → 0x04.
fn route_hotel_to_lobby_no_venue(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let direction_flag = if floor_anchor > LOBBY_FLOOR { 0 } else { 1 };
    let result =
        resolve_sim_route_between_floors(world, sim_index, floor_anchor, LOBBY_FLOOR, direction_flag, Some(time), None);
    let sim = &mut world.sims[sim_index as usize];
    if result == -1 || result == 0 {
        sim.state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    sim.venue_return_state = STATE_CHECKOUT_QUEUE;
    if result == 3 {
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        return;
    }
    sim.origin_floor = sim.floor_anchor;
    // Phase 4: resolve owns sim.selectedFloor / destinationFloor (per-leg).
    sim.state_code = STATE_ACTIVE_TRANSIT;
}

fn activate_hotel_stay(world: &mut WorldState, sim_index: i64, time: &TimeState) {
    let Some(object) = find_object_for_sim(world, sim_index) else {
        return;
    };

    // Route requirement: actual route must succeed, not just structural check.
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let direction_flag = if floor_anchor > LOBBY_FLOOR { 1 } else { 0 };
    let result =
        resolve_sim_route_between_floors(world, sim_index, LOBBY_FLOOR, floor_anchor, direction_flag, Some(time), None);
    if result == -1 || result == 0 {
        return;
    }

    world.sims[sim_index as usize].origin_floor = LOBBY_FLOOR;

    // Binary 1228:33ba (en-route) + 1228:3434 (same-floor): both branches call
    // activate_family_345_unit when the room is still vacant.
    activate_hotel_unit(world, &object, time);

    if result == 3 {
        // Same-floor arrival: increment stay phase, then parity-split into
        // STATE_ACTIVE (even) or STATE_CHECKOUT_QUEUE (odd).
        increment_stay_phase345(object_mut(world, &object), time);
        let state = hotel_arrival_state(world, sim_index);
        let sim = &mut world.sims[sim_index as usize];
        sim.state_code = state;
        sim.selected_floor = sim.floor_anchor;
    } else {
        // Phase 4: resolve owns sim.selectedFloor / destinationFloor (per-leg).
        world.sims[sim_index as usize].state_code = STATE_MORNING_TRANSIT;
    }
}

pub fn checkout_hotel_stay(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    _sim_index: i64,
    object: &str,
) {
    let o = object_mut(world, object);
    let tile_name = if o.object_type_code == FAMILY_HOTEL_SINGLE {
        "hotelSingle"
    } else if o.object_type_code == FAMILY_HOTEL_TWIN {
        "hotelTwin"
    } else {
        "hotelSuite"
    };
    add_cashflow_from_family_resource(ledger, tile_name, o.rent_level, o.object_type_code);
    world.gate_flags.family345_sale_count += 1;
    let sale_count = world.gate_flags.family345_sale_count;
    if (sale_count < 20 && sale_count % 2 == 0) || (sale_count >= 20 && sale_count % 8 == 0) {
        world.gate_flags.newspaper_trigger = 1;
    } else {
        world.gate_flags.newspaper_trigger = 0;
    }
    // Binary 1228:2fa7 (dispatch state-0x05) does NOT touch sibling state
    // bytes at checkout. The base-0→PARKED / others→MORNING_GATE rewrite lives
    // in the per-sim NIGHT_B (state 0x26) handler at 1228:2bc5, applied only
    // to the sim being dispatched. Iterating siblings here clobbers any twin
    // already mid-trip on the carrier (state 0x45 → 0x20 anomaly).
    let o = object_mut(world, object);
    o.unit_status = if time.daypart_index < 4 { 0x28 } else { 0x30 };
    // Binary deactivate_family_hotel_unit_with_income (1180:0f24):
    //   1180:0f7d: MOV byte ptr ES:[BX+0x13],1  (dirty)
    //   1180:0f8e: MOV byte ptr ES:[BX+0x14],0  (occupied cleared)
    o.dirty_flag = 1;
    o.occupied_flag = 0;
    o.activation_tick_count = 0;
    let population_amount = if o.object_type_code == FAMILY_HOTEL_SINGLE { 1 } else { 2 };
    let code = o.object_type_code;
    add_to_population_bucket(world, code, -population_amount);
}

// --- Per-state handlers ---

/// hotel_refresh_0x24 — parked, awaiting guest room assignment.
fn handle_hotel_parked(
    _world: &mut WorldState,
    _ledger: &mut LedgerState,
    _time: &TimeState,
    _sim_index: i64,
    _object: &str,
) {
    // Binary: state 0x24 is NOT in the hotel jump table — it's a no-op.
}

/// hotel_refresh_0x20 — morning activation gate (STATE_MORNING_GATE).
fn handle_hotel_morning_gate(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    // Binary 1228:2c8c: CMP byte ptr ES:[BX+0x14],0x0 — reads occupied flag.
    if world.object(object).unwrap().occupied_flag == 0 {
        return;
    }
    // BOX: hotels -- under BOX RULES a room is booked by its appeal; one
    // with no booking tonight stays empty, as a suite below three stars does.
    if (time.daypart_index >= 4 && time.day_tick < DAY_TICK_NEW_DAY)
        && crate::box_ext::hotels::stays_empty(world, object, time.day_counter)
    {
        world.sims[sim_index as usize].state_code = STATE_NIGHT_B;
        return;
    }
    if time.daypart_index == 4 {
        if sample_rng(world) % 12 != 0 {
            return;
        }
        if world.sims[sim_index as usize].family_code == FAMILY_HOTEL_SUITE && world.star_count <= 2 {
            world.sims[sim_index as usize].state_code = STATE_NIGHT_B;
            return;
        }
        activate_hotel_stay(world, sim_index, time);
        return;
    }
    // Binary refresh jumptable state 0x20 (1228:2c63): `daypart > 4 AND
    // day_tick < 2300` dispatches the state-0x20 handler unconditionally
    // (no RNG). The prior code converted to CHECKOUT_QUEUE here, which
    // then rolled the CHECKOUT_QUEUE 1/12 gate — an extra LCG sample.
    if time.daypart_index > 4 && time.day_tick < DAY_TICK_NEW_DAY {
        if world.sims[sim_index as usize].family_code == FAMILY_HOTEL_SUITE && world.star_count <= 2 {
            world.sims[sim_index as usize].state_code = STATE_NIGHT_B;
            return;
        }
        activate_hotel_stay(world, sim_index, time);
    }
}

/// hotel_refresh_0x01 — active, route to commercial venue (STATE_ACTIVE).
fn handle_hotel_active(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    if time.daypart_index <= 3 {
        return;
    }
    if time.daypart_index > 4 {
        world.sims[sim_index as usize].state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if sample_rng(world) % 6 != 0 {
        return;
    }
    if world.sims[sim_index as usize].family_code == FAMILY_HOTEL_SUITE
        && world.star_count > 2
        && world.object(object).unwrap().unit_status != 0
    {
        try_assign_parking_service(world, time, sim_index);
    }
    // Upstream's closure holds `object`; this one reaches it by its key.
    let mut on_venue_reserved = |world: &mut WorldState| {
        let o = object_mut(world, object);
        o.activation_tick_count = ACTIVATION_TICK_CAP.min(o.activation_tick_count + 1);
    };
    let dispatched = dispatch_commercial_venue_visit(
        world,
        time,
        sim_index,
        HOTEL_ROOM_SELECTOR,
        STATE_ACTIVE,
        Some(STATE_ACTIVE_TRANSIT),
        None,
        Some(true),
        Some(true),
        Some(&mut on_venue_reserved),
    );
    if dispatched && world.sims[sim_index as usize].state_code == COMMERCIAL_DWELL_STATE {
        // Hotel same-floor venue: binary writes state=0x22, not 0x62.
        let sim = &mut world.sims[sim_index as usize];
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        sim.last_demand_tick = -1;
        return;
    }
    if !dispatched {
        route_hotel_to_lobby_no_venue(world, time, sim_index);
    }
}

/// Per-stride in-transit handler for STATE_ACTIVE_TRANSIT (0x41) and other
/// transit states whose binary base handler does NOT call resolve. The binary's
/// 0x41 handler (0x3126, alias of 0x01) uses the venue selector, not resolve;
/// for in-segment sims here we must still re-resolve the segment so the leg
/// progresses (carrier-routed sims are gated out by the caller).
fn handle_hotel_active_transit(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    let sim = &world.sims[sim_index as usize];
    if matches!(sim.route, RouteState::Carrier { .. }) {
        return;
    }
    if sim.destination_floor < 0 {
        return;
    }
    let source_floor = sim.selected_floor;
    let target_floor = sim.destination_floor;
    // Alias state 0x41 (ACTIVE_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here, so distance feedback was already applied by the base state
    // when the trip began.
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
        // Arrived. Trip counter already advanced inside resolve's same-floor
        // branch (1218:0046), which also clears `lastDemandTick`. Mirror the
        // binary's `handle_hotel_guest_venue_acquisition` (1228:4fab) inline
        // state transition: rc=3 → acquire_slot → state=0x22. Don't go through
        // `dispatchSimArrival` → `handleHotelSimArrival` here because that path
        // fires its own advance (matching binary's `dispatch_destination_queue_entries`
        // arrival site). Routing per-stride arrivals back through the carrier-
        // arrival handler causes a double-advance for the same trip.
        let _ = ledger;
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = target_floor;
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
    }
}

/// Per-stride STATE_MORNING_TRANSIT (0x60) handler. Binary alias of state 0x20
/// (handler 0x317b @ 0x327d): src=lookup 11a0:0650 (assigned target floor),
/// tgt=arg [BP+0xa]. Post-resolve transitions:
///   rc=-1 → clear or state=0x04 (CHECKOUT_QUEUE)
///   rc=0/1/2 → state=0x60 (stay in transit; next stride re-resolves)
///   rc=3 → state=0x01 / 0x04 via hotelArrivalState (parity-split).
fn handle_hotel_morning_transit(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    let source_floor = world.sims[sim_index as usize].selected_floor;
    let target_floor = world.sims[sim_index as usize].floor_anchor;
    // Alias state 0x60 (MORNING_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here (current_state 0x60 != base_state 0x20). Distance feedback
    // was already applied by the base state 0x20 dispatch.
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
        world.sims[sim_index as usize].state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if route_result == 3 {
        // Same-floor success. Binary 1228:3434 path: activate (if vacant), bump
        // stay phase, then parity-split via hotelArrivalState.
        activate_hotel_unit(world, object, time);
        increment_stay_phase345(object_mut(world, object), time);
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        let state = hotel_arrival_state(world, sim_index);
        world.sims[sim_index as usize].state_code = state;
    }
    // rc=0/1/2: stay in STATE_MORNING_TRANSIT; next stride will re-resolve.
}

/// Per-stride STATE_DEPARTURE_TRANSIT (0x45) handler. Binary alias of state
/// 0x05 (handler 0x2fa7 @ 0x2fd9): src=lookup 11a0:0650, tgt=arg [BP+0xa].
/// Post-resolve transitions:
///   rc=-1 → state=0x20 (MORNING_GATE) + service-eval-fail
///   rc=0/1/2 → state=0x45 (stay in transit; next stride re-resolves)
///   rc=3 → state=0x20 (MORNING_GATE)
///
/// NOTE: The binary's shared 0x05/0x45 handler at 1228:2fa7 has a
/// `CMP [BP-0x4], 0x5; JNZ 0x3503` at 1228:30bd that gates the
/// unit_status decrement + checkoutHotelStay block on state == 0x05.
/// State 0x45 SKIPS the checkout — payment was already booked by the
/// base 0x05 dispatch. Mirror that here: do NOT call checkoutHotelStay
/// from this transit handler.
fn handle_hotel_departure_transit(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    let sim = &world.sims[sim_index as usize];
    let source_floor = sim.selected_floor;
    let target_floor = if sim.destination_floor >= 0 {
        sim.destination_floor
    } else {
        LOBBY_FLOOR
    };
    // Alias state 0x45 (DEPARTURE_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here. Distance feedback was already applied by the base state
    // 0x05 dispatch.
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
        world.sims[sim_index as usize].state_code = STATE_MORNING_GATE;
        return;
    }
    if route_result == 3 {
        // Lobby arrived. Binary 0x45 handler skips the checkout/unit_status
        // decrement block (CMP/JNZ at 1228:30bd) — cash was already booked
        // by the state-0x05 dispatch that initiated this trip. Just transition
        // to MORNING_GATE (or HOTEL_PARKED for baseOffset==0).
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = if sim.base_offset == 0 {
            STATE_HOTEL_PARKED
        } else {
            STATE_MORNING_GATE
        };
    }
    // rc=0/1/2: stay in STATE_DEPARTURE_TRANSIT; next stride will re-resolve.
}

/// hotel_refresh_0x04 — checkout queue (STATE_CHECKOUT_QUEUE).
fn handle_hotel_checkout_queue(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    if time.daypart_index < 5 {
        return;
    }
    if time.day_tick <= 2400 && sample_rng(world) % 12 != 0 {
        return;
    }
    world.sims[sim_index as usize].state_code = STATE_TRANSITION;
}

/// hotel_refresh_0x10 — transition (STATE_TRANSITION).
fn handle_hotel_transition(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    if time.daypart_index >= 5 {
        if time.day_tick <= 2566 {
            return;
        }
        if sample_rng(world) % 12 != 0 {
            return;
        }
    }
    let o = object_mut(world, object);
    if o.unit_status == 0x10 {
        o.unit_status = if o.object_type_code == FAMILY_HOTEL_SINGLE { 1 } else { 2 };
    }
    world.sims[sim_index as usize].state_code = STATE_DEPARTURE;
}

/// hotel_refresh_0x05 — departure (STATE_DEPARTURE).
fn handle_hotel_departure(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    // Binary state 0x05 refresh handler at 1228:2c43:
    // daypart 0: 1/12 RNG gate, daypart >= 6: no-op, else: dispatch.
    if time.daypart_index == 0 && sample_rng(world) % 12 != 0 {
        return;
    }
    if time.daypart_index >= 6 {
        return;
    }
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        floor_anchor,
        LOBBY_FLOOR,
        if floor_anchor > LOBBY_FLOOR { 0 } else { 1 },
        Some(time),
        None,
    );
    if route_result == 0 {
        return;
    }
    if route_result == -1 {
        // Binary mapping: rc=-1 → state=0x20 (MORNING_GATE) + service-eval-fail.
        world.sims[sim_index as usize].state_code = STATE_MORNING_GATE;
        return;
    }
    let sim = &mut world.sims[sim_index as usize];
    sim.origin_floor = sim.floor_anchor;
    // Phase 4: resolve owns sim.selectedFloor / destinationFloor.
    // trip_prep_for_checkout: decrement unit_status; when it hits the
    // final countdown boundary, take the payout immediately (binary
    // books cash at dispatch, not lobby arrival).
    let o = object_mut(world, object);
    o.unit_status -= 1;
    let ready = (o.unit_status & 0x07) == 0;
    if route_result == 3 {
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        if ready {
            checkout_hotel_stay(world, ledger, time, sim_index, object);
        } else {
            sim.state_code = if sim.base_offset == 0 {
                STATE_HOTEL_PARKED
            } else {
                STATE_MORNING_GATE
            };
        }
    } else {
        if ready {
            checkout_hotel_stay(world, ledger, time, sim_index, object);
        }
        world.sims[sim_index as usize].state_code = STATE_DEPARTURE_TRANSIT;
    }
}

/// hotel_refresh_0x22 — venue trip dwell (STATE_VENUE_TRIP).
///
/// Binary shared 0x22/0x62 handler (1228:50ef): release_commercial_venue_slot
/// returns 1 immediately for fake-lunch (sim+6 < 0), with no dwell gate. Then
/// resolve_route(lobby, home) runs; the carrier-enqueue branch ORs in the 0x40
/// transit bit, producing state 0x62. The sim physically rides home; arrival
/// drops it to STATE_CHECKOUT_QUEUE via the dispatch handler.
fn handle_hotel_venue_trip(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    if world.sims[sim_index as usize].venue_return_state == STATE_CHECKOUT_QUEUE {
        let floor_anchor = world.sims[sim_index as usize].floor_anchor;
        let dir = if floor_anchor > LOBBY_FLOOR { 1 } else { 0 };
        let result = resolve_sim_route_between_floors(world, sim_index, LOBBY_FLOOR, floor_anchor, dir, Some(time), None);
        let sim = &mut world.sims[sim_index as usize];
        if result == 3 {
            sim.state_code = STATE_CHECKOUT_QUEUE;
            sim.venue_return_state = 0;
            return;
        }
        // Binary 1228:50ef maps resolve returns 0/1/2 all to state 0x62
        // (commercial dwell). Resolve already flips 0x62 on success (1/2);
        // queue-full (0) only sets the waiting bit, so promote here.
        if result == 0 {
            set_sim_in_transit(sim, true);
        }
        return;
    }
    let sim = &mut world.sims[sim_index as usize];
    // Real-venue dwell: stay in 0x22 until service_duration elapsed.
    if time.day_tick - sim.queue_tick < 64 {
        return;
    }
    if sim.selected_floor == sim.floor_anchor {
        // Binary: release_venue_slot → resolve(floor→floor)=3 → advanceSimTripCounters
        advance_sim_trip_counters(sim);
        sim.state_code = STATE_CHECKOUT_QUEUE;
        sim.venue_return_state = 0;
        return;
    }
    sim.state_code = COMMERCIAL_DWELL_STATE;
    sim.queue_tick = time.day_tick;
}

/// hotel_refresh_0x62 — commercial dwell state (COMMERCIAL_DWELL_STATE).
///
/// For the no-venue return path, this fires when the sim is in 0x62 with no
/// active carrier route — i.e. the previous resolve attempt returned queue-full.
/// Retry from current floor back home; when the carrier eventually picks the
/// sim up, refresh skips it (route.mode !== 'idle') until arrival fires the
/// dispatch handler.
fn handle_hotel_commercial_dwell(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    let sim = &world.sims[sim_index as usize];
    if sim.venue_return_state == STATE_CHECKOUT_QUEUE {
        let (selected_floor, floor_anchor) = (sim.selected_floor, sim.floor_anchor);
        let dir = if floor_anchor > LOBBY_FLOOR { 1 } else { 0 };
        let result =
            resolve_sim_route_between_floors(world, sim_index, selected_floor, floor_anchor, dir, Some(time), None);
        if result == 3 {
            let sim = &mut world.sims[sim_index as usize];
            sim.state_code = STATE_CHECKOUT_QUEUE;
            sim.venue_return_state = 0;
        }
        return;
    }
    finish_commercial_venue_dwell(&mut world.sims[sim_index as usize], time, STATE_ACTIVE);
}

/// hotel_refresh_0x26 — night park (STATE_NIGHT_B).
fn handle_hotel_night_b(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    // Binary: dayTick <= 2300 → no-op; dayTick > 2300 → reset.
    if time.day_tick <= DAY_TICK_NEW_DAY {
        return;
    }
    let sim = &mut world.sims[sim_index as usize];
    sim.state_code = if sim.base_offset == 0 {
        STATE_HOTEL_PARKED
    } else {
        STATE_MORNING_GATE
    };
}

/// Upstream's `(world, ledger, time, sim, object)`: the sim by its index in
/// `world.sims`, the object by its key in `world.placed_objects`.
pub type HotelHandler = fn(&mut WorldState, &mut LedgerState, &TimeState, i64, &str);

/// Hotel refresh dispatch table (state_code → handler).
///
/// (A constant `Map`: pairs in insertion order; `js::record_get` looks one up.)
pub const HOTEL_REFRESH_HANDLER_TABLE: &[(i64, HotelHandler)] = &[
    (STATE_HOTEL_PARKED, handle_hotel_parked),                 // 0x24
    (STATE_MORNING_GATE, handle_hotel_morning_gate),           // 0x20
    (STATE_ACTIVE, handle_hotel_active),                       // 0x01
    (STATE_MORNING_TRANSIT, handle_hotel_morning_transit),     // 0x60 → 0x317b alias of 0x20
    (STATE_ACTIVE_TRANSIT, handle_hotel_active_transit),       // 0x41 → 0x3126 alias of 0x01
    (STATE_DEPARTURE_TRANSIT, handle_hotel_departure_transit), // 0x45 → 0x2fa7 alias of 0x05
    (STATE_CHECKOUT_QUEUE, handle_hotel_checkout_queue),       // 0x04
    (STATE_TRANSITION, handle_hotel_transition),               // 0x10
    (STATE_DEPARTURE, handle_hotel_departure),                 // 0x05
    (STATE_VENUE_TRIP, handle_hotel_venue_trip),               // 0x22
    (COMMERCIAL_DWELL_STATE, handle_hotel_commercial_dwell),   // 0x62
    (STATE_NIGHT_B, handle_hotel_night_b),                     // 0x26
];

pub fn process_hotel_sim(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64) {
    let Some(object) = find_object_for_sim(world, sim_index) else {
        return;
    };

    // Binary `refresh_runtime_entities_for_tick_stride` (1228:0d64) gates the
    // family 3/4/5 handler on `base_offset > 0` — the first occupant (sim+2==0)
    // is never refreshed, so its state persists.
    if world.sims[sim_index as usize].base_offset == 0 {
        return;
    }

    let handler = crate::js::record_get(HOTEL_REFRESH_HANDLER_TABLE, world.sims[sim_index as usize].state_code);
    if let Some(handler) = handler {
        handler(world, ledger, time, sim_index, &object);
    } else {
        world.sims[sim_index as usize].state_code = STATE_HOTEL_PARKED;
    }
}

pub fn handle_hotel_sim_arrival(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    sim_index: i64,
    arrival_floor: i64,
) {
    let object = find_object_for_sim(world, sim_index);

    let sim = &mut world.sims[sim_index as usize];
    if sim.state_code == STATE_MORNING_TRANSIT && arrival_floor == sim.floor_anchor {
        // Binary `dispatch_destination_queue_entries` (1218:0883) writes
        // sim+7 = arrival_floor then invokes the family handler with
        // arg = arrival_floor. For state 0x60 (alias of 0x20) at handler
        // 0x317b, the resolve call is src=sim+7, tgt=arg — both equal to
        // the arrival floor. Resolve's same-floor branch (1218:0046,
        // gated on is_passenger_route=1 which 0x317b passes) advances the
        // sim trip counters before returning rc=3. We mirror that advance
        // here because dispatchSimArrival shortcuts the per-stride re-entry.
        advance_sim_trip_counters(sim);
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        if let Some(object) = object {
            // Binary state-0x60 "in-transit arrived" (1228:3434): activate if
            // vacant, bump stay phase, then parity-split into 0x01 / 0x04.
            activate_hotel_unit(world, &object, time);
            increment_stay_phase345(object_mut(world, &object), time);
            let state = hotel_arrival_state(world, sim_index);
            world.sims[sim_index as usize].state_code = state;
        }
        return;
    }

    if sim.state_code == STATE_ACTIVE_TRANSIT
        && arrival_floor == LOBBY_FLOOR
        && sim.venue_return_state == STATE_CHECKOUT_QUEUE
    {
        // Binary state-0x41 reentry on arrival at lobby with no venue reserved
        // (sim+6 = 0xb0 sentinel): handle_hotel_guest_venue_acquisition (1228:4fab)
        // calls resolve_sim_route_between_floors(is_passenger_route=1, src=lobby,
        // dst=lobby) → same-floor rc=3 → advance_sim_trip_counters at 1218:0046.
        // Always fire advance (binary is unconditional). Per-stride
        // handleHotelActiveTransit's resolve+advance path is suppressed below
        // (we no longer call dispatchSimArrival from there).
        advance_sim_trip_counters(sim);
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        return;
    }

    if sim.state_code == STATE_ACTIVE_TRANSIT && arrival_floor == sim.destination_floor {
        // Hotel arrival at a real commercial venue (binary 1228:4fab sets
        // state=0x22 with queueTick = dayTick as phase-start, not 0x62).
        sim.destination_floor = -1;
        sim.selected_floor = arrival_floor;
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        return;
    }

    if sim.state_code == COMMERCIAL_DWELL_STATE
        && arrival_floor == sim.floor_anchor
        && sim.venue_return_state == STATE_CHECKOUT_QUEUE
    {
        // Binary 1228:50ef state-0x62 dispatch (jumptable entry shared with 0x22):
        // release_commercial_venue_slot returns 1, resolve(home→home)=3, then
        // advance_sim_trip_counters fires inside resolve at 1218:0046.
        // Unconditional to match binary; double-firing is prevented by NOT
        // calling dispatchSimArrival from the per-stride handler below.
        advance_sim_trip_counters(sim);
        sim.destination_floor = -1;
        sim.selected_floor = arrival_floor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
        sim.venue_return_state = 0;
        return;
    }

    if sim.state_code == STATE_DEPARTURE_TRANSIT && arrival_floor == LOBBY_FLOOR {
        // Binary state-0x45 dispatch jumptable entry → handler invokes
        // resolve(rc=3, isPassengerRoute=1) on same-floor lobby arrival, which
        // advances trip counters at 1218:0046. Unconditional to match binary.
        advance_sim_trip_counters(sim);
        sim.destination_floor = -1;
        sim.state_code = if sim.base_offset == 0 {
            STATE_HOTEL_PARKED
        } else {
            STATE_MORNING_GATE
        };
        return;
    }

    if sim.state_code == STATE_CHECKOUT_QUEUE && arrival_floor == LOBBY_FLOOR {
        sim.destination_floor = -1;
        if let Some(object) = object {
            let o = object_mut(world, &object);
            o.unit_status -= 1;
            if (o.unit_status & 0x07) == 0 {
                checkout_hotel_stay(world, ledger, time, sim_index, &object);
            } else {
                let sim = &mut world.sims[sim_index as usize];
                sim.state_code = if sim.base_offset == 0 {
                    STATE_HOTEL_PARKED
                } else {
                    STATE_MORNING_GATE
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::SimRecord;

    fn room(left: i64, code: i64, status: i64) -> PlacedObjectRecord {
        PlacedObjectRecord {
            left_tile_index: left,
            right_tile_index: left + 3,
            object_type_code: code,
            unit_status: status,
            aux_value_or_timer: 0,
            linked_record_index: -1,
            dirty_flag: 0,
            occupied_flag: 1,
            eval_level: -1,
            eval_score: -1,
            rent_level: 1,
            activation_tick_count: 0,
            housekeeping_claimed_flag: 0,
            vip_flag: None,
        }
    }

    fn sim_at(floor_anchor: i64, home_column: i64) -> SimRecord {
        SimRecord {
            floor_anchor,
            home_column,
            base_offset: 1,
            facility_slot: 0,
            family_code: FAMILY_HOTEL_SINGLE,
            state_code: STATE_MORNING_GATE,
            route: RouteState::Idle,
            selected_floor: floor_anchor,
            origin_floor: floor_anchor,
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
            spawn_floor: floor_anchor,
            post_claim_countdown: 0,
            encoded_target_floor: 0,
            commercial_venue_slot: -1,
        }
    }

    #[test]
    fn room_rank_parity_is_by_column_order() {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        let y = GRID_HEIGHT - 1 - 20;
        // Inserted out of column order; a numeric (not string) sort puts 8 before 40.
        for (x, code) in [(40, FAMILY_HOTEL_TWIN), (8, FAMILY_HOTEL_SINGLE), (100, FAMILY_HOTEL_SUITE), (60, 7)] {
            w.placed_objects.insert(format!("{x},{y}"), room(x, code, 0x18));
        }
        w.placed_objects.insert(format!("12,{}", y - 1), room(12, FAMILY_HOTEL_SINGLE, 0x18));
        w.sims = vec![sim_at(20, 8), sim_at(20, 40), sim_at(20, 100), sim_at(20, 60)];
        assert_eq!(floor_local_room_rank(&w, 0), 0);
        assert_eq!(floor_local_room_rank(&w, 1), 1);
        assert_eq!(floor_local_room_rank(&w, 2), 2);
        // Not a hotel column: indexOf gives -1, clamped to 0.
        assert_eq!(floor_local_room_rank(&w, 3), 0);
        assert_eq!(hotel_arrival_state(&w, 1), STATE_CHECKOUT_QUEUE);
        assert_eq!(hotel_arrival_state(&w, 2), STATE_ACTIVE);
    }

    #[test]
    fn stay_phase() {
        let mut t = crate::time::create_time_state();
        let mut o = room(0, FAMILY_HOTEL_SINGLE, 0x10);
        increment_stay_phase345(&mut o, &t);
        assert_eq!(o.unit_status, 1);
        o.unit_status = 0x10;
        t.daypart_index = 4;
        increment_stay_phase345(&mut o, &t);
        assert_eq!(o.unit_status, 9);
        o.unit_status = 0xff;
        increment_stay_phase345(&mut o, &t);
        assert_eq!(o.unit_status, 0);
    }
}

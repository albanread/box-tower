// ts: sims/condo.ts

use crate::ledger::{add_cashflow_from_family_resource, LedgerState};
use crate::progression::add_to_population_bucket;
use crate::queue::resolve::ResolveSimRouteOptions;
use crate::resources::FAMILY_CONDO;
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::time::{pre_day4, TimeState};
use crate::world::{sample_rng, PlacedObjectRecord, RouteState, SimRecord, WorldState};

use super::states::{
    COMMERCIAL_VENUE_DWELL_TICKS, CONDO_SELECTOR_FAST_FOOD, CONDO_SELECTOR_RESTAURANT,
    CONDO_SELECTOR_RETAIL, LOBBY_FLOOR, STATE_ACTIVE, STATE_ACTIVE_TRANSIT, STATE_AT_WORK,
    STATE_AT_WORK_TRANSIT, STATE_CHECKOUT_QUEUE, STATE_COMMUTE, STATE_COMMUTE_TRANSIT,
    STATE_MORNING_GATE, STATE_MORNING_TRANSIT, STATE_TRANSITION, STATE_VENUE_HOME_TRANSIT,
    STATE_VENUE_TRIP, UNIT_STATUS_CONDO_VACANT,
};
use super::{dispatch_commercial_venue_visit, dispatch_sim_arrival, find_object_for_sim, resolve_sim_route_between_floors};

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

/// finalize_condo_sale @ family-9 helper: credits YEN #1001 condo payout,
/// drops unit_status into the occupied band (0 pre-day-4, 8 after), and
/// marks the slot sold. Idempotent because `unit_status >= 0x18` guards the
/// caller.
fn finalize_condo_sale(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, object: &str) {
    let o = object_mut(world, object);
    add_cashflow_from_family_resource(ledger, "condo", o.rent_level, o.object_type_code);
    o.unit_status = if pre_day4(time) { 0x00 } else { 0x08 };
    add_to_population_bucket(world, FAMILY_CONDO, 3);
}

/// dispatch_0x20 (MORNING_GATE) per condo-handler-decomp / spec PEOPLE.md
/// §0x20/0x60. Routes the new resident from the lobby UP to the unit; the
/// SALE point fires when the unit is still vacant and resolve returned
/// 0/1/2/3. Per-leg progression is driven by re-entry to
/// handleCondoMorningTransit on subsequent strides.
///
/// Binary 1228:3b71 (state-0x20 handler) pushes:
///   source_floor = 0xa (LOBBY, hardcoded at 1228:3bb2 for state==0x20)
///   target_floor = sim's home floor ([BP+0xa] = floorAnchor)
/// The state-0x20 trip is "lobby → home", not "home → lobby". The earlier
/// wiring had the args reversed, which made resolve enqueue a DOWN call
/// from the home floor (stamping secondaryRouteStatusByFloor[homeSlot]=1)
/// and pulled the elevator car up to the home floor before any rider
/// boarded.
///
/// Transition table (matches binary + spec):
///   rc=-1 + sold     → INC unit_status → 0x04 (CHECKOUT_QUEUE)
///   rc=-1 + unsold   → 0x60 (no sale; retry next stride)
///   rc=0/1/2+unsold  → 0x60 + SALE
///   rc=3   + unsold  → INC → 0x04 + SALE
///   rc=3   + sold    → INC → 0x04
///
/// Resolve owns sim.selectedFloor / sim.destinationFloor on rc=1/2 (per-leg
/// progression) — do NOT overwrite them after the call.
fn dispatch_condo_morning_gate(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let direction_flag = if floor_anchor > LOBBY_FLOOR { 1 } else { 0 };
    let result =
        resolve_sim_route_between_floors(world, sim_index, LOBBY_FLOOR, floor_anchor, direction_flag, Some(time), None);
    let was_vacant = world.object(object).unwrap().unit_status >= UNIT_STATUS_CONDO_VACANT;

    if result == -1 {
        // Sold sims terminate to CHECKOUT_QUEUE; unsold sims stay in the
        // transit state to retry next stride.
        world.sims[sim_index as usize].state_code = if was_vacant {
            STATE_MORNING_TRANSIT
        } else {
            STATE_CHECKOUT_QUEUE
        };
        return;
    }

    if was_vacant {
        finalize_condo_sale(world, ledger, time, object);
    }

    let sim = &mut world.sims[sim_index as usize];
    if result == 3 {
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
        return;
    }

    // rc=0/1/2: in transit. Resolve owns sim+7/sim+0x12 — handler sets state only.
    sim.state_code = STATE_MORNING_TRANSIT;
}

// --- Per-state handlers ---

/// condo_refresh_0x20 — morning gate (STATE_MORNING_GATE).
fn handle_condo_morning_gate(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    // Binary 1228:375e: CMP byte ptr ES:[BX+0x14],0x0 — reads occupied flag.
    // refresh_0x20: slot+0x14 != 0 AND daypart < 5 → dispatch.
    if world.object(object).unwrap().occupied_flag == 0 {
        return;
    }
    if time.daypart_index >= 5 {
        return;
    }
    dispatch_condo_morning_gate(world, ledger, time, sim_index, object);
}

/// condo_refresh_0x60 — in-transit morning gate (STATE_MORNING_TRANSIT). Per
/// binary jump table, state 0x60 aliases state 0x20's handler with variant
/// flag = 0. Per stride, re-resolves the route from sim+7 to LOBBY; resolve
/// advances sim+7 by one leg. On rc=3 (arrived) the SALE finalizes and the
/// sim transitions to CHECKOUT_QUEUE. On rc=-1 with a still-vacant unit the
/// sim stays in the transit state to retry next stride (matches binary's
/// "unsold" branch staying at 0x60).
fn handle_condo_morning_transit(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, object: &str) {
    let sim = &world.sims[sim_index as usize];
    if matches!(sim.route, RouteState::Carrier { .. }) {
        return;
    }
    let source_floor = sim.selected_floor;
    let target_floor = sim.floor_anchor;
    // Alias state 0x60 (MORNING_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here. Distance feedback was already applied by the base state
    // 0x20 dispatch. Per binary 1228:3b71 the trip target is the sim's home
    // floor ([BP+0xa] = floorAnchor) — same as state 0x20 — with the source
    // taken from the in-transit local (sim.selectedFloor) on the 0x60 alias.
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        no_distance_feedback(),
    );
    let was_vacant = world.object(object).unwrap().unit_status >= UNIT_STATUS_CONDO_VACANT;

    if result == -1 {
        world.sims[sim_index as usize].state_code = if was_vacant {
            STATE_MORNING_TRANSIT
        } else {
            STATE_CHECKOUT_QUEUE
        };
        return;
    }

    if was_vacant {
        finalize_condo_sale(world, ledger, time, object);
    }

    if result == 3 {
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
    }
    // rc=0/1/2: stay in transit; next stride re-resolves the next leg.
}

/// condo_refresh_0x40 — in-transit commute (STATE_COMMUTE_TRANSIT). Binary
/// 0x40 aliases 0x00 (handler 1228:3a77). Per binary mapping table:
///   rc=-1 → 0x40 (transit; we treat as CHECKOUT_QUEUE on hard fail per office
///            pattern)
///   rc=0/1/2 → stay in transit (next stride re-resolves)
///   rc=3 → arrived → AT_WORK (via dispatchSimArrival → handleCondoSimArrival)
///
/// Source = sim+7 (selectedFloor); Target = 0xa (LOBBY_FLOOR). Resolve owns
/// sim+7/sim+0x12 on rc=1/2 — handler must not overwrite.
fn handle_condo_commute_transit(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    let sim = &world.sims[sim_index as usize];
    if matches!(sim.route, RouteState::Carrier { .. }) {
        return;
    }
    let source_floor = sim.selected_floor;
    let target_floor = LOBBY_FLOOR;
    // Alias state 0x40 (COMMUTE_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here. Distance feedback was already applied by the base state
    // 0x00 dispatch.
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        no_distance_feedback(),
    );
    if result == -1 {
        world.sims[sim_index as usize].state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if result == 3 {
        // Arrived. Trip counter + lastDemandTick reset already done inside
        // resolve's same-floor branch (1218:0046). The binary arrival path
        // 1218:0883 has no further advance.
        dispatch_sim_arrival(world, ledger, time, sim_index, target_floor);
    }
}

/// condo_refresh_0x61 — in-transit at-work return (STATE_AT_WORK_TRANSIT).
/// Binary 0x61 aliases 0x21 (handler 1228:3d8a). Per binary mapping table:
///   rc=-1 → 0x04 (CHECKOUT_QUEUE)
///   rc=0/1/2 → 0x61 (stays; next stride re-resolves)
///   rc=3 → 0x04 (CHECKOUT_QUEUE)
///
/// Source = sim+7 (selectedFloor); Target = arg (floorAnchor / home). Resolve
/// owns sim+7/sim+0x12 on rc=1/2 — handler must not overwrite.
fn handle_condo_at_work_transit(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    let sim = &world.sims[sim_index as usize];
    if matches!(sim.route, RouteState::Carrier { .. }) {
        return;
    }
    let source_floor = sim.selected_floor;
    let target_floor = sim.floor_anchor;
    // Alias state 0x61 (AT_WORK_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here. Distance feedback was already applied by the base state
    // 0x21 dispatch.
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        no_distance_feedback(),
    );
    if result == -1 {
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if result == 3 {
        // Arrived. Trip counter + lastDemandTick reset already done inside
        // resolve's same-floor branch (1218:0046).
        dispatch_sim_arrival(world, ledger, time, sim_index, target_floor);
    }
}

/// condo_refresh_0x41 — in-transit active/venue (STATE_ACTIVE_TRANSIT). Binary
/// state 0x01/0x41 does NOT call resolve in the per-tick handler; the venue
/// selector (1238:0000) is invoked once at dispatch time. For per-leg segment
/// progression, however, the handler still must advance one leg per stride;
/// otherwise the sim is stuck because the legacy whole-trip finalizer is now
/// skipped for condos. Carrier-routed sims are skipped here (handled by
/// maybe_dispatch_queued_route_after_wait per the binary).
fn handle_condo_active_transit(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
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
    // is `0` here. Distance feedback was already applied by the base state
    // 0x01 dispatch (which uses the venue selector, not resolve, but follows
    // the same base-vs-alias contract).
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        no_distance_feedback(),
    );
    if result == -1 {
        world.sims[sim_index as usize].state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if result == 3 {
        // Arrived. Trip counter + lastDemandTick reset already done inside
        // resolve's same-floor branch (1218:0046).
        dispatch_sim_arrival(world, ledger, time, sim_index, target_floor);
    }
}

/// condo_refresh_0x62 — in-transit venue-home (STATE_VENUE_HOME_TRANSIT).
/// Binary state 0x22/0x62 does NOT call resolve in the per-tick handler
/// (uses 1238:0244 release path). Per-leg segment progression still requires
/// a re-resolve here so the sim isn't stuck. Carrier-routed sims are skipped
/// (handled by maybe_dispatch_queued_route_after_wait).
fn handle_condo_venue_home_transit(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    let sim = &world.sims[sim_index as usize];
    if matches!(sim.route, RouteState::Carrier { .. }) {
        return;
    }
    if sim.destination_floor < 0 {
        return;
    }
    let source_floor = sim.selected_floor;
    let target_floor = sim.destination_floor;
    // Alias state 0x62 (VENUE_HOME_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here. Distance feedback was already applied by the base state
    // 0x22 dispatch.
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        no_distance_feedback(),
    );
    if result == -1 {
        let sim = &mut world.sims[sim_index as usize];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if result == 3 {
        // Arrived. Trip counter + lastDemandTick reset already done inside
        // resolve's same-floor branch (1218:0046).
        dispatch_sim_arrival(world, ledger, time, sim_index, target_floor);
    }
}

/// condo_refresh_0x04 — checkout queue (STATE_CHECKOUT_QUEUE).
fn handle_condo_checkout_queue(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    // refresh_0x04: baseOffset==2 branch dispatches unconditionally at
    // daypart>=5 (no RNG). Other siblings roll 1/12 until dayTick>=2401,
    // then dispatch unconditionally.
    if time.daypart_index < 5 {
        return;
    }
    if world.sims[sim_index as usize].base_offset != 2 && time.day_tick < 2401 && sample_rng(world) % 12 != 0 {
        return;
    }
    world.sims[sim_index as usize].state_code = STATE_TRANSITION;
}

/// condo_refresh_0x10 — unit status transition (STATE_TRANSITION).
fn handle_condo_transition(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    // refresh_0x10: daypart < 5 → dispatch; daypart >= 5 AND
    // dayTick < 2567 → skip; daypart >= 5 AND dayTick >= 2567 →
    // 1/12 RNG → dispatch.
    if time.daypart_index >= 5 {
        if time.day_tick < 2567 {
            return;
        }
        if sample_rng(world) % 12 != 0 {
            return;
        }
    }
    // dispatch_0x10 per FUN_1228_397b:
    //   weekend_flag == 1 && BP+0xc % 2 != 0 → 0x04 (CHECKOUT_QUEUE)
    //   weekend_flag == 1 && BP+0xc % 2 == 0 → 0x01 (ACTIVE)
    //   weekend_flag != 1 && BP+0xe == 1     → 0x01 (ACTIVE)
    //   weekend_flag != 1 && BP+0xe != 1     → 0x00 (COMMUTE)
    let sim = &mut world.sims[sim_index as usize];
    if time.weekend_flag == 1 {
        sim.state_code = if sim.facility_slot % 2 != 0 {
            STATE_CHECKOUT_QUEUE
        } else {
            STATE_ACTIVE
        };
    } else {
        sim.state_code = if sim.base_offset == 1 { STATE_ACTIVE } else { STATE_COMMUTE };
    }
}

/// condo_refresh_0x00 — commute (STATE_COMMUTE).
fn handle_condo_commute(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    // refresh_0x00: daypart 0 → 1/12 RNG; daypart 6 → skip; else dispatch.
    if time.daypart_index == 6 {
        return;
    }
    if time.daypart_index == 0 && sample_rng(world) % 12 != 0 {
        return;
    }
    dispatch_condo_commute(world, time, sim_index);
}

/// condo_refresh_0x01 — active / venue selection (STATE_ACTIVE).
fn handle_condo_active(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    // refresh_0x01 (1228:3681):
    //   weekend_flag == 1 AND BP+8 (facilitySlot) % 4 == 0:
    //     daypart 4: 1/6 RNG → dispatch (fallthrough)
    //     daypart > 4: set state = 0x04 (CHECKOUT_QUEUE) directly, return
    //     daypart < 4: return
    //   else (non-weekend or slot%4 != 0):
    //     daypart 0: dayTick > 240 AND 1/12 RNG → dispatch
    //     daypart 6: skip
    //     else: dispatch
    if time.weekend_flag == 1 && world.sims[sim_index as usize].facility_slot % 4 == 0 {
        if time.daypart_index == 4 {
            if sample_rng(world) % 6 == 0 {
                dispatch_condo_active(world, time, sim_index);
            }
            return;
        }
        if time.daypart_index > 4 {
            world.sims[sim_index as usize].state_code = STATE_CHECKOUT_QUEUE;
            return;
        }
        return;
    }
    if time.daypart_index == 6 {
        return;
    }
    if time.daypart_index == 0 {
        if time.day_tick < 0xf1 {
            return;
        }
        if sample_rng(world) % 12 != 0 {
            return;
        }
    }
    dispatch_condo_active(world, time, sim_index);
}

/// condo_refresh_0x21 — at work (STATE_AT_WORK).
fn handle_condo_at_work(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    // refresh_0x21: baseOffset==2 branch fires a daypart earlier.
    if world.sims[sim_index as usize].base_offset == 2 {
        if time.daypart_index < 3 {
            return;
        }
        if time.daypart_index == 3 && sample_rng(world) % 12 != 0 {
            return;
        }
    } else {
        if time.daypart_index < 4 {
            return;
        }
        if time.daypart_index == 4 && sample_rng(world) % 12 != 0 {
            return;
        }
    }
    dispatch_condo_at_work(world, time, sim_index);
}

/// condo_refresh_0x22 — venue trip (STATE_VENUE_TRIP).
fn handle_condo_venue_trip(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState, sim_index: i64, _object: &str) {
    // refresh_0x22: daypart > 2 → dispatch.
    if time.daypart_index <= 2 {
        return;
    }
    // Binary release_commercial_venue_slot gates the exit on service_duration.
    let sim = &world.sims[sim_index as usize];
    if sim.venue_return_state != STATE_CHECKOUT_QUEUE && time.day_tick - sim.queue_tick < COMMERCIAL_VENUE_DWELL_TICKS {
        return;
    }
    dispatch_condo_venue_trip(world, time, sim_index);
}

/// Upstream's `(world, ledger, time, sim, object)`: the sim by its index in
/// `world.sims`, the object by its key in `world.placed_objects`.
pub type CondoHandler = fn(&mut WorldState, &mut LedgerState, &TimeState, i64, &str);

/// Family-9 (condo) refresh dispatch table (state_code → handler).
///
/// (A constant `Map`: pairs in insertion order; `js::record_get` looks one up.)
pub const CONDO_REFRESH_HANDLER_TABLE: &[(i64, CondoHandler)] = &[
    (STATE_MORNING_GATE, handle_condo_morning_gate),        // 0x20
    (STATE_MORNING_TRANSIT, handle_condo_morning_transit),  // 0x60 alias of 0x20
    (STATE_CHECKOUT_QUEUE, handle_condo_checkout_queue),    // 0x04
    (STATE_TRANSITION, handle_condo_transition),            // 0x10
    (STATE_COMMUTE, handle_condo_commute),                  // 0x00
    (STATE_COMMUTE_TRANSIT, handle_condo_commute_transit),  // 0x40 alias of 0x00
    (STATE_ACTIVE, handle_condo_active),                    // 0x01
    (STATE_ACTIVE_TRANSIT, handle_condo_active_transit),    // 0x41 (no resolve in binary;
    // per-leg progression here for our merged-segment model)
    (STATE_AT_WORK, handle_condo_at_work),                  // 0x21
    (STATE_AT_WORK_TRANSIT, handle_condo_at_work_transit),  // 0x61 alias of 0x21
    (STATE_VENUE_TRIP, handle_condo_venue_trip),            // 0x22
    (STATE_VENUE_HOME_TRANSIT, handle_condo_venue_home_transit), // 0x62
];

pub fn process_condo_sim(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64) {
    let Some(object) = find_object_for_sim(world, sim_index) else {
        return;
    };

    let handler = crate::js::record_get(CONDO_REFRESH_HANDLER_TABLE, world.sims[sim_index as usize].state_code);
    if let Some(handler) = handler {
        handler(world, ledger, time, sim_index, &object);
    }
}

/// dispatch_0x00 (COMMUTE) — route from home to LOBBY. Binary state 0x00 in the
/// condo dispatch maps:
///   rc=-1 → 0x40 (transit; retry)   [our model: CHECKOUT_QUEUE on hard-fail]
///   rc=0/1/2 → 0x40 (in transit)
///   rc=3 → arrival → 0x21 (AT_WORK)
///
/// Resolve owns sim.selectedFloor / sim.destinationFloor on rc=1/2.
fn dispatch_condo_commute(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let direction_flag = if floor_anchor > LOBBY_FLOOR { 0 } else { 1 };
    let result =
        resolve_sim_route_between_floors(world, sim_index, floor_anchor, LOBBY_FLOOR, direction_flag, Some(time), None);
    let sim = &mut world.sims[sim_index as usize];
    if result == -1 {
        sim.state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if result == 3 {
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_AT_WORK;
        return;
    }
    // rc=0/1/2: in transit. Don't overwrite resolve's per-leg writes.
    sim.state_code = STATE_COMMUTE_TRANSIT;
}

fn dispatch_condo_active(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    // dispatch_0x01 selector (1228:3b34-3b54):
    //   weekend_flag == 0               → 0 (retail)
    //   weekend_flag != 0, slot % 4 == 0 → 1 (restaurant)
    //   weekend_flag != 0, slot % 4 != 0 → 2 (fast food)
    let venue_families = if time.weekend_flag == 0 {
        CONDO_SELECTOR_RETAIL
    } else if world.sims[sim_index as usize].facility_slot % 4 == 0 {
        CONDO_SELECTOR_RESTAURANT
    } else {
        CONDO_SELECTOR_FAST_FOOD
    };
    // Per family-9 dispatch table (spec PEOPLE.md §0x22/0x62): on dwell complete /
    // home arrival, INC unit_status → STATE_CHECKOUT_QUEUE. Re-entry to STATE_ACTIVE
    // happens via STATE_TRANSITION (0x04 → 0x10 → 0x01), not directly off the dwell.
    let dispatched = dispatch_commercial_venue_visit(
        world,
        time,
        sim_index,
        venue_families,
        STATE_CHECKOUT_QUEUE,
        Some(STATE_ACTIVE_TRANSIT),
        None,
        Some(true),
        None,
        None,
    );
    if !dispatched {
        route_condo_to_lobby_no_venue(world, time, sim_index);
    }
}

/// Binary route_sim_to_commercial_venue (1238:0000) state-0x01 branch: when
/// `pickAvailableVenue` returns null, the helper still resolves a route from
/// the home floor to the lobby and forces state=0x41 (no failure path is
/// exposed to the family-9 dispatcher). Mirrors hotel's
/// `routeHotelToLobbyNoVenue`. The `venueReturnState = STATE_CHECKOUT_QUEUE`
/// marker steers `handleCondoSimArrival` to drop into STATE_VENUE_TRIP on
/// lobby arrival, matching the binary's hidden 0x22 transition.
fn route_condo_to_lobby_no_venue(world: &mut WorldState, time: &TimeState, sim_index: i64) {
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
    // rc=1/2: in transit. Don't overwrite resolve's per-leg writes.
    sim.state_code = STATE_ACTIVE_TRANSIT;
}

fn dispatch_condo_at_work(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let floor_anchor = world.sims[sim_index as usize].floor_anchor;
    let direction_flag = if floor_anchor > LOBBY_FLOOR { 1 } else { 0 };
    let result =
        resolve_sim_route_between_floors(world, sim_index, LOBBY_FLOOR, floor_anchor, direction_flag, Some(time), None);
    if result == -1 || result == 3 {
        world.sims[sim_index as usize].state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    // rc=0/1/2: in transit. Don't overwrite resolve's per-leg writes.
    world.sims[sim_index as usize].state_code = STATE_AT_WORK_TRANSIT;
}

fn dispatch_condo_venue_trip(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let sim = &world.sims[sim_index as usize];
    let (selected_floor, floor_anchor) = (sim.selected_floor, sim.floor_anchor);
    let direction_flag = if floor_anchor > selected_floor { 1 } else { 0 };
    let result =
        resolve_sim_route_between_floors(world, sim_index, selected_floor, floor_anchor, direction_flag, Some(time), None);
    if result == -1 || result == 3 {
        world.sims[sim_index as usize].state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    // rc=0/1/2: in transit. Don't overwrite resolve's per-leg writes.
    world.sims[sim_index as usize].state_code = STATE_VENUE_HOME_TRANSIT;
}

pub fn handle_condo_sim_arrival(sim: &mut SimRecord, arrival_floor: i64, time: &TimeState, arrived_via_carrier: bool) {
    // Binary's per-stride 0x60/0x40/0x41/0x61/0x62 handler re-resolves(target,
    // target)=3 on arrival, draining stress via advance_sim_trip_counters at
    // 1218:0046. The carrier-delivery shortcut (queue/dispatch-arrivals.ts)
    // invokes this dispatcher instead of running the per-stride handler, so
    // the advance must fire here for carrier-routed arrivals. Stair/segment
    // arrivals reach this dispatcher via the per-stride handler's rc=3 →
    // dispatchSimArrival path, where resolve already advanced trip counters,
    // so we gate on `arrivedViaCarrier` (captured pre-clearSimRoute) to avoid
    // double-counting.
    if sim.state_code == STATE_MORNING_TRANSIT && arrival_floor == sim.floor_anchor {
        // Binary 1228:3b71 state-0x20 trip target is sim.floorAnchor (home),
        // not LOBBY: morning gate is the new resident moving lobby → home.
        if arrived_via_carrier {
            advance_sim_trip_counters(sim);
        }
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if sim.state_code == STATE_COMMUTE_TRANSIT && arrival_floor == LOBBY_FLOOR {
        if arrived_via_carrier {
            advance_sim_trip_counters(sim);
        }
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_AT_WORK;
        return;
    }
    if sim.state_code == STATE_ACTIVE_TRANSIT
        && arrival_floor == LOBBY_FLOOR
        && sim.venue_return_state == STATE_CHECKOUT_QUEUE
    {
        // No-venue fallback arrival: binary 1238:0000 state-0x01 lobby path
        // (acquire_slot(-1)=3 fall-through) lands in state 0x22 (VENUE_TRIP).
        // The existing 0x22/0x62 handler then unwinds via 0x04 (CHECKOUT_QUEUE).
        if arrived_via_carrier {
            advance_sim_trip_counters(sim);
        }
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        return;
    }
    if sim.state_code == STATE_ACTIVE_TRANSIT && arrival_floor != LOBBY_FLOOR {
        // Condo arrived at a real commercial venue: binary 1228:4fab writes
        // state=0x22 (VENUE_TRIP) with queueTick latched for the dwell gate.
        // Clear venueReturnState so the 0x22 handler treats this as real-venue
        // (binary release_commercial_venue_slot gates on service_duration when
        // facilitySlot >= 0; stale CHECKOUT_QUEUE marker from a prior fake-lunch
        // must not short-circuit the dwell).
        if arrived_via_carrier {
            advance_sim_trip_counters(sim);
        }
        sim.destination_floor = -1;
        sim.selected_floor = arrival_floor;
        sim.state_code = STATE_VENUE_TRIP;
        sim.queue_tick = time.day_tick;
        sim.venue_return_state = 0;
        return;
    }
    if sim.state_code == STATE_AT_WORK_TRANSIT && arrival_floor == sim.floor_anchor {
        // Spec PEOPLE.md §0x21/0x61: arrived → INC unit_status → 0x04. Re-entry
        // to ACTIVE happens via STATE_TRANSITION (0x04 → 0x10 → 0x01).
        if arrived_via_carrier {
            advance_sim_trip_counters(sim);
        }
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
        return;
    }
    if sim.state_code == STATE_VENUE_HOME_TRANSIT && arrival_floor == sim.floor_anchor {
        // Binary family-9 dispatch table: state 0x22/0x62 fail/arrived →
        // INC unit_status → 0x04 (CHECKOUT_QUEUE). Arrival here is the
        // "arrived" branch.
        if arrived_via_carrier {
            advance_sim_trip_counters(sim);
        }
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        sim.state_code = STATE_CHECKOUT_QUEUE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim() -> SimRecord {
        SimRecord {
            floor_anchor: 20,
            home_column: 40,
            base_offset: 0,
            facility_slot: 0,
            family_code: FAMILY_CONDO,
            state_code: STATE_ACTIVE_TRANSIT,
            route: RouteState::Idle,
            selected_floor: 20,
            origin_floor: 20,
            destination_floor: 30,
            venue_return_state: STATE_CHECKOUT_QUEUE,
            queue_tick: 0,
            elapsed_ticks: 0,
            transit_ticks_remaining: 0,
            last_demand_tick: -1,
            trip_count: 0,
            accumulated_ticks: 0,
            target_room_floor: -1,
            target_room_column: -1,
            spawn_floor: 20,
            post_claim_countdown: 0,
            encoded_target_floor: 0,
            commercial_venue_slot: -1,
        }
    }

    #[test]
    fn arrival_at_a_real_venue_clears_the_fake_lunch_marker() {
        // (arrivedViaCarrier false: no trip-counter advance, which is a stub
        // until stress/ is translated.)
        let mut t = crate::time::create_time_state();
        t.day_tick = 777;
        let mut s = sim();
        handle_condo_sim_arrival(&mut s, 30, &t, false);
        assert_eq!((s.state_code, s.selected_floor, s.destination_floor), (STATE_VENUE_TRIP, 30, -1));
        assert_eq!((s.queue_tick, s.venue_return_state), (777, 0));
        // The lobby with the marker set: venue trip, marker kept.
        let mut s = sim();
        handle_condo_sim_arrival(&mut s, LOBBY_FLOOR, &t, false);
        assert_eq!((s.state_code, s.venue_return_state), (STATE_VENUE_TRIP, STATE_CHECKOUT_QUEUE));
        // A state no branch takes: untouched.
        let mut s = sim();
        s.state_code = STATE_AT_WORK;
        handle_condo_sim_arrival(&mut s, 20, &t, false);
        assert_eq!(s, sim_with_state(STATE_AT_WORK));
    }

    fn sim_with_state(state: i64) -> SimRecord {
        let mut s = sim();
        s.state_code = state;
        s
    }
}

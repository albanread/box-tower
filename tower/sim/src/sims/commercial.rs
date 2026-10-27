// ts: sims/commercial.ts
//
// Upstream holds `sim` (an element of world.sims), `object` (a value of
// world.placedObjects) and `record`/`venue` (an element of world.sidecars)
// as references across calls that take `world`.  None of those calls adds
// or removes sims, placed objects or sidecars, so here they are reached
// through `sim_index`, the object's key and the sidecar index each time.

use crate::ledger::LedgerState;
use crate::queue::resolve::{resolve_sim_route_between_floors, ResolveSimRouteOptions};
use crate::resources::{FAMILY_RESTAURANT, FAMILY_RETAIL};
use crate::sims::facility_refunds::{activate_retail_shop, increment_venue_seed};
use crate::sims::population::{clear_sim_route, find_object_for_sim};
use crate::sims::states::{
    COMMERCIAL_VENUE_DWELL_TICKS, LOBBY_FLOOR, STATE_DEPARTURE, STATE_DEPARTURE_TRANSIT,
    STATE_MORNING_GATE, STATE_MORNING_TRANSIT, STATE_NIGHT_A, STATE_NIGHT_B, STATE_PARKED,
};
use crate::sims::{release_service_request, try_acquire_office_venue_slot, VENUE_SLOT_FULL};
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::time::TimeState;
use crate::world::{
    sample_rng, CommercialVenueRecord, SidecarRecord, WorldState, VENUE_CLOSED, VENUE_DORMANT,
};

/// `world.sidecars[index]` when it is a commercial venue record (upstream's
/// `venue?.kind === "commercial_venue"` after the lookup); `None` where
/// JavaScript reads `undefined` or another kind.
fn venue_mut(sidecars: &mut [SidecarRecord], index: i64) -> Option<&mut CommercialVenueRecord> {
    if index < 0 {
        return None;
    }
    match sidecars.get_mut(index as usize) {
        Some(SidecarRecord::CommercialVenue(v)) => Some(v),
        _ => None,
    }
}

/// `tryAcquireOfficeVenueSlot(venue, sim, time, sim.familyCode)` with the
/// venue and the sim reached through the world.
fn acquire_own_venue_slot(world: &mut WorldState, record_index: i64, sim_index: i64, time: &TimeState) -> i64 {
    let WorldState { sidecars, sims, .. } = world;
    let sim = &mut sims[sim_index as usize];
    let family_code = sim.family_code;
    let venue = venue_mut(sidecars, record_index).expect("not a commercial venue record");
    try_acquire_office_venue_slot(venue, sim, time, family_code)
}

pub fn process_commercial_sim(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let Some(object_key) = find_object_for_sim(world, sim_index) else {
        return;
    };
    let (linked_record_index, occupied_flag) = {
        let object = &world.object(object_key.as_str()).unwrap();
        (object.linked_record_index, object.occupied_flag)
    };

    let state = world.sims[si].state_code;

    // --- Night / parked states ---
    if state == STATE_NIGHT_A || state == STATE_NIGHT_B || state == STATE_PARKED {
        if time.day_tick > 2300 {
            world.sims[si].state_code = STATE_MORNING_GATE;
        }
        return;
    }

    // --- Morning activation gate ---
    // Binary: gate_object_family_retail_state_handler (1228:3ed9) and
    // gate_object_family_restaurant_fast_food_state_handler (1228:466d).
    // The two gates differ: retail has an early DORMANT+occupiedFlag exit
    // *before* any RNG; restaurant/fast-food has no occupied-flag check.
    if state == STATE_MORNING_GATE {
        let family_code = world.sims[si].family_code;
        if family_code == FAMILY_RETAIL {
            // Binary 1228:4014/4044 retail gate: return early when the venue is
            // dormant AND the object has not yet been marked operationally
            // occupied (+0x14). Non-dormant venues always fall through to RNG
            // gates. 1228:4044: CMP byte ptr ES:[BX+0x14],0x0.
            if linked_record_index >= 0 {
                if let Some(venue) = venue_mut(&mut world.sidecars, linked_record_index) {
                    if venue.availability_state == VENUE_DORMANT && occupied_flag == 0 {
                        return;
                    }
                }
            }
        }

        if family_code == FAMILY_RESTAURANT {
            // Binary 1228:466d restaurant branch:
            // dp4 → 1/12 RNG gate → dispatch; dp<5 → return;
            // dp5 with dayTick<=2199 → dispatch; else return.
            if time.daypart_index == 4 {
                if sample_rng(world) % 12 != 0 {
                    return;
                }
            } else if time.daypart_index < 5 {
                return;
            } else if time.day_tick > 2199 {
                return;
            }
        } else {
            // Binary 1228:3ed9 / 1228:466d fast-food+retail branch:
            // dp>=5 → return; dayTick<241 → return;
            // dp0-3 → 1/36 RNG gate; dp4 → 1/6 RNG gate.
            if time.daypart_index >= 5 {
                return;
            }
            if time.day_tick < 0xf1 {
                return;
            }
            if time.daypart_index <= 3 {
                if sample_rng(world) % 36 != 0 {
                    return;
                }
            } else if sample_rng(world) % 6 != 0 {
                return;
            }
        }

        // Dispatch. Restaurant/fast-food share dispatch_object_family_6_0c
        // (1228:4851) which silent-parks when availabilityState == CLOSED before
        // calling try_consume_commercial_venue_capacity. Retail's dispatch
        // (1228:40c0) has no silent-park branch — it only consumes capacity.
        if linked_record_index < 0 {
            return;
        }
        let Some(record) = venue_mut(&mut world.sidecars, linked_record_index) else {
            return;
        };
        let sim = &mut world.sims[si];

        if sim.family_code != FAMILY_RETAIL {
            // Restaurant/fast-food state-0x20 silent-park (1228:499c):
            // if venue.availabilityState == VENUE_CLOSED, set sim to PARKED and
            // return — RNG already consumed by the gate, no transition emitted.
            if record.availability_state == VENUE_CLOSED {
                sim.state_code = STATE_PARKED;
                return;
            }
        }

        // try_consume_commercial_venue_capacity (11b0:1150). Eligibility-threshold
        // check only fires when the threshold is signed-negative; otherwise the
        // baseOffset comparison is skipped.
        if record.remaining_capacity <= 0 {
            return;
        }
        if record.eligibility_threshold < 0 && sim.base_offset > 1 - record.eligibility_threshold {
            return;
        }

        // Binary `try_consume_commercial_venue_capacity` (11b0:1150) decrements
        // record+6 (remainingCapacity), increments record+7 (todayVisitCount,
        // staff-emit count), and increments record+0x10 (acquireCount). It
        // does NOT touch record+9 (currentPopulation) — that field is
        // incremented only by `acquire_commercial_venue_slot` (11b0:0d92),
        // which the binary calls at the per-stride state-0x20/0x60 handler
        // ONLY when route_result == 3 (i.e. the sim has actually arrived at
        // the venue floor). Mirror that here: at MORNING_GATE we run
        // try_consume but defer the currentPopulation increment to the
        // arrival site (handleCommercialMorningTransit /
        // handleCommercialSimArrival).
        // The phaseA/phaseB seed grow (binary `clamp_object_type_limit`) is
        // NOT called here — it fires only on the successful departure path
        // (state→0x27 PARKED via rc=3 in DEPARTURE / DEPARTURE_TRANSIT /
        // carrier arrival). See those sites below.
        record.remaining_capacity -= 1;
        record.last_acquire_tick = time.day_tick;
        record.today_visit_count += 1;
        record.acquire_count += 1;
        record.visit_count += 1;

        // Route to home floor. Binary state-0x20 dispatch (1228:41cb) calls
        // try_consume first, then resolve_sim_route_between_floors, then
        // activates the retail shop on the success paths (route_result 0..3)
        // only when the venue is still DORMANT. On route_result == -1 the
        // binary reverts sim state fields and skips activation — capacity
        // stays decremented but no cashflow is added.
        let floor_anchor = sim.floor_anchor;
        let route_result = resolve_sim_route_between_floors(
            world,
            sim_index,
            LOBBY_FLOOR,
            floor_anchor,
            if floor_anchor > LOBBY_FLOOR { 1 } else { 0 },
            Some(time),
            None,
        );
        let record = venue_mut(&mut world.sidecars, linked_record_index).unwrap();
        let sim = &mut world.sims[si];
        if route_result == -1 {
            // Binary 1228:4297 route_result==-1 handler. Branch on venue[+0x02]:
            //   UNTENANTED (availabilityState == 0xff / VENUE_DORMANT, 1228:42d8):
            //     clear sim[+9]/sim[+0xc]/sim[+0xe] and CALLF 11b0:11de, which
            //     reverts try_consume (rec[+6]++, rec[+7]--, rec[+0x10]--). Sim
            //     stays in MORNING_GATE to retry next tick.
            //   TENANTED (1228:45dd): sim[+5]=0x27 (PARKED), clamp_object_type_limit.
            if record.availability_state == VENUE_DORMANT {
                sim.trip_count = 0;
                sim.elapsed_ticks = 0;
                sim.accumulated_ticks = 0;
                record.remaining_capacity += 1;
                record.today_visit_count -= 1;
                record.acquire_count -= 1;
                record.visit_count -= 1;
            } else {
                sim.state_code = STATE_PARKED;
            }
            return;
        }
        if sim.family_code == FAMILY_RETAIL && record.availability_state == VENUE_DORMANT {
            activate_retail_shop(world, &object_key, linked_record_index, ledger);
        }
        if route_result == 3 {
            // Same-floor venue (LOBBY-anchored): binary state-0x20 handler at
            // rc=3 (1228:4415 retail / 1228:4b37 ff) immediately calls
            // `acquire_commercial_venue_slot` (11b0:0d92):
            //   acquire returns -1 (UNAVAILABLE) → state 0x05 (DEPARTURE)
            //   acquire returns 2 (FULL)        → state 0x60 (MORNING_TRANSIT,
            //                                     re-attempt acquire next stride)
            //   acquire returns 3 (ACQUIRED)    → state 0x05 (DEPARTURE)
            // Acquire stamps sim+0xa = g_day_tick on success/full paths; that's
            // the dwell-start latch read by release.
            let sim = &mut world.sims[si];
            sim.destination_floor = -1;
            sim.selected_floor = sim.floor_anchor;
            // Owner sim arriving at their own venue: pass sim.familyCode as the
            // venue owner family so the type/variant gate suppresses the
            // acquireCount bump (binary 11b0:0f3a–0f55: same family ⇒ skip).
            // The owner's visit was already counted by
            // try_consume_commercial_venue_capacity at MORNING_GATE above.
            let acquire_result = acquire_own_venue_slot(world, linked_record_index, sim_index, time);
            let sim = &mut world.sims[si];
            if acquire_result == VENUE_SLOT_FULL {
                sim.state_code = STATE_MORNING_TRANSIT;
            } else {
                sim.state_code = STATE_DEPARTURE;
                sim.elapsed_ticks = 0;
                sim.last_demand_tick = time.day_tick;
            }
        } else {
            world.sims[si].state_code = STATE_MORNING_TRANSIT;
        }
        return;
    }

    // --- Departure ---
    // Binary state-5 handler (1228:4517 retail / 1228:4bd7 ff+restaurant).
    // CALLF release_commercial_venue_slot (11b0:0fae) gate:
    //   g_day_tick - sim[+0x0a] < service_duration → return 0 (still dwelling).
    // sim[+0x0a] is stamped ONCE by acquire_commercial_venue_slot at service
    // start (handleCommercialSimArrival); binary never re-stamps it during
    // dwell and never rebases elapsed on state 0x05. Using dayTick - ldt as
    // the gate preserves elapsed=0 across the dwell so the return-leg stair
    // penalty of 35 is the only accumulated stress contribution.
    if state == STATE_DEPARTURE {
        if time.day_tick - world.sims[si].last_demand_tick < COMMERCIAL_VENUE_DWELL_TICKS {
            return;
        }
        // Binary retail/restaurant state-0x05 handler (1228:454e / 1228:4c0e)
        // calls `release_commercial_venue_slot` (11b0:0fae) which decrements
        // record+9 (currentPopulation) on the success path. Mirror that here
        // so the venue's effective capacity gate is restored for visiting
        // office workers / hotel guests / entertainment guests.
        if linked_record_index >= 0 {
            if let Some(venue) = venue_mut(&mut world.sidecars, linked_record_index) {
                if venue.availability_state != VENUE_DORMANT
                    && venue.availability_state != VENUE_CLOSED
                    && venue.current_population > 0
                {
                    venue.current_population -= 1;
                }
            }
        }
        // Binary retail/restaurant state-0x05 handler (1228:4517 / 1228:4bd7):
        // src = sim+7 lookup (current/home floor), tgt = LOBBY (0xa).
        // rc=-1 → fail; rc=0/1/2 → ok (in-transit); rc=3 → fail (binary collapses
        // the same-floor return into the fail tail at 1228:45a8 / 1228:4c68).
        let source_floor = world.sims[si].selected_floor;
        let route_result = resolve_sim_route_between_floors(
            world,
            sim_index,
            source_floor,
            LOBBY_FLOOR,
            if LOBBY_FLOOR > source_floor { 1 } else { 0 },
            Some(time),
            None,
        );
        if route_result == -1 || route_result == 3 {
            // Binary 1228:4c9e: state-0x05/0x45 jump table routes rc=-1 and rc=3
            // to the PARKED (0x27) writer, not NIGHT_B (0x26).
            release_service_request(world, sim_index);
            world.sims[si].state_code = STATE_PARKED;
            // Binary `clamp_object_type_limit` fires after `[BX+5]=0x27` on the
            // rc=3 (successful arrival at lobby) path only — see 1228:4cbf →
            // 1228:4d24. rc=-1 (route fail) sets PARKED but does NOT call clamp.
            if route_result == 3 && linked_record_index >= 0 {
                if venue_mut(&mut world.sidecars, linked_record_index).is_some() {
                    let family_code = world.sims[si].family_code;
                    increment_venue_seed(linked_record_index, family_code, sim_index, world, time);
                }
            }
            return;
        }
        // Phase 1d-ii parity: resolve owns sim.selectedFloor/destinationFloor.
        // rc=0/1/2 → in-transit; next stride re-enters via STATE_DEPARTURE_TRANSIT.
        world.sims[si].state_code = STATE_DEPARTURE_TRANSIT;
        return;
    }

    // --- Per-stride in-transit handlers ---
    // Binary jump table aliases: 0x60 (MORNING_TRANSIT) → state 0x20 handler;
    // 0x45 (DEPARTURE_TRANSIT) → state 0x05 handler. variantFlag=0 (distance
    // feedback off), but our resolver applies the penalty unconditionally.
    if state == STATE_MORNING_TRANSIT {
        handle_commercial_morning_transit(world, ledger, time, sim_index);
        return;
    }
    if state == STATE_DEPARTURE_TRANSIT {
        handle_commercial_departure_transit(world, time, sim_index);
    }
}

/// Per-stride in-transit handler for STATE_MORNING_TRANSIT (0x60).
/// Binary alias of state 0x20 (1228:41cb retail / 1228:495c ff+restaurant) with
/// variantFlag=0. src = sim+7 (last leg endpoint or home), tgt = floorAnchor.
///   rc=-1 → fail (park).
///   rc=0/1/2 → stay in MORNING_TRANSIT; next stride re-resolves the next leg.
///   rc=3 → arrived at home venue; activate retail (DORMANT case) and call
///          acquire_commercial_venue_slot. ACQUIRED/UNAVAILABLE → state 0x05;
///          FULL → stay in 0x60 (re-attempt acquire next stride).
fn handle_commercial_morning_transit(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let object_key = find_object_for_sim(world, sim_index);
    let source_floor = world.sims[si].selected_floor;
    let target_floor = world.sims[si].floor_anchor;
    // Alias state 0x60 (MORNING_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here. Distance feedback was already applied by the base state
    // 0x20 dispatch.
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        target_floor,
        if target_floor > source_floor { 1 } else { 0 },
        Some(time),
        Some(ResolveSimRouteOptions {
            emit_distance_feedback: Some(false),
            ..Default::default()
        }),
    );
    if route_result == -1 {
        // Binary 1228:4297 -1 path: TENANTED venue → STATE_PARKED.
        world.sims[si].state_code = STATE_PARKED;
        release_service_request(world, sim_index);
        return;
    }
    if route_result == 3 {
        // Arrived at home venue floor. Trip counter advanced inside resolve.
        // Binary state-0x60 handler at rc=3 (1228:4415 retail / 1228:4b37 ff)
        // calls `acquire_commercial_venue_slot` (11b0:0d92):
        //   acquire returns -1 (UNAVAILABLE) → state 0x05 (DEPARTURE)
        //   acquire returns 2  (FULL)        → stay in 0x60, retry next stride
        //   acquire returns 3  (ACQUIRED)    → state 0x05 (DEPARTURE)
        // Acquire stamps sim+0xa = g_day_tick on success/full paths; that's
        // the dwell-start latch read by release.
        let sim = &mut world.sims[si];
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        // Activate retail shop on arrival when venue is still DORMANT (binary
        // 1228:42d0 fires activate on success paths). This must happen before
        // the acquire call because acquire short-circuits to UNAVAILABLE on
        // dormant venues; activation flips availabilityState to OPEN first.
        let mut venue: Option<i64> = None;
        if let Some(object_key) = &object_key {
            let linked = world.object(object_key.as_str()).unwrap().linked_record_index;
            if linked >= 0 {
                if let Some(candidate) = venue_mut(&mut world.sidecars, linked) {
                    venue = Some(linked);
                    if world.sims[si].family_code == FAMILY_RETAIL && candidate.availability_state == VENUE_DORMANT {
                        activate_retail_shop(world, object_key, linked, ledger);
                    }
                }
            }
        }
        let Some(venue) = venue else {
            let sim = &mut world.sims[si];
            sim.state_code = STATE_DEPARTURE;
            sim.elapsed_ticks = 0;
            sim.last_demand_tick = time.day_tick;
            return;
        };
        // Owner sim arriving at their own venue via MORNING_TRANSIT: pass
        // sim.familyCode as the venue owner family so the type/variant gate
        // suppresses the acquireCount bump (already counted at MORNING_GATE).
        let acquire_result = acquire_own_venue_slot(world, venue, sim_index, time);
        if acquire_result == VENUE_SLOT_FULL {
            // Stay in STATE_MORNING_TRANSIT; next stride retries acquire.
            return;
        }
        let sim = &mut world.sims[si];
        sim.state_code = STATE_DEPARTURE;
        sim.elapsed_ticks = 0;
        sim.last_demand_tick = time.day_tick;
    }
    // rc=0/1/2: stay in STATE_MORNING_TRANSIT; next stride re-resolves.
}

/// Per-stride in-transit handler for STATE_DEPARTURE_TRANSIT (0x45).
/// Binary alias of state 0x05 (1228:4517 retail / 1228:4bd7 ff+restaurant) with
/// variantFlag=0. src = sim+7 (last leg endpoint), tgt = LOBBY.
///   rc=-1 → fail (park).
///   rc=0/1/2 → stay in DEPARTURE_TRANSIT; next stride re-resolves next leg.
///   rc=3 → arrived at LOBBY; binary collapses to fail tail per state 0x05 table,
///          but at the transit alias the same-floor result represents arrival —
///          park the sim and release the service request.
fn handle_commercial_departure_transit(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let source_floor = world.sims[si].selected_floor;
    // Alias state 0x45 (DEPARTURE_TRANSIT): in the binary `emit_distance_feedback`
    // is `0` here. Distance feedback was already applied by the base state
    // 0x05 dispatch.
    let route_result = resolve_sim_route_between_floors(
        world,
        sim_index,
        source_floor,
        LOBBY_FLOOR,
        if LOBBY_FLOOR > source_floor { 1 } else { 0 },
        Some(time),
        Some(ResolveSimRouteOptions {
            emit_distance_feedback: Some(false),
            ..Default::default()
        }),
    );
    if route_result == -1 {
        // Binary 1228:4c9e: state-0x45 rc=-1 writes PARKED (0x27), not NIGHT_B.
        release_service_request(world, sim_index);
        world.sims[si].state_code = STATE_PARKED;
        return;
    }
    if route_result == 3 {
        // Arrived at lobby. Trip counter advanced inside resolve.
        let sim = &mut world.sims[si];
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_PARKED;
        release_service_request(world, sim_index);
        // Binary `clamp_object_type_limit` fires after the rc=3 state→0x27
        // transition (1228:4cbf → 1228:4d24).
        increment_seed_of_home_venue(world, sim_index, time);
    }
    // rc=0/1/2: stay in STATE_DEPARTURE_TRANSIT; next stride re-resolves.
}

/// The tail upstream writes out at each rc=3 departure site:
/// `findObjectForSim`, then `incrementVenueSeed` if the object's linked
/// record is a commercial venue.
fn increment_seed_of_home_venue(world: &mut WorldState, sim_index: i64, time: &TimeState) {
    let Some(object_key) = find_object_for_sim(world, sim_index) else {
        return;
    };
    let linked = world.object(object_key.as_str()).unwrap().linked_record_index;
    if linked >= 0 && venue_mut(&mut world.sidecars, linked).is_some() {
        let family_code = world.sims[sim_index as usize].family_code;
        increment_venue_seed(linked, family_code, sim_index, world, time);
    }
}

pub fn handle_commercial_sim_arrival(world: &mut WorldState, sim_index: i64, arrival_floor: i64, time: &TimeState) {
    let si = sim_index as usize;
    if world.sims[si].state_code == STATE_MORNING_TRANSIT && arrival_floor == world.sims[si].floor_anchor {
        // Binary `dispatch_destination_queue_entries` (1218:0883) writes
        // sim+7 = arrival_floor then invokes the family handler with
        // arg = arrival_floor. For state 0x60 (alias of 0x20) the resolve
        // call's same-floor branch (1218:0046, gated on is_passenger_route=1
        // which restaurant/fast-food/retail pass) advances the trip counters
        // before returning rc=3. Mirror that advance here because
        // dispatchSimArrival shortcuts the per-stride re-entry.
        //
        // At rc=3 the binary state-0x60 handler (1228:4415 retail / 1228:4b37
        // ff) calls `acquire_commercial_venue_slot` (11b0:0d92):
        //   acquire returns -1 (UNAVAILABLE) → state 0x05 (DEPARTURE)
        //   acquire returns 2  (FULL)        → stay in 0x60, retry next stride
        //   acquire returns 3  (ACQUIRED)    → state 0x05 (DEPARTURE)
        // Acquire stamps sim+0xa = g_day_tick on success/full paths; that's
        // the dwell-start latch read by release.
        let sim = &mut world.sims[si];
        advance_sim_trip_counters(sim);
        sim.destination_floor = -1;
        sim.selected_floor = sim.floor_anchor;
        let object_key = find_object_for_sim(world, sim_index);
        let mut venue: Option<i64> = None;
        if let Some(object_key) = &object_key {
            let linked = world.object(object_key.as_str()).unwrap().linked_record_index;
            if linked >= 0 && venue_mut(&mut world.sidecars, linked).is_some() {
                venue = Some(linked);
            }
        }
        let Some(venue) = venue else {
            let sim = &mut world.sims[si];
            sim.state_code = STATE_DEPARTURE;
            sim.elapsed_ticks = 0;
            sim.last_demand_tick = time.day_tick;
            return;
        };
        // Owner sim arriving at their own venue via carrier (handleCommercialSimArrival):
        // pass sim.familyCode as the venue owner family so the type/variant gate
        // suppresses the acquireCount bump (already counted at MORNING_GATE).
        let acquire_result = acquire_own_venue_slot(world, venue, sim_index, time);
        let sim = &mut world.sims[si];
        if acquire_result == VENUE_SLOT_FULL {
            // Stay in STATE_MORNING_TRANSIT; next stride re-attempts acquire.
            // Restore route fields the per-stride handler expects.
            sim.destination_floor = -1;
            return;
        }
        sim.state_code = STATE_DEPARTURE;
        // Binary: sim[+0x0A] (dword last_activity_tick) is stamped at dwell
        // start; release_commercial_venue_slot uses it to gate the service
        // duration check. Track via elapsedTicks (rebased each stride).
        sim.elapsed_ticks = 0;
        sim.last_demand_tick = time.day_tick;
        return;
    }

    if world.sims[si].state_code == STATE_DEPARTURE_TRANSIT && arrival_floor == LOBBY_FLOOR {
        // Binary 1228:4bd7 (state 0x45 alias of 0x05) calls resolve with
        // `is_passenger_route=1`. At carrier arrival sim+7 == LOBBY (target),
        // so resolve hits the same-floor branch (1218:0046) which calls
        // advance_sim_trip_counters because is_passenger_route != 0.
        let sim = &mut world.sims[si];
        advance_sim_trip_counters(sim);
        sim.state_code = STATE_PARKED;
        sim.selected_floor = LOBBY_FLOOR;
        release_service_request(world, sim_index);
        // Binary `clamp_object_type_limit` fires after the state→0x27 write on
        // the carrier-arrival rc=3 path (dispatch_sim_behavior 1228:1ba6 case
        // 6/10/12).
        increment_seed_of_home_venue(world, sim_index, time);
        return;
    }

    // Fallback: park
    let sim = &mut world.sims[si];
    sim.state_code = STATE_NIGHT_B;
    clear_sim_route(sim);
}

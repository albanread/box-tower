// ts: queue/resolve.ts

// 1218:0000 resolve_sim_route_between_floors
//
// Binary-faithful route resolver. Consults the route scorer, then either
// (a) walks the sim over a special-link segment, (b) enqueues onto a
// carrier queue, or (c) reports no-route / same-floor.

use super::enqueue::enqueue_request_into_route_queue;
use crate::js;
use crate::resources::{
    FAMILY_CATHEDRAL_BASE, FAMILY_CATHEDRAL_MAX, FAMILY_CINEMA, FAMILY_CINEMA_LOWER, FAMILY_CINEMA_STAIRS_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER, FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE,
    FAMILY_HOTEL_TWIN, FAMILY_HOUSEKEEPING, FAMILY_OFFICE, FAMILY_PARTY_HALL, FAMILY_PARTY_HALL_LOWER,
    FAMILY_RESTAURANT, FAMILY_RETAIL,
};
use crate::route_scoring::delay_table::perStopParityDelay;
use crate::route_scoring::select_candidate::{select_best_route_candidate, RouteCandidate, RouteCandidateKind};
use crate::route_scoring::select_cathedral::select_cathedral_route;
use crate::route_scoring::select_housekeeping::select_housekeeping_route;
use crate::sim_access::state_bits::{set_sim_in_transit, set_sim_waiting};
use crate::sims::population::{clear_sim_route, sim_key};
use crate::sims::scoring::maybe_apply_distance_feedback;
use crate::sims::states::LOBBY_FLOOR;
use crate::stress::add_delay::add_delay_to_current_sim;
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::time::TimeState;
use crate::world::{RouteDirection, RouteState, WorldState};

/// Family → route-selector dispatch.
///
/// Earlier comments here claimed the binary's `assign_request_to_runtime_route`
/// (1218:0d4e) hosted per-family *route* selectors for families
/// {0x0f, 0x12, 0x1d, 0x21, 0x24}. That was a misreading: the jump table at
/// 1218:0f4b dispatches to per-family **target-floor** selectors
/// (`get_housekeeping_room_claim_selector` 1228:6757,
/// `dispatch_entertainment_guest_substate` 1228:662a,
/// `resolve_family_recycling_center_lower_selector_value` 1228:65c1,
/// `resolve_family_parking_selector_value` 1228:6700) whose results feed
/// `choose_transfer_floor_from_carrier_reachability` (11b8:0e41) on the
/// carrier-queue boarding path — not `select_best_route_candidate` (11b8:1484).
///
/// The actual route selector (`select_best_route_candidate`) is family-agnostic.
/// Family differentiation happens only via the `is_passenger_route` argument
/// passed to `resolve_sim_route_between_floors` (1218:0000), which forwards it
/// as `prefer_local_mode` to `select_best_route_candidate`. Per the binary
/// call-site survey:
///
///   - Housekeeping (0x0f): `is_passenger_route = 0` (service mode).
///     Sources: `update_object_family_housekeeping_connection_state`
///     (1228:602b) at 1228:620f and 1228:6320.
///   - Cathedral (0x24-0x28): `is_passenger_route = 1` (passenger mode).
///     Sources: `handle_family_parking_outbound_route` (1228:5ddd) and
///     `handle_family_parking_return_route` (1228:5e7e).
///   - All other passenger families: `is_passenger_route = 1`.
///
/// The per-family wrappers below make the family→selector mapping explicit and
/// binary-traceable; each is a thin shim over `selectBestRouteCandidate` with
/// the binary-correct `prefer_local_mode` value baked in.
const SHARED_ROUTE_SELECTOR_FAMILIES: &[i64] = &[
    FAMILY_HOTEL_SINGLE,
    FAMILY_HOTEL_TWIN,
    FAMILY_HOTEL_SUITE,
    FAMILY_RESTAURANT,
    FAMILY_OFFICE,
    FAMILY_CONDO,
    FAMILY_FAST_FOOD,
    FAMILY_RETAIL,
    FAMILY_CINEMA,
    FAMILY_CINEMA_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CINEMA_STAIRS_LOWER,
    FAMILY_PARTY_HALL,
    FAMILY_PARTY_HALL_LOWER,
];

fn is_cathedral_family(family_code: i64) -> bool {
    family_code >= FAMILY_CATHEDRAL_BASE && family_code <= FAMILY_CATHEDRAL_MAX
}

/// Phase 5b: families whose state machine reads the 0x40 / 0x20 bits of
/// `sim.stateCode` through the binary's `dispatch_sim_behavior` (1228:186c)
/// two-tier switch. See ROUTING-BINARY-MAP.md §4.2 + families/state-tables/
/// family-prologue.ts (cs:1c71). Other families have their own low-valued
/// state machines (housekeeping: 0..4; cathedral: family-specific; etc.) and
/// are NOT driven by the 0x40 / 0x20 bits, so resolver-side state-bit sync
/// would corrupt their states vs. the reference trace.
const STATE_BIT_FAMILIES: &[i64] = &[
    FAMILY_HOTEL_SINGLE,
    FAMILY_HOTEL_TWIN,
    FAMILY_HOTEL_SUITE,
    FAMILY_RESTAURANT,
    FAMILY_OFFICE,
    FAMILY_CONDO,
    FAMILY_FAST_FOOD,
    FAMILY_RETAIL,
    FAMILY_CINEMA,
    FAMILY_CINEMA_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CINEMA_STAIRS_LOWER,
    FAMILY_PARTY_HALL,
    FAMILY_PARTY_HALL_LOWER,
];

fn family_uses_state_bits(family_code: i64) -> bool {
    STATE_BIT_FAMILIES.contains(&family_code)
}

fn select_route_for_family(
    world: &WorldState,
    family_code: i64,
    from_floor: i64,
    to_floor: i64,
    target_height_metric: i64,
) -> Option<RouteCandidate> {
    if family_code == FAMILY_HOUSEKEEPING {
        return select_housekeeping_route(world, from_floor, to_floor, target_height_metric);
    }
    if is_cathedral_family(family_code) {
        return select_cathedral_route(world, from_floor, to_floor, target_height_metric);
    }
    if SHARED_ROUTE_SELECTOR_FAMILIES.contains(&family_code) {
        return select_best_route_candidate(
            world,
            from_floor,
            to_floor,
            /* preferLocalMode = */ Some(true),
            Some(target_height_metric),
        );
    }
    None
}

/// Return codes mirror `resolveSimRouteBetweenFloors` from
/// ROUTING.md / SPEC.md:
///
///  -1 = no viable route (sim remains unrouted)
///   0 = carrier queue full; sim[+8] = 0xff and sim[+7] = source floor,
///       so the sim stays parked on the source floor and retries next tick
///   1 = direct special-link leg accepted; sim[+8] = segment index,
///       sim[+7] = post-link floor (the leg's destination)
///   2 = queued onto a carrier; sim[+8] = 0x40 + id (up) or 0x58 + id (down),
///       sim[+7] = source floor
///   3 = same-floor success (treated as immediate arrival by the caller)
///
/// Binary quirk: same-floor returns 3 (not 2) — distinct from the enqueued
/// case so the caller can short-circuit the arrival path.
///
/// (Upstream's type is the literal union `-1 | 0 | 1 | 2 | 3`; a plain i64
/// here, compared with the numbers as upstream does.)
pub type RouteResolution = i64;

// Optional flags for `resolveSimRouteBetweenFloors`. These mirror the binary's
// two short arguments to `resolve_sim_route_between_floors` (1218:0000):
//
//   Stack[0x4]:2 → `is_passenger_route` (the variable Ghidra previously named
//     `prefer_local_mode` for the helper). In the binary it gates:
//       - `advance_sim_trip_counters` on same-floor (rc=3) and route-failure
//         (rc=-1) branches
//       - `add_delay_to_current_sim(g_route_failure_delay = 300)` on rc=-1
//       - `add_delay_to_current_sim(g_waiting_state_delay = 5)` on rc=0
//       - `add_delay_to_current_sim(per_stop_parity_delay × step)` on rc=1
//     and is forwarded as `prefer_local_mode` to `select_best_route_candidate`
//     (11b8:1484), which biases scoring toward escalators over stairs.
//
//     At every binary call site this is `1` for passenger families (office,
//     hotel, condo, commercial, cathedral, entertainment) and `0` for service
//     families (housekeeping at 1228:620f / 1228:6320).
//
//   Stack[0x6]:2 → `emit_distance_feedback`. Gates the long-trip distance
//     penalty (`add_delay_to_current_sim` 30-tick / 60-tick branches) on the
//     segment success and carrier success branches.
//
//     At every passenger-family binary call site this is computed as
//     `(current_state_code == base_state_code) ? 1 : 0` — i.e. fire the
//     distance penalty exactly once per trip, on the BASE state handler, and
//     suppress it on the +0x40 transit alias re-entries. Housekeeping passes
//     `0` at both call sites (1228:620f / 1228:6320).
//
// Defaults: `isPassengerRoute = true` and `emitDistanceFeedback = true` match
// the most common base-state passenger-family call site.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolveSimRouteOptions {
    pub target_height_metric: Option<i64>,
    pub is_passenger_route: Option<bool>,
    pub emit_distance_feedback: Option<bool>,
}

/// (`sim` is `world.sims[sim_index]`.  `time` is upstream's
/// `TimeState | undefined`; `options` its defaulted parameter, `None` for
/// `{}`.)
pub fn resolve_sim_route_between_floors(
    world: &mut WorldState,
    sim_index: i64,
    mut source_floor: i64,
    mut destination_floor: i64,
    direction_flag: i64,
    time: Option<&TimeState>,
    options: Option<ResolveSimRouteOptions>,
) -> RouteResolution {
    let si = sim_index as usize;
    let options = options.unwrap_or_default();
    let target_height_metric = options.target_height_metric.unwrap_or(world.sims[si].home_column);
    let is_passenger_route = options.is_passenger_route.unwrap_or(true);
    let emit_distance_feedback = options.emit_distance_feedback.unwrap_or(true);

    // Binary 1218:001e/0029: clamp negative source/target to LOBBY_FLOOR (10).
    // Sentinel -1 reaches here from `get_entertainment_link_routing_source_floor`
    // (binary stores 0xff in the half it has not registered yet) and from
    // `get_current_commercial_venue_destination_floor` (lobby fallback).
    if source_floor < 0 {
        source_floor = LOBBY_FLOOR;
    }
    if destination_floor < 0 {
        destination_floor = LOBBY_FLOOR;
    }

    if source_floor == destination_floor {
        // Binary quirk: same-floor result code is 3 (not 2). The caller treats
        // this as an immediate arrival and does not enqueue onto a carrier.
        // Binary 1218:0046: same-floor branch is one of the 6 advance call sites,
        // gated on `is_passenger_route != 0`.
        if is_passenger_route {
            advance_sim_trip_counters(&mut world.sims[si]);
        }
        return 3;
    }

    // `prefer_local_mode` is now baked into the per-family selector wrappers
    // (`selectHousekeepingRoute` passes false, `selectCathedralRoute` and the
    // shared passenger families pass true), matching the binary's call-site
    // pattern in 1228:602b vs 1228:5ddd/5e7e vs all other family handlers.
    //
    // Note: the binary forwards `is_passenger_route` to `select_best_route_candidate`
    // as `prefer_local_mode`. Here we already differentiate per-family in
    // `selectRouteForFamily`, so the value of `isPassengerRoute` does not feed
    // route selection — it only gates the post-resolve delay/trip-counter writes
    // below.
    let route = select_route_for_family(
        world,
        world.sims[si].family_code,
        source_floor,
        destination_floor,
        target_height_metric,
    );
    let Some(route) = route else {
        let sim = &mut world.sims[si];
        clear_sim_route(sim);
        // Binary 1218:00ec-ish (no-route path): both the failure-delay (300) and
        // trip-counter advance are gated on `is_passenger_route`.
        if is_passenger_route {
            add_delay_to_current_sim(sim, 300);
            advance_sim_trip_counters(sim);
        }
        return -1;
    };

    if route.kind == RouteCandidateKind::Segment {
        // Binary 1218:0140-019d: resolve writes ONE leg per call. sim+7 jumps to
        // `source ± ((segment.mode_and_span >> 1) + 1)` (the leg endpoint), and
        // sim+8 = leg index. The per-tick stride dispatcher re-invokes the state
        // handler every 16 ticks per sim, which re-calls resolve to advance the
        // next leg. When source == destination on a re-call, the same-floor branch
        // (1218:0046) advances trip counters and returns 3, signaling arrival.
        //
        // With per-tile placement (extent_minus_one = 0 for a 2-floor stair tile),
        // (flags >> 1) + 1 == 1, so each leg advances exactly one floor.
        if emit_distance_feedback {
            let segment_height_metric = if route.id >= 0 {
                world.special_links.get(route.id as usize).map(|segment| segment.height_metric)
            } else {
                None
            };
            maybe_apply_distance_feedback(
                world,
                sim_index,
                segment_height_metric.unwrap_or(target_height_metric),
                target_height_metric,
                true,
            );
        }
        let direction = if destination_floor > source_floor { 1 } else { -1 };
        let next_floor = source_floor + direction;
        let parity_bit = if route.id >= 0 {
            world.special_links.get(route.id as usize).map(|segment| js::to_int32(segment.flags) & 1)
        } else {
            None
        }
        .unwrap_or(0);
        let sim = &mut world.sims[si];
        sim.route = RouteState::Segment {
            segment_id: route.id,
            destination: destination_floor,
        };
        // Phase 5b: `sim.stateCode` bits are authoritative for routing mode
        // in the hotel / office / condo / restaurant / fast-food / retail
        // families (the `dispatch_sim_behavior` families per cs:1c71).
        if family_uses_state_bits(sim.family_code) {
            set_sim_in_transit(sim, true);
        }
        sim.queue_tick = time.map(|t| t.day_tick).unwrap_or(sim.queue_tick);
        sim.selected_floor = next_floor;
        sim.destination_floor = destination_floor;
        // Per-leg stress penalty (binary: add_delay_to_current_sim with per-stop
        // parity delay × step). Step is 1 here (one floor per leg), matching the
        // binary's per-tile segment processing. Gated on `is_passenger_route` —
        // service routes (housekeeping) skip this entirely in the binary.
        if is_passenger_route {
            // (`segment ? segment.flags & 1 : 0`, read above.)
            add_delay_to_current_sim(sim, perStopParityDelay[parity_bit as usize]);
        }
        // transitTicksRemaining is no longer used to gate segment progression —
        // the per-stride state handler re-resolves the next leg.
        sim.transit_ticks_remaining = 0;
        if let Some(time) = time {
            sim.last_demand_tick = time.day_tick;
        }
        return 1;
    }

    let carrier = world.carriers.iter().position(|candidate| candidate.carrier_id == route.id);
    let Some(carrier) = carrier else {
        let sim = &mut world.sims[si];
        clear_sim_route(sim);
        if is_passenger_route {
            add_delay_to_current_sim(sim, 300);
            advance_sim_trip_counters(sim);
        }
        return -1;
    };

    let sim_id = sim_key(&world.sims[si]);
    let lobby_mode = world.lobby_mode;
    let queued = enqueue_request_into_route_queue(
        &mut world.carriers[carrier],
        &sim_id,
        source_floor,
        destination_floor,
        direction_flag,
        lobby_mode,
    );
    if !queued {
        // Binary 1218:021f-ish (queue-full path): writes sim+8 = 0xff and
        // sim+7 = source floor, sets waiting bit. 5-tick penalty matches
        // g_waiting_state_delay and is gated on `is_passenger_route` in the
        // binary; the family handler retries next stride.
        let sim = &mut world.sims[si];
        clear_sim_route(sim);
        if family_uses_state_bits(sim.family_code) {
            set_sim_waiting(sim, true);
        }
        sim.selected_floor = source_floor;
        sim.destination_floor = destination_floor;
        if is_passenger_route {
            add_delay_to_current_sim(sim, 5);
        }
        return 0;
    }

    let sim = &mut world.sims[si];
    sim.route = RouteState::Carrier {
        carrier_id: route.id,
        direction: if direction_flag == 1 { RouteDirection::Up } else { RouteDirection::Down },
        source: source_floor,
    };
    // Phase 5b: idle → carrier (enqueued, boarding pending) → set 0x40 for
    // dispatch_sim_behavior families. See `familyUsesStateBits` below.
    if family_uses_state_bits(sim.family_code) {
        set_sim_in_transit(sim, true);
    }
    sim.queue_tick = time.map(|t| t.day_tick).unwrap_or(sim.queue_tick);
    // Binary: carrier branch writes sim+7 = source_floor (sim parks at source
    // while waiting for the carrier; the carrier-arrival path will later set
    // sim+7 = destination_floor when the car deposits the sim).
    sim.selected_floor = source_floor;
    sim.destination_floor = destination_floor;
    if emit_distance_feedback {
        let (column, carrier_mode) = (world.carriers[carrier].column, world.carriers[carrier].carrier_mode);
        maybe_apply_distance_feedback(world, sim_index, column, target_height_metric, carrier_mode != 0);
    }
    if let Some(time) = time {
        world.sims[si].last_demand_tick = time.day_tick;
    }
    2
}


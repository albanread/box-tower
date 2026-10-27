// ts: route-scoring/select-candidate.ts

// 11b8:1484 selectBestRouteCandidate
//
// Family-agnostic route selector. Mirrors the binary's three-stage scan:
// (1) a 64-segment local/express scan gated by direct walkability, (2) when
// passenger-mode AND no direct segment was found, an 8-record special-link
// pass that can trigger a *second* 64-segment local scan against
// `sourceFloor ± 1`, (3) a 24-carrier scan whose threshold is whatever
// `local_e` was preserved from earlier stages (0x7fff if no segment, else
// the segment cost).

use crate::reachability::span_checks::{
    is_floor_span_walkable_for_housekeeping_route, is_floor_span_walkable_for_local_route,
};
use crate::world::{WorldState, MAX_SPECIAL_LINK_RECORDS};

use super::constants::{ROUTE_COST_INFINITE, STAIRS_ROUTE_EXTRA_COST};
use super::score_carrier::{score_carrier_direct_route, score_carrier_transfer_route};
use super::score_local::{score_housekeeping_route_segment, score_local_route_segment};
use super::score_special_link::score_special_link_route;

/// `RouteCandidate.kind` (upstream's inline `"segment" | "carrier"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteCandidateKind {
    Segment,
    Carrier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteCandidate {
    pub kind: RouteCandidateKind,
    pub id: i64,
    pub cost: i64,
}

/// `prefer_local_mode` defaults to true and `target_height_metric` to 0, as
/// upstream's optional parameters do.  `null` is `None`.
pub fn select_best_route_candidate(
    world: &WorldState,
    from_floor: i64,
    to_floor: i64,
    prefer_local_mode: Option<bool>,
    target_height_metric: Option<i64>,
) -> Option<RouteCandidate> {
    let prefer_local_mode = prefer_local_mode.unwrap_or(true);
    let target_height_metric = target_height_metric.unwrap_or(0);
    if from_floor == to_floor {
        return None;
    }

    let delta = (from_floor - to_floor).abs();
    // Binary `local_e` and `local_idx`. `local_e` doubles as the running
    // minimum cost AND the carrier-scan threshold.
    let mut best_cost = ROUTE_COST_INFINITE;
    let mut best_index: i64 = -1;

    if !prefer_local_mode {
        // Housekeeping path. Single 64-segment scan gated by continuous-stairs
        // walkability; on success it returns immediately, otherwise falls
        // straight through to the carrier scan.
        if delta == 1 || is_floor_span_walkable_for_housekeeping_route(world, from_floor, to_floor) {
            for (segment_index, segment) in world.special_links.iter().enumerate() {
                let cost = score_housekeeping_route_segment(segment, from_floor, to_floor, target_height_metric);
                if cost < best_cost {
                    best_index = segment_index as i64;
                    best_cost = cost;
                }
            }
            if best_index >= 0 {
                return Some(RouteCandidate { kind: RouteCandidateKind::Segment, id: best_index, cost: best_cost });
            }
        }
    } else {
        // Passenger path. First local scan gated by local walkability.
        if delta == 1 || is_floor_span_walkable_for_local_route(world, from_floor, to_floor) {
            for (segment_index, segment) in world.special_links.iter().enumerate() {
                let cost = score_local_route_segment(segment, from_floor, to_floor, target_height_metric);
                if cost < best_cost {
                    best_index = segment_index as i64;
                    best_cost = cost;
                }
            }
            // 11b8:1510-1515 early return when the cheapest segment is below
            // the stairs cost-extra threshold (escalator hit).
            if best_index >= 0 && best_cost < STAIRS_ROUTE_EXTRA_COST {
                return Some(RouteCandidate { kind: RouteCandidateKind::Segment, id: best_index, cost: best_cost });
            }
        }

        // 11b8:151d Special-link block. Runs only if the first scan found
        // nothing (`local_idx < 0`). If a winner exists with cost >= 640, the
        // binary skips this block entirely and proceeds to the carrier scan
        // with `local_e` preserved as the threshold.
        if best_index < 0 {
            let mut direction = 0;
            let mut found_record = false;
            for record_index in 0..MAX_SPECIAL_LINK_RECORDS {
                let result = score_special_link_route(world, record_index, from_floor, to_floor);
                if result.cost < best_cost {
                    best_index = record_index;
                    direction = result.direction;
                    best_cost = result.cost;
                    found_record = true;
                }
            }

            if found_record {
                // Second 64-segment local scan against sourceFloor ± 1.
                // The binary resets `local_e = 0x7fff; local_idx = -1` here.
                let adjacent_floor = if direction == 0 { from_floor - 1 } else { from_floor + 1 };
                best_cost = ROUTE_COST_INFINITE;
                best_index = -1;
                for (segment_index, segment) in world.special_links.iter().enumerate() {
                    let cost = score_local_route_segment(segment, from_floor, adjacent_floor, target_height_metric);
                    if cost < best_cost {
                        best_index = segment_index as i64;
                        best_cost = cost;
                    }
                }
                // Post-scan early return: winner found with cost < 640.
                if best_index >= 0 && best_cost < STAIRS_ROUTE_EXTRA_COST {
                    return Some(RouteCandidate { kind: RouteCandidateKind::Segment, id: best_index, cost: best_cost });
                }
            }
        }
    }

    // 11b8:1632 Carrier scan. Threshold is the preserved `local_e`. Update
    // `bestIndex`/`bestCost` only when a carrier strictly beats the current
    // minimum. Carrier IDs are encoded as `0x40 + carrierIndex` to disambiguate
    // from segment indices in the binary's single return value.
    for carrier_index in 0..24 {
        let Some(carrier) = world.carriers.iter().find(|candidate| candidate.carrier_id == carrier_index) else {
            continue;
        };
        if if prefer_local_mode { carrier.carrier_mode == 2 } else { carrier.carrier_mode != 2 } {
            continue;
        }
        // Binary calls `score_carrier_transfer_route` once per carrier; TS
        // splits direct vs transfer — use the cheaper of the two.
        let direct_cost =
            score_carrier_direct_route(world, carrier.carrier_id, from_floor, to_floor, target_height_metric);
        let transfer_cost = score_carrier_transfer_route(
            world,
            carrier.carrier_id,
            from_floor,
            to_floor,
            prefer_local_mode,
            target_height_metric,
        );
        let mut carrier_cost = direct_cost.min(transfer_cost);
        // BOX: short hops -- under BOX RULES a lift's queue at this floor
        // costs (each person already waiting), so a crowded lift loses a
        // short hop to the stairs and the emptier of two lifts wins (upstream
        // counts the queue only when it is full).
        if carrier_cost < ROUTE_COST_INFINITE && carrier.carrier_mode != 0 && crate::box_ext::box_rules(world) {
            let q = crate::route_scoring::score_carrier::queue_count(carrier, from_floor, if to_floor > from_floor { 1 } else { 0 });
            carrier_cost += q * crate::box_ext::QUEUE_COST_PER_PERSON;
        }
        if carrier_cost < best_cost {
            best_index = 0x40 + carrier_index;
            best_cost = carrier_cost;
        }
    }

    if best_index < 0 {
        return None;
    }
    if best_index >= 0x40 {
        return Some(RouteCandidate { kind: RouteCandidateKind::Carrier, id: best_index - 0x40, cost: best_cost });
    }
    Some(RouteCandidate { kind: RouteCandidateKind::Segment, id: best_index, cost: best_cost })
}

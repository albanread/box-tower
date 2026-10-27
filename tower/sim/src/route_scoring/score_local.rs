// ts: route-scoring/score-local.ts

// 11b8:18fb scoreLocalRouteSegment
// 11b8:19a8 scoreHousekeepingRouteSegment
//
// Cost for a direct stairs/escalator segment, plus the stairs-only
// housekeeping variant (called from the !is_passenger_route branch in
// select_best_route_candidate).

use crate::js;
use crate::world::SpecialLinkSegment;

use super::constants::{ROUTE_COST_INFINITE, STAIRS_ROUTE_EXTRA_COST};

pub fn score_local_route_segment(
    segment: &SpecialLinkSegment,
    from_floor: i64,
    to_floor: i64,
    target_height_metric: i64,
) -> i64 {
    if !segment.active {
        return ROUTE_COST_INFINITE;
    }
    // Binary 11b8:18fb only validates the source landing — the destination
    // (toFloor) is not range-checked against the segment span.
    if !segment_covers_floor(segment, from_floor) {
        return ROUTE_COST_INFINITE;
    }
    if !can_enter_segment_from_floor(segment, from_floor, to_floor) {
        return ROUTE_COST_INFINITE;
    }
    let is_stairs = (segment.flags & 1) != 0;
    let distance = (segment.height_metric - target_height_metric).abs() * 8;
    if is_stairs {
        distance + STAIRS_ROUTE_EXTRA_COST
    } else {
        distance
    }
}

pub fn score_housekeeping_route_segment(
    segment: &SpecialLinkSegment,
    from_floor: i64,
    to_floor: i64,
    target_height_metric: i64,
) -> i64 {
    if !segment.active {
        return ROUTE_COST_INFINITE;
    }
    if (segment.flags & 1) == 0 {
        return ROUTE_COST_INFINITE;
    }
    // Binary only validates the source landing — destination is not checked.
    if !segment_covers_floor(segment, from_floor) {
        return ROUTE_COST_INFINITE;
    }
    if !can_enter_segment_from_floor(segment, from_floor, to_floor) {
        return ROUTE_COST_INFINITE;
    }
    (segment.height_metric - target_height_metric).abs() * 8 + STAIRS_ROUTE_EXTRA_COST
}

fn get_segment_extent_minus_one(segment: &SpecialLinkSegment) -> i64 {
    js::sar(segment.flags, 1)
}

fn get_segment_top_floor(segment: &SpecialLinkSegment) -> i64 {
    // Binary encoding: top_floor = entry_floor + (flags >> 1) + 1.
    segment.entry_floor + get_segment_extent_minus_one(segment) + 1
}

fn segment_covers_floor(segment: &SpecialLinkSegment, floor: i64) -> bool {
    floor >= segment.entry_floor && floor <= get_segment_top_floor(segment)
}

fn can_enter_segment_from_floor(segment: &SpecialLinkSegment, from_floor: i64, to_floor: i64) -> bool {
    // Binary score_local_route_segment: terminal-landing entry gate, applied
    // uniformly to stairs and escalators. Going up: source must equal entry
    // floor. Going down: source must equal top floor (entry + (flags>>1) + 1).
    if to_floor > from_floor {
        return from_floor == segment.entry_floor;
    }
    from_floor == get_segment_top_floor(segment)
}

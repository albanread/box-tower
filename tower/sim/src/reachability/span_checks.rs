// ts: reachability/span-checks.ts

// 11b8:12d2 isFloorSpanWalkableForLocalRoute
// 11b8:1392 isFloorSpanWalkableForHousekeepingRoute
// 11b8:0ccf isFloorWithinSpecialLinkSpan
//
// Geometric walkability / membership checks used by the route scorer and
// the selector in `select-candidate.ts`.
//
// Bit semantics of `floorWalkabilityFlags[floor]`:
//   bit 0x1 = escalator present on floor
//   bit 0x2 = stairs present on floor
// The local-route gate is escalator-tolerant (bit 0x1, ≤2-floor stair gap).
// The housekeeping-route gate demands continuous stairs (bit 0x2, no gap),
// and is invoked only from the !is_passenger_route branch in
// select_best_route_candidate (object family 0x0f, janitors).

use crate::js;
use crate::world::{SpecialLinkSegment, WorldState};

pub fn is_floor_span_walkable_for_local_route(world: &WorldState, from_floor: i64, to_floor: i64) -> bool {
    let lower = from_floor.min(to_floor);
    let upper = from_floor.max(to_floor);
    if upper - lower >= 7 {
        return false;
    }
    let mut seen_gap = false;
    // Binary 11b8:12d2 iterates [lower, upper) — the target floor is not
    // flag-tested here.
    let mut floor = lower;
    while floor < upper {
        let flags = js::at_or(&world.floor_walkability_flags, floor, 0);
        if flags == 0 {
            return false;
        }
        if (flags & 1) == 0 {
            seen_gap = true;
        }
        if seen_gap && floor - lower > 2 {
            return false;
        }
        floor += 1;
    }
    true
}

pub fn is_floor_span_walkable_for_housekeeping_route(world: &WorldState, from_floor: i64, to_floor: i64) -> bool {
    if (to_floor - from_floor).abs() >= 7 {
        return false;
    }
    // Binary quirk: the counter increments once per floor and fails when >= 3,
    // so only up to 3 intermediate floors (exclusive of the target) are checked.
    // Floors beyond the 3rd are not validated for bit 2 (stairs).
    //
    // (`undefined & 2` is 0: a floor off the table fails.)
    let mut count = 0;
    let (start, end) = if from_floor < to_floor { (from_floor, to_floor) } else { (to_floor, from_floor) };
    let mut floor = start;
    while floor < end {
        if (js::at_or(&world.floor_walkability_flags, floor, 0) & 2) == 0 {
            return false;
        }
        if count >= 3 {
            return false;
        }
        count += 1;
        floor += 1;
    }
    true
}

pub fn is_floor_within_special_link_span(segment: &SpecialLinkSegment, floor: i64) -> bool {
    let extent_minus_one = js::sar(segment.flags, 1);
    // Binary encoding: top_floor = entry_floor + (flags >> 1) + 1.
    let top_floor = segment.entry_floor + extent_minus_one + 1;
    floor >= segment.entry_floor && floor <= top_floor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_flags(flags: &[(i64, i64)]) -> WorldState {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        for &(f, v) in flags {
            w.floor_walkability_flags[f as usize] = v;
        }
        w
    }

    #[test]
    fn local_span_tolerates_a_short_stair_gap() {
        // Escalator 10..12, stairs 13..15.
        let w = world_with_flags(&[(10, 1), (11, 1), (12, 1), (13, 2), (14, 2), (15, 2)]);
        assert!(is_floor_span_walkable_for_local_route(&w, 10, 13));
        assert!(is_floor_span_walkable_for_local_route(&w, 13, 10));
        // Gap seen at 13, then floor 13 - 10 > 2: fails.
        assert!(!is_floor_span_walkable_for_local_route(&w, 10, 14));
        assert!(!is_floor_span_walkable_for_local_route(&w, 9, 11));
        assert!(!is_floor_span_walkable_for_local_route(&w, 10, 17));
    }

    #[test]
    fn housekeeping_span_checks_at_most_three_floors() {
        let w = world_with_flags(&[(10, 2), (11, 2), (12, 2), (13, 2)]);
        assert!(is_floor_span_walkable_for_housekeeping_route(&w, 10, 13));
        assert!(is_floor_span_walkable_for_housekeeping_route(&w, 13, 10));
        // A fourth floor fails on the counter even though it has stairs.
        assert!(!is_floor_span_walkable_for_housekeeping_route(&w, 10, 14));
        assert!(!is_floor_span_walkable_for_housekeeping_route(&w, 9, 10));
    }
}

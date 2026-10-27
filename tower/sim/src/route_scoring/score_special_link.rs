// ts: route-scoring/score-special-link.ts

// 11b8:0be2 scoreSpecialLinkRoute
//
// Pass/fail viability check for one `specialLinkRecords[recordIndex]`. Returns
// `{ cost: 0, direction }` when the record can shuttle from `fromFloor` to
// `toFloor`, otherwise `{ cost: ROUTE_COST_INFINITE, direction: 0 }`.
//
// Binary structure:
//   1. Reject inactive records or records whose span does not cover src.
//   2. If span covers dst too, succeed with `direction = src < dst ? 1 : 0`.
//   3. Otherwise consult the per-floor reachability table. The src entry of
//      that table is a 1-based index into `transferGroupEntries` for floors
//      inside the span, or a peer-bitmask for floors outside. The dst entry
//      is a peer-bitmask (the floor lives outside the span here).
//      A direct match requires `mask[dst] != 0`, `mask[src] != 0`, and the
//      carriers reachable via `transferGroupEntries[mask[src]-1]` to be a
//      superset of `mask[dst]`. Failing that, fall back to
//      `testSpecialLinkTransferReachability`.

use crate::js;
use crate::reachability::mask_tests::test_special_link_transfer_reachability;
use crate::world::WorldState;

use super::constants::ROUTE_COST_INFINITE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialLinkRouteResult {
    pub cost: i64,
    pub direction: i64,
}

const FAIL: SpecialLinkRouteResult = SpecialLinkRouteResult {
    cost: ROUTE_COST_INFINITE,
    direction: 0,
};

pub fn score_special_link_route(
    world: &WorldState,
    record_index: i64,
    from_floor: i64,
    to_floor: i64,
) -> SpecialLinkRouteResult {
    let record = if record_index >= 0 { world.special_link_records.get(record_index as usize) } else { None };
    let Some(record) = record.filter(|record| record.active) else {
        return FAIL;
    };

    let src_in_span = from_floor >= record.lower_floor && from_floor <= record.upper_floor;
    if !src_in_span {
        return FAIL;
    }

    let dst_in_span = to_floor >= record.lower_floor && to_floor <= record.upper_floor;
    if dst_in_span {
        return SpecialLinkRouteResult { cost: 0, direction: if from_floor < to_floor { 1 } else { 0 } };
    }

    let dst_mask = js::at_or(&record.reachability_masks_by_floor, to_floor, 0);
    if dst_mask != 0 {
        let src_mask = js::at_or(&record.reachability_masks_by_floor, from_floor, 0);
        if src_mask != 0 {
            // Inside the span the table stores a 1-based index into
            // transferGroupEntries; use that entry's carrierMask as the
            // reachable-peer set and check whether dstMask is a subset.
            //
            // (`srcMask - 1` past the table, or negative for a mask with bit
            // 31 set, reads undefined: no entry.)
            let src_entry = if src_mask - 1 >= 0 {
                world.transfer_group_entries.get((src_mask - 1) as usize)
            } else {
                None
            };
            if let Some(src_entry) = src_entry {
                if src_entry.active && (src_entry.carrier_mask & dst_mask) == dst_mask {
                    return SpecialLinkRouteResult { cost: 0, direction: 0 };
                }
            }
        }
    }

    // Final fallback: peer-record reachability scan via the transfer-group
    // cache. testSpecialLinkTransferReachability operates on a transferGroupEntry,
    // not a record — match the binary by trying every entry that this record
    // participates in.
    let record_bit = js::shl(1, 24 + record_index);
    for (entry_index, entry) in world.transfer_group_entries.iter().enumerate() {
        if !entry.active {
            continue;
        }
        if (entry.carrier_mask & record_bit) == 0 {
            continue;
        }
        if test_special_link_transfer_reachability(world, entry_index as i64, to_floor) {
            return SpecialLinkRouteResult { cost: 0, direction: 0 };
        }
    }
    FAIL
}

// ts: route-scoring/score-carrier.ts

// 11b8:168e scoreCarrierTransferRoute
//
// Cost functions for carrier (elevator) routes. The binary map lists a
// single `scoreCarrierTransferRoute` entry, but the current TS scorer
// splits into direct vs. transfer variants — both kept here and exported
// under the binary name suffixes so the selector can call them independently.
//
// TODO(11b8:168e): confirm whether the binary folds the direct and
// transfer paths into one function. If so, merge these two; otherwise
// keep them split and treat both as helpers of the binary entry.

use crate::carriers::carrier_spans_floor;
use crate::carriers::slot::is_express_stop_floor;
use crate::js;
use crate::reachability::mask_tests::test_carrier_transfer_reachability;
use crate::world::{CarrierRecord, LobbyMode, WorldState};

use super::constants::{
    DIRECT_ROUTE_BASE_COST, DIRECT_ROUTE_FULL_QUEUE_COST, QUEUE_FULL_COUNT, ROUTE_COST_INFINITE,
    TRANSFER_ROUTE_BASE_COST, TRANSFER_ROUTE_FULL_QUEUE_COST,
};

/// Binary `score_carrier_transfer_route` (11b8:168e) gates eligibility on
/// `served_floor_flags[floor] != 0`. In the binary that bit is the union of
/// "this floor is in the carrier's served set" (set when the shaft is built/
/// extended) and "the player has not toggled this floor off" (cleared by the
/// per-floor stop-dialog button at FUN_10a8_0085). TS splits these into
/// `carrierSpansFloor` (geometric [bottom, top] coverage) and
/// `stopFloorEnabled[slot]` (player toggle), so the gate must AND both.
///
/// Express carriers additionally restrict to lobby/sky-lobby slots even within
/// their span, so chain `isExpressStopFloor` for mode-0.
fn carrier_eligible_floor(carrier: &CarrierRecord, floor: i64, lobby_mode: LobbyMode) -> bool {
    if !carrier_spans_floor(carrier, floor) {
        return false;
    }
    let slot = floor - carrier.bottom_served_floor;
    if js::at_or(&carrier.stop_floor_enabled, slot, 1) == 0 {
        return false;
    }
    if carrier.carrier_mode == 0 {
        // Express: only stops at internal/binary floors 1..10 (basement/ground)
        // and the lobby cadence above. The `floor` parameter is in internal
        // numbering — same convention `isExpressStopFloor` expects.
        return is_express_stop_floor(floor, lobby_mode);
    }
    true
}

fn get_floor_queue_count(carrier: &CarrierRecord, floor: i64, direction_flag: i64) -> i64 {
    let slot = floor - carrier.bottom_served_floor;
    if slot < 0 || slot >= carrier.floor_queues.len() as i64 {
        return 0;
    }
    let queue = &carrier.floor_queues[slot as usize];
    let direction_queue = if direction_flag == 1 { &queue.up } else { &queue.down };
    direction_queue.size().min(QUEUE_FULL_COUNT)
}

/// Scores a carrier route where the carrier directly serves both endpoints.
/// Mirrors the "carrier directly serves floor" branch of the binary's
/// `scoreCarrierTransferRoute` (11b8:168e).
/// BOX: the people waiting for `carrier` at `floor` going `direction`.
pub fn queue_count(carrier: &crate::world::CarrierRecord, floor: i64, direction: i64) -> i64 {
    get_floor_queue_count(carrier, floor, direction)
}

pub fn score_carrier_direct_route(
    world: &WorldState,
    carrier_id: i64,
    from_floor: i64,
    to_floor: i64,
    target_height_metric: i64,
) -> i64 {
    let Some(carrier) = world.carriers.iter().find(|candidate| candidate.carrier_id == carrier_id) else {
        return ROUTE_COST_INFINITE;
    };
    if !carrier_eligible_floor(carrier, from_floor, world.lobby_mode) {
        return ROUTE_COST_INFINITE;
    }
    if !carrier_eligible_floor(carrier, to_floor, world.lobby_mode) {
        return ROUTE_COST_INFINITE;
    }
    let q_count = get_floor_queue_count(carrier, from_floor, if to_floor > from_floor { 1 } else { 0 });
    // 11b8:168e direct branch. mode==0 (express) ignores distance and adds the
    // integer queue count directly: cost = qCount + 640. mode!=0 uses hypot
    // distance plus a step-function penalty that replaces the base with 1000
    // only when the queue is saturated (==40).
    if carrier.carrier_mode == 0 {
        return q_count + DIRECT_ROUTE_BASE_COST;
    }
    let distance = (carrier.column - target_height_metric).abs() * 8;
    distance
        + if q_count == QUEUE_FULL_COUNT {
            DIRECT_ROUTE_FULL_QUEUE_COST
        } else {
            DIRECT_ROUTE_BASE_COST
        }
}

/// 11b8:168e scoreCarrierTransferRoute
///
/// Scores a multi-leg carrier route: the carrier covers the source floor,
/// and a transfer group exists that reaches the destination through a
/// peer carrier or derived special-link record.
pub fn score_carrier_transfer_route(
    world: &WorldState,
    carrier_id: i64,
    from_floor: i64,
    to_floor: i64,
    prefer_local_mode: bool,
    target_height_metric: i64,
) -> i64 {
    let Some(carrier) = world.carriers.iter().find(|candidate| candidate.carrier_id == carrier_id) else {
        return ROUTE_COST_INFINITE;
    };
    if !carrier_eligible_floor(carrier, from_floor, world.lobby_mode) {
        return ROUTE_COST_INFINITE;
    }
    if !test_carrier_transfer_reachability(world, carrier_id, to_floor, prefer_local_mode) {
        return ROUTE_COST_INFINITE;
    }
    let q_count = get_floor_queue_count(carrier, from_floor, if to_floor > from_floor { 1 } else { 0 });
    // 11b8:168e transfer branch. Same shape as direct branch with base=3000,
    // full-queue penalty=6000.
    if carrier.carrier_mode == 0 {
        return q_count + TRANSFER_ROUTE_BASE_COST;
    }
    let distance = (carrier.column - target_height_metric).abs() * 8;
    distance
        + if q_count == QUEUE_FULL_COUNT {
            TRANSFER_ROUTE_FULL_QUEUE_COST
        } else {
            TRANSFER_ROUTE_BASE_COST
        }
}

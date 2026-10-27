// ts: carriers/slot.ts

// 10a8:17ee floor_to_carrier_slot_index
//
// Maps (carrier, floor) → per-floor queue slot index. Returns -1 when the
// floor is outside the served range or not a lobby/express slot on an
// express carrier.

use crate::world::{CarrierRecord, LobbyMode};

pub fn floor_to_slot(carrier: &CarrierRecord, floor: i64) -> i64 {
    if floor < carrier.bottom_served_floor || floor > carrier.top_served_floor {
        return -1;
    }
    floor - carrier.bottom_served_floor
}

/// Binary 10a8:17ee express branch. Only the "express queue slot" floors —
/// basement/ground (binary floors 1..10) and the sky-lobby cadence above.
///   "perfect-parity": sky lobbies at (floor-10) % 15 == 14
///     (binary 24, 39, 54, ... = logical 14, 29, 44, ...).
///   "modern": sky lobbies at (floor-10) % 15 == 0 with floor > 10
///     (binary 25, 40, 55, ... = logical 15, 30, 45, ...).
/// Any other floor on a mode-0 carrier cannot host a call or enqueue.
/// Used by the scorer and enqueue paths to gate floor acceptance without
/// changing `floorToSlot`'s linear slot arithmetic (which the per-car
/// destination-count and route-status tables rely on).
pub fn is_express_stop_floor(floor: i64, mode: LobbyMode) -> bool {
    if floor <= 10 {
        return floor >= 1;
    }
    let cycle_offset = if mode == LobbyMode::Modern { 0 } else { 14 };
    (floor - 10) % 15 == cycle_offset
}

pub fn carrier_serves_floor(carrier: &CarrierRecord, floor: i64) -> bool {
    floor_to_slot(carrier, floor) >= 0
}

/// Geometric span check: `floor ∈ [bottomServedFloor, topServedFloor]`. NOT a
/// substitute for binary `served_floor_flags[f]`, which on express carriers is
/// gated by FUN_10a8_1296 to set the byte only at express-stop floors (binary
/// 1..10 and the sky-lobby cadence). Use this where a pure span test is wanted
/// (e.g. resetting out-of-range cars); use `carrierEligibleFloor` /
/// `isExpressStopFloor` for the served-flag-equivalent gate the binary applies
/// to route scoring and transfer-floor remapping.
pub fn carrier_spans_floor(carrier: &CarrierRecord, floor: i64) -> bool {
    floor >= carrier.bottom_served_floor && floor <= carrier.top_served_floor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn express_stops_follow_the_lobby_cadence() {
        let stops = |mode| (0..60).filter(|&f| is_express_stop_floor(f, mode)).collect::<Vec<_>>();
        assert_eq!(stops(LobbyMode::PerfectParity), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 24, 39, 54]);
        assert_eq!(stops(LobbyMode::Modern), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 25, 40, 55]);
    }
}

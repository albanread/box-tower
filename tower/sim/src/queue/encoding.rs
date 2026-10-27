// ts: queue/encoding.ts

// 1218:1b96 decode_runtime_route_target
//
// Decode the byte at `sim+0x08` (`encoded_route_target`) into a (slot,
// direction) pair:
//   byte < 0x40   -> special-link slot, slot = byte
//   0x40..0x57   -> carrier slot, direction = up,   slot = byte - 0x40
//   0x58..0x6f   -> carrier slot, direction = down, slot = byte - 0x58
//
// Phase 3 still stores route state on `sim.route` as a discriminated union;
// Phase 5 flips storage to the byte. This helper lets queue code speak the
// binary encoding today without touching sim.route.

use crate::world::{RouteDirection, RouteState, SimRecord};

pub const ROUTE_TARGET_SPECIAL_LINK_MAX: i64 = 0x40;
pub const ROUTE_TARGET_CARRIER_UP_BASE: i64 = 0x40;
pub const ROUTE_TARGET_CARRIER_DOWN_BASE: i64 = 0x58;

/// `RuntimeRouteTarget.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRouteTargetKind {
    /// "special-link"
    SpecialLink,
    /// "carrier"
    Carrier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeRouteTarget {
    /// "special-link" = stairs/escalator segment; "carrier" = elevator.
    pub kind: RuntimeRouteTargetKind,
    /// Segment index or carrier index, depending on kind.
    pub slot: i64,
    /// 1 = up, 0 = down. Always 0 for special-link targets.
    pub direction_flag: i64,
}

/// Decode a raw encoded-route-target byte. Returns null for 0xff (unrouted).
pub fn decode_encoded_route_target_byte(byte: i64) -> Option<RuntimeRouteTarget> {
    if byte == 0xff {
        return None;
    }
    if byte < ROUTE_TARGET_SPECIAL_LINK_MAX {
        return Some(RuntimeRouteTarget {
            kind: RuntimeRouteTargetKind::SpecialLink,
            slot: byte,
            direction_flag: 0,
        });
    }
    if byte < ROUTE_TARGET_CARRIER_DOWN_BASE {
        return Some(RuntimeRouteTarget {
            kind: RuntimeRouteTargetKind::Carrier,
            slot: byte - ROUTE_TARGET_CARRIER_UP_BASE,
            direction_flag: 1,
        });
    }
    Some(RuntimeRouteTarget {
        kind: RuntimeRouteTargetKind::Carrier,
        slot: byte - ROUTE_TARGET_CARRIER_DOWN_BASE,
        direction_flag: 0,
    })
}

/// Encode a (kind, slot, direction) tuple into the binary's one-byte form.
pub fn encode_runtime_route_target(target: &RuntimeRouteTarget) -> i64 {
    if target.kind == RuntimeRouteTargetKind::SpecialLink {
        return crate::js::to_int32(target.slot) & 0xff;
    }
    let base = if target.direction_flag == 1 {
        ROUTE_TARGET_CARRIER_UP_BASE
    } else {
        ROUTE_TARGET_CARRIER_DOWN_BASE
    };
    crate::js::to_int32(base + target.slot) & 0xff
}

/// Binary `decode_runtime_route_target` (1218:1b96). Reads sim's route state
/// (currently stored on `sim.route`, not yet on the encoded byte — that's
/// Phase 5) and returns the decoded (slot, direction) view, or null when
/// the sim is idle / queued / not yet assigned a concrete target.
pub fn decode_runtime_route_target(sim: &SimRecord) -> Option<RuntimeRouteTarget> {
    if let RouteState::Carrier { carrier_id, direction, .. } = &sim.route {
        return Some(RuntimeRouteTarget {
            kind: RuntimeRouteTargetKind::Carrier,
            slot: *carrier_id,
            direction_flag: if *direction == RouteDirection::Up { 1 } else { 0 },
        });
    }
    if let RouteState::Segment { segment_id, .. } = &sim.route {
        return Some(RuntimeRouteTarget {
            kind: RuntimeRouteTargetKind::SpecialLink,
            slot: *segment_id,
            direction_flag: 0,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for byte in 0..0x70 {
            let t = decode_encoded_route_target_byte(byte).unwrap();
            assert_eq!(encode_runtime_route_target(&t), byte);
        }
        assert_eq!(decode_encoded_route_target_byte(0xff), None);
        let t = decode_encoded_route_target_byte(0x5a).unwrap();
        assert_eq!((t.kind, t.slot, t.direction_flag), (RuntimeRouteTargetKind::Carrier, 2, 0));
    }
}

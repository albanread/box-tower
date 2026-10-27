// ts: reachability/mask-tests.ts

// 11b8:0f33 testCarrierTransferReachability
// 11b8:0fe6 testSpecialLinkTransferReachability
// 11b8:0e41 chooseTransferFloorFromCarrierReachability
//
// Bit-mask tests against `transferGroupEntries` + `specialLinkRecords`,
// plus the transfer-floor picker used when a carrier doesn't directly
// serve the target floor.
//
// (Masks are JavaScript int32s; shifts go through `js::shl`, so that
// `1 << 31` is negative and `1 << 32` is 1 as in JavaScript.  See
// rebuild_tables.rs.)

use crate::carriers::carrier_spans_floor;
use crate::carriers::slot::is_express_stop_floor;
use crate::js;
use crate::world::{CarrierRecord, LobbyMode, SpecialLinkRecord, WorldState, MAX_SPECIAL_LINK_RECORDS};

/// 11b8:0f33 testCarrierTransferReachability
///
/// Walks the transfer-group entries for any entry whose carrier mask
/// includes `carrierId` AND can reach `toFloor` (directly via a peer
/// carrier, or transitively via a peer special-link record).
pub fn test_carrier_transfer_reachability(
    world: &WorldState,
    carrier_id: i64,
    to_floor: i64,
    prefer_local_mode: bool,
) -> bool {
    // Binary 11b8:0f33: walks the 16-entry transfer_group_cache. Each entry's
    // membership mask is tested for the input carrier's bit, that bit is
    // cleared, and the remainder is AND'd against the carrier's per-floor
    // reachability mask for `toFloor`. No pre-loop gate on special_link_records
    // exists in the binary — peer-carrier reachability through a shared
    // lobby/sky-lobby tile is sufficient.
    for (entry_index, entry) in world.transfer_group_entries.iter().enumerate() {
        if !entry.active {
            continue;
        }
        if (entry.carrier_mask & js::shl(1, carrier_id)) == 0 {
            continue;
        }
        if entry_reaches_destination_floor(world, entry_index as i64, to_floor, prefer_local_mode) {
            return true;
        }
    }
    false
}

/// 11b8:0fe6 testSpecialLinkTransferReachability
///
/// Probe whether any peer special-link record covers `toFloor`.
///
/// Upstream takes the entry object (one of `world.transferGroupEntries`);
/// here its index (TRANSLATION.md: container + index).
pub fn test_special_link_transfer_reachability(world: &WorldState, entry_index: i64, to_floor: i64) -> bool {
    let entry = &world.transfer_group_entries[entry_index as usize];
    for record_index in 0..MAX_SPECIAL_LINK_RECORDS {
        if (entry.carrier_mask & js::shl(1, 24 + record_index)) == 0 {
            continue;
        }
        let Some(record) = world.special_link_records.get(record_index as usize) else {
            continue;
        };
        if !record.active {
            continue;
        }
        if derived_record_reaches_floor(record, to_floor) {
            return true;
        }
    }
    false
}

/// 11b8:0e41 chooseTransferFloorFromCarrierReachability
///
/// Resolve a transfer floor for a carrier route where the carrier doesn't
/// directly serve the target floor. Scans transfer-group entries to find
/// the first valid transfer floor in the travel direction.
///
/// Returns the transfer floor, or -1 if no valid transfer found.
pub fn choose_transfer_floor_from_carrier_reachability(
    world: &WorldState,
    carrier_id: i64,
    current_floor: i64,
    target_floor: i64,
) -> i64 {
    let Some(carrier) = world.carriers.iter().find(|candidate| candidate.carrier_id == carrier_id) else {
        return -1;
    };

    // Binary 11b8:0e41 opens with `carrier.served_floor_flags[target] != 0`.
    // `served_floor_flags` is written by FUN_10a8_1296 as `(carrier_mode != 0
    // || floor <= 10 || isSkyLobbyFloor(floor)) && !insideMultiFloorLobby`, so
    // on express carriers the byte is set ONLY at express-stop floors (binary
    // 1..10 and the sky-lobby cadence) — non-lobby intermediate floors fall
    // through into the transfer-cache loop. The per-floor stop-enable bit
    // cleared by the carrier-dialog toggle (FUN_10a8_0085) also clears this
    // flag, matching the "force transfer routing" branch.
    if carrier_spans_floor(carrier, target_floor) {
        let target_slot = target_floor - carrier.bottom_served_floor;
        let stop_enabled = js::at_or(&carrier.stop_floor_enabled, target_slot, 1) != 0;
        let express_ok = carrier.carrier_mode != 0 || is_express_stop_floor(target_floor, world.lobby_mode);
        if stop_enabled && express_ok {
            return target_floor;
        }
    }

    // 11b8:0e41 binary loop — scans the 16-slot transfer_group_cache directly.
    // For each entry that includes this carrier, clears the carrier's own bit
    // and requires at least one peer (carrier or derived special-link record)
    // in the remaining mask to reach targetFloor. First entry in cache order
    // whose tagged floor lies in the travel direction wins.
    let direction_up = target_floor > current_floor;
    let carrier_bit = js::shl(1, carrier_id);

    for entry in &world.transfer_group_entries {
        if !entry.active {
            continue;
        }
        if (entry.carrier_mask & carrier_bit) == 0 {
            continue;
        }
        if entry.tagged_floor == current_floor {
            continue;
        }
        if direction_up && entry.tagged_floor <= current_floor {
            continue;
        }
        if !direction_up && entry.tagged_floor >= current_floor {
            continue;
        }

        let peers_mask = entry.carrier_mask & !carrier_bit;
        if !peers_mask_reaches_floor(world, peers_mask, target_floor) {
            continue;
        }

        return entry.tagged_floor;
    }

    -1
}

fn carrier_stops_at_floor(carrier: &CarrierRecord, floor: i64, lobby_mode: LobbyMode) -> bool {
    if !carrier_spans_floor(carrier, floor) {
        return false;
    }
    // Binary served_floor_flags[floor] != 0 — the player toggle in the
    // carrier stop-floor dialog (FUN_10a8_0085) clears this bit, removing
    // the floor from the carrier's effective served set for routing.
    let slot = floor - carrier.bottom_served_floor;
    if js::at_or(&carrier.stop_floor_enabled, slot, 1) == 0 {
        return false;
    }
    if carrier.carrier_mode == 0 {
        return is_express_stop_floor(floor, lobby_mode);
    }
    true
}

fn peers_mask_reaches_floor(world: &WorldState, mask: i64, target_floor: i64) -> bool {
    for carrier_index in 0..24 {
        if (mask & js::shl(1, carrier_index)) == 0 {
            continue;
        }
        let peer = world.carriers.iter().find(|c| c.carrier_id == carrier_index);
        if let Some(peer) = peer {
            if carrier_stops_at_floor(peer, target_floor, world.lobby_mode) {
                return true;
            }
        }
    }
    for record_index in 0..MAX_SPECIAL_LINK_RECORDS {
        if (mask & js::shl(1, 24 + record_index)) == 0 {
            continue;
        }
        if let Some(record) = world.special_link_records.get(record_index as usize) {
            if record.active && derived_record_reaches_floor(record, target_floor) {
                return true;
            }
        }
    }
    false
}

// Shared helper used by both `testCarrierTransferReachability` and the
// derived-record scanners in `route-scoring/select-candidate.ts`.
pub fn derived_record_reaches_floor(record: &SpecialLinkRecord, target_floor: i64) -> bool {
    if target_floor >= record.lower_floor && target_floor <= record.upper_floor {
        return true;
    }
    js::at_or(&record.reachability_masks_by_floor, target_floor, 0) != 0
}

/// Upstream takes the entry object; here its index in
/// `world.transfer_group_entries`.
fn entry_reaches_destination_floor(world: &WorldState, entry_index: i64, to_floor: i64, prefer_local_mode: bool) -> bool {
    let entry = &world.transfer_group_entries[entry_index as usize];
    for carrier_index in 0..24 {
        if (entry.carrier_mask & js::shl(1, carrier_index)) == 0 {
            continue;
        }
        let Some(carrier) = world.carriers.iter().find(|candidate| candidate.carrier_id == carrier_index) else {
            continue;
        };
        if if prefer_local_mode { carrier.carrier_mode == 2 } else { carrier.carrier_mode != 2 } {
            continue;
        }
        if carrier_stops_at_floor(carrier, to_floor, world.lobby_mode) {
            return true;
        }
    }
    test_special_link_transfer_reachability(world, entry_index, to_floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::TransferGroupEntry;

    #[test]
    fn record_seven_uses_the_sign_bit() {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        w.special_link_records[7].active = true;
        w.special_link_records[7].lower_floor = 40;
        w.special_link_records[7].upper_floor = 45;
        // `1 << 31` in JavaScript is -2147483648.
        w.transfer_group_entries[0] = TransferGroupEntry { active: true, tagged_floor: 40, carrier_mask: -2147483648 };
        assert!(test_special_link_transfer_reachability(&w, 0, 44));
        assert!(!test_special_link_transfer_reachability(&w, 0, 46));
        // `1 << 32` is 1: a carrier id of 32 tests bit 0 (carrier 0's bit),
        // and entry 0 (record 7's floor) reaches 44 through the record.
        w.transfer_group_entries[0].carrier_mask = 1 | -2147483648;
        assert!(test_carrier_transfer_reachability(&w, 32, 44, true));
        assert!(!test_carrier_transfer_reachability(&w, 33, 44, true));
    }
}

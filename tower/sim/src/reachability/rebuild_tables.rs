// ts: reachability/rebuild-tables.ts

// 11b8:00f2 rebuildRouteReachabilityTables
// 11b8:049f rebuildTransferGroupCache
// 11b8:0000 clearRouteReachabilityTables
// 11b8:006d clearTransferGroupCache
//
// Rebuild + clear helpers for the per-floor walkability flags, the
// per-carrier transfer-group cache, and the projected reachability masks
// on each derived special-link record.
//
// The masks are JavaScript int32s: `1 << (24 + 7)` is -2^31, and `|=`, `&`
// and `~` give int32 results.  Every mask here is built from such values, and
// i64 `|`, `&` and `!` on sign-extended int32s give the sign-extended int32
// result, so plain i64 operators match once each shift goes through
// `js::shl`.

use crate::carriers::{carrier_serves_floor, carrier_spans_floor};
use crate::js;
use crate::resources::FAMILY_PARKING;
use crate::world::{
    y_to_floor, TransferGroupEntry, WorldState, GRID_HEIGHT, MAX_SPECIAL_LINK_RECORDS, MAX_TRANSFER_GROUPS,
};

pub fn clear_route_reachability_tables(world: &mut WorldState) {
    world.floor_walkability_flags = vec![0; GRID_HEIGHT as usize];
}

pub fn clear_transfer_group_cache(world: &mut WorldState) {
    world.transfer_group_cache = vec![0; GRID_HEIGHT as usize];
    world.transfer_group_entries = (0..MAX_TRANSFER_GROUPS)
        .map(|_| TransferGroupEntry {
            active: false,
            tagged_floor: 0xff,
            carrier_mask: 0,
        })
        .collect();
    for record in world.special_link_records.iter_mut() {
        record.reachability_masks_by_floor.fill(0);
    }
}

/// 11b8:00f2 rebuildRouteReachabilityTables
///
/// Populates per-floor walkability bits from the active special-link
/// segments. Bit 0 = escalator-walkable, bit 1 = stairs-walkable.
pub fn rebuild_route_reachability_tables(world: &mut WorldState) {
    clear_route_reachability_tables(world);

    for segment in &world.special_links {
        if !segment.active {
            continue;
        }
        let bit = if (segment.flags & 1) != 0 { 2 } else { 1 };
        let extent_minus_one = js::sar(segment.flags, 1);
        // Binary encoding: top_floor = entry_floor + (flags >> 1) + 1.
        let top_floor = segment.entry_floor + extent_minus_one + 1;
        let mut floor = segment.entry_floor;
        while floor <= top_floor {
            if floor >= 0 && floor < GRID_HEIGHT {
                world.floor_walkability_flags[floor as usize] |= bit;
            }
            floor += 1;
        }
    }
}

/// `Number(s)` for a coordinate of a cell key ("x,y").
fn key_number(s: &str) -> i64 {
    s.parse().unwrap_or_else(|_| panic!("cell key coordinate {s:?} is not an integer"))
}

/// 11b8:049f rebuildTransferGroupCache
///
/// Flood-fills transfer-group membership across the carrier + special-link
/// graph. Each entry pairs a tagged floor with a carrier-mask bit field
/// (bits 0..23 = carriers, bits 24..31 = derived special-link records).
pub fn rebuild_transfer_group_cache(world: &mut WorldState) {
    clear_transfer_group_cache(world);

    // Tile-level parking candidates (one per placed-object cell). Contiguous
    // spans of parking tiles on the same floor are then coalesced into a
    // single candidate so the carrier-mask picks up every carrier whose
    // column lies within the span — matching the binary's facility-struct
    // layout where a lobby is one multi-tile object, not N width-1 objects.
    struct TileCandidate {
        floor: i64,
        left: i64,
        right: i64,
        #[allow(dead_code)]
        x: i64,
    }
    let mut tile_candidates: Vec<TileCandidate> = world
        .placed_objects
        .iter()
        .filter(|(_, object)| object.object_type_code == FAMILY_PARKING)
        .map(|(key, object)| {
            let mut parts = key.split(',');
            let x = key_number(parts.next().unwrap_or(""));
            let y = key_number(parts.next().unwrap_or(""));
            TileCandidate {
                floor: y_to_floor(y),
                left: object.left_tile_index,
                right: object.right_tile_index,
                x,
            }
        })
        .collect();
    tile_candidates.sort_by(|a, b| {
        if a.floor == b.floor {
            a.left.cmp(&b.left)
        } else {
            a.floor.cmp(&b.floor)
        }
    });

    struct Span {
        floor: i64,
        left: i64,
        right: i64,
    }
    let mut spans: Vec<Span> = Vec::new();
    for tile in &tile_candidates {
        if let Some(prev) = spans.last_mut() {
            if prev.floor == tile.floor && tile.left <= prev.right + 1 {
                if tile.right > prev.right {
                    prev.right = tile.right;
                }
                continue;
            }
        }
        spans.push(Span { floor: tile.floor, left: tile.left, right: tile.right });
    }

    struct Candidate {
        floor: i64,
        #[allow(dead_code)]
        x: i64,
        membership_mask: i64,
    }
    let candidates: Vec<Candidate> = spans
        .iter()
        .map(|span| {
            let mut membership_mask: i64 = 0;
            for carrier in &world.carriers {
                if carrier.carrier_id >= 24 {
                    continue;
                }
                if carrier.column < span.left || carrier.column > span.right {
                    continue;
                }
                // Binary 11b8:049f gates carrier membership in a transfer-group
                // entry on `served_floor_flags[tagged_floor] != 0` — the carrier
                // must span the tagged floor. The prior `carrierReachesTransferFloor`
                // helper allowed distance ≤6 (≤4 for service); that over-counted
                // main-elevator membership in sky-lobby entries and produced
                // spurious multi-carrier transfer routes for sky_office.
                if !carrier_spans_floor(carrier, span.floor) {
                    continue;
                }
                // Player toggle (FUN_10a8_0085) clears served_floor_flags so a
                // disabled lobby/sky-lobby floor drops out of transfer membership.
                let slot = span.floor - carrier.bottom_served_floor;
                if js::at_or(&carrier.stop_floor_enabled, slot, 1) == 0 {
                    continue;
                }
                membership_mask |= js::shl(1, carrier.carrier_id);
            }
            Candidate { floor: span.floor, x: span.left, membership_mask }
        })
        .filter(|candidate| candidate.membership_mask != 0)
        .collect();

    // Append+collapse: append each candidate, then collapse into the
    // immediately preceding entry if it has the same tagged floor and an
    // overlapping carrier mask.
    let mut entry_count: i64 = 0;
    for candidate in &candidates {
        if entry_count > 0 {
            if let Some(prev) = world.transfer_group_entries.get_mut((entry_count - 1) as usize) {
                if prev.active
                    && prev.tagged_floor == candidate.floor
                    && (prev.carrier_mask & candidate.membership_mask) != 0
                {
                    prev.carrier_mask |= candidate.membership_mask;
                    continue;
                }
            }
        }
        // (Upstream returns from the whole function here, skipping the
        // passes below.)
        if entry_count >= MAX_TRANSFER_GROUPS {
            return;
        }
        world.transfer_group_entries[entry_count as usize] = TransferGroupEntry {
            active: true,
            tagged_floor: candidate.floor,
            carrier_mask: candidate.membership_mask,
        };
        entry_count += 1;
    }

    for record_index in 0..world.special_link_records.len() {
        let record = &world.special_link_records[record_index];
        if !record.active {
            continue;
        }
        let (lower_floor, upper_floor) = (record.lower_floor, record.upper_floor);
        for entry in world.transfer_group_entries.iter_mut() {
            if !entry.active {
                continue;
            }
            if entry.tagged_floor >= lower_floor && entry.tagged_floor <= upper_floor {
                entry.carrier_mask |= js::shl(1, 24 + record_index as i64);
            }
        }
    }

    for index in 0..world.transfer_group_entries.len() {
        let entry = &world.transfer_group_entries[index];
        if !entry.active {
            continue;
        }
        let (tagged_floor, carrier_mask) = (entry.tagged_floor, entry.carrier_mask);
        world.transfer_group_cache[tagged_floor as usize] |= carrier_mask;
    }

    for record_index in 0..world.special_link_records.len() {
        if !world.special_link_records[record_index].active {
            continue;
        }
        rebuild_special_link_record_reachability(world, record_index as i64);
    }
}

/// Upstream takes `(world, recordIndex, record)`; the record is
/// world.specialLinkRecords[recordIndex], so it is dropped.
fn rebuild_special_link_record_reachability(world: &mut WorldState, record_index: i64) {
    let ri = record_index as usize;
    let record_bit = js::shl(1, 24 + record_index);
    let mut aggregate_mask: i64 = 0;
    for entry in &world.transfer_group_entries {
        if !entry.active {
            continue;
        }
        if (entry.carrier_mask & record_bit) == 0 {
            continue;
        }
        aggregate_mask |= entry.carrier_mask;
    }
    aggregate_mask &= !record_bit;

    let mut floor: i64 = 0;
    while floor < GRID_HEIGHT {
        let record = &world.special_link_records[ri];
        let inside_span = floor >= record.lower_floor && floor <= record.upper_floor;
        let local_entry_index = world
            .transfer_group_entries
            .iter()
            .position(|entry| entry.active && entry.tagged_floor == floor && (entry.carrier_mask & record_bit) != 0)
            .map_or(-1, |i| i as i64);
        if inside_span && local_entry_index >= 0 {
            world.special_link_records[ri].reachability_masks_by_floor[floor as usize] = local_entry_index + 1;
            floor += 1;
            continue;
        }

        let mut projected_mask: i64 = 0;
        for carrier_index in 0..24 {
            if (aggregate_mask & js::shl(1, carrier_index)) == 0 {
                continue;
            }
            let Some(carrier) = world.carriers.iter().find(|candidate| candidate.carrier_id == carrier_index) else {
                continue;
            };
            if !carrier_serves_floor(carrier, floor) {
                continue;
            }
            // Binary served_floor_flags[floor] gate — drops the carrier from
            // the projected reachability mask when the floor is toggled off.
            let slot = floor - carrier.bottom_served_floor;
            if js::at_or(&carrier.stop_floor_enabled, slot, 1) == 0 {
                continue;
            }
            projected_mask |= js::shl(1, carrier_index);
        }
        for peer_index in 0..MAX_SPECIAL_LINK_RECORDS {
            if (aggregate_mask & js::shl(1, 24 + peer_index)) == 0 {
                continue;
            }
            let Some(peer) = world.special_link_records.get(peer_index as usize) else {
                continue;
            };
            if !peer.active {
                continue;
            }
            if floor >= peer.lower_floor && floor <= peer.upper_floor {
                projected_mask |= js::shl(1, 24 + peer_index);
            }
        }
        world.special_link_records[ri].reachability_masks_by_floor[floor as usize] = projected_mask;
        floor += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carriers::rebuild_carrier_list;
    use crate::reachability::special_link_records::rebuild_special_link_route_records;
    use crate::snapshot::SimSnapshot;

    fn upstream_snapshots() -> Vec<(String, SimSnapshot)> {
        let dir = std::env::var("TOWER_SNAPSHOTS").unwrap_or_else(|_| "/Volumes/xc/BOX/caches/tower/snapshots".into());
        let Ok(fixtures) = std::fs::read_dir(&dir) else {
            eprintln!("SKIPPED: no snapshot dumps in {dir}");
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut fixtures: Vec<_> = fixtures.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        fixtures.sort();
        for fixture in fixtures {
            let mut files: Vec<_> = std::fs::read_dir(&fixture)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect();
            files.sort();
            for file in files {
                let text = std::fs::read_to_string(&file).unwrap();
                let snapshot: SimSnapshot = serde_json::from_str(&text).unwrap();
                out.push((file.display().to_string(), snapshot));
            }
        }
        out
    }

    /// Upstream's snapshots hold the tables these rebuilds derive (the
    /// commands and hydration rebuild them from the same overlays, carriers
    /// and parking): rebuilding a saved world must give back the same
    /// segments, records, walkability flags, transfer groups and cache.  The
    /// segments' load counters are runtime state the rebuild zeroes, so they
    /// are left out of the comparison.
    #[test]
    fn rebuilds_reproduce_upstream_tables() {
        let mut checked = 0;
        for (label, snapshot) in upstream_snapshots() {
            let mut w = snapshot.world.clone();
            rebuild_special_link_route_records(&mut w);
            rebuild_route_reachability_tables(&mut w);
            rebuild_transfer_group_cache(&mut w);
            let strip = |w: &WorldState| {
                w.special_links
                    .iter()
                    .map(|s| (s.active, s.flags, s.height_metric, s.entry_floor, s.reserved_byte))
                    .collect::<Vec<_>>()
            };
            let s = &snapshot.world;
            assert_eq!(strip(&w), strip(s), "{label}: specialLinks");
            assert_eq!(w.special_link_records, s.special_link_records, "{label}: specialLinkRecords");
            assert_eq!(w.floor_walkability_flags, s.floor_walkability_flags, "{label}: floorWalkabilityFlags");
            assert_eq!(w.transfer_group_entries, s.transfer_group_entries, "{label}: transferGroupEntries");
            assert_eq!(w.transfer_group_cache, s.transfer_group_cache, "{label}: transferGroupCache");
            checked += 1;
        }
        eprintln!("{checked} snapshots' reachability tables rebuilt");
    }

    /// The carrier list rebuilt from a saved world's overlays matches the
    /// saved carriers' shafts (ids, columns, modes and spans).
    #[test]
    fn carrier_list_rebuild_matches_upstream_shafts() {
        for (label, snapshot) in upstream_snapshots() {
            let mut w = snapshot.world.clone();
            rebuild_carrier_list(&mut w);
            let shape = |w: &WorldState| {
                w.carriers
                    .iter()
                    .map(|c| (c.carrier_id, c.column, c.carrier_mode, c.bottom_served_floor, c.top_served_floor, c.cars.len()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(shape(&w), shape(&snapshot.world), "{label}: carriers");
        }
    }
}

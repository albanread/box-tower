// ts: reachability/special-link-records.ts

// 11b8:06a4 rebuildSpecialLinkRouteRecords
// 11b8:0763 scanSpecialLinkSpanBound
//
// Rebuilds the eight derived `specialLinkRecord` transfer zones from the
// set of stairs/escalator overlays, and scans a center floor outward to
// find the top or bottom of a walkable span.

use crate::js;
use crate::world::{
    y_to_floor, LobbyMode, SpecialLinkRecord, SpecialLinkSegment, WorldState, GRID_HEIGHT, MAX_SPECIAL_LINKS,
    MAX_SPECIAL_LINK_RECORDS,
};

// Lobby/sky-lobby centers track the express-stop cadence (see
// `isExpressStopFloor`): perfect-parity mode places them at 10, 24, 39, 54, ...
// (step 15 starting at logical offset 14); modern mode at 10, 25, 40, 55, ...
fn derived_record_centers(lobby_mode: LobbyMode) -> Vec<i64> {
    let cycle_offset = if lobby_mode == LobbyMode::Modern { 15 } else { 14 };
    vec![
        10,
        10 + cycle_offset,
        25 + cycle_offset,
        40 + cycle_offset,
        55 + cycle_offset,
        70 + cycle_offset,
        85 + cycle_offset,
    ]
}

/// `Number(s)` for a coordinate of a cell key ("x,y").
fn key_number(s: &str) -> i64 {
    s.parse().unwrap_or_else(|_| panic!("cell key coordinate {s:?} is not an integer"))
}

// Per-tile placement matches binary 1200:149c. flags = ((extent_minus_one) << 1)
// | stairsBit; for 2-floor tiles extent_minus_one = 0. entryFloor is the lower
// landing (overlay coordinate is the upper landing in this codebase).
pub fn rebuild_special_link_route_records(world: &mut WorldState) {
    world.special_links = (0..MAX_SPECIAL_LINKS)
        .map(|_| SpecialLinkSegment {
            active: false,
            flags: 0,
            height_metric: 0,
            entry_floor: 0,
            reserved_byte: 0,
            descending_load_counter: 0,
            ascending_load_counter: 0,
        })
        .collect();
    world.special_link_records = (0..MAX_SPECIAL_LINK_RECORDS)
        .map(|_| SpecialLinkRecord {
            active: false,
            lower_floor: 0,
            upper_floor: 0,
            reachability_masks_by_floor: vec![0; GRID_HEIGHT as usize],
        })
        .collect();

    // Upstream's `type: "stairs" | "escalator"`; here whether it is stairs.
    struct TileEntry {
        column: i64,
        entry_floor: i64,
        is_stairs: bool,
    }
    let mut tile_entries: Vec<TileEntry> = Vec::new();

    for (key, r#type) in world.overlays.iter() {
        if r#type != "stairs" && r#type != "escalator" {
            continue;
        }
        let mut parts = key.split(',');
        let x_str = parts.next().unwrap_or("");
        let y_str = parts.next().unwrap_or("");
        let column = key_number(x_str);
        let upper_floor = y_to_floor(key_number(y_str));
        // Overlay coordinate is the upper landing; the binary stores entry_floor
        // as the bottom landing — for a tile at floor F that's F - 1.
        tile_entries.push(TileEntry { column, entry_floor: upper_floor - 1, is_stairs: r#type == "stairs" });
    }

    tile_entries.sort_by(|a, b| {
        if a.column == b.column {
            a.entry_floor.cmp(&b.entry_floor)
        } else {
            a.column.cmp(&b.column)
        }
    });

    let mut segment_index = 0;
    for tile in &tile_entries {
        if segment_index >= MAX_SPECIAL_LINKS {
            break;
        }
        let stairs_bit = if tile.is_stairs { 1 } else { 0 };
        // extent_minus_one = 0 for a 2-floor tile, so flags == stairsBit.
        world.special_links[segment_index as usize] = SpecialLinkSegment {
            active: true,
            flags: stairs_bit,
            height_metric: tile.column,
            entry_floor: tile.entry_floor,
            reserved_byte: 0,
            descending_load_counter: 0,
            ascending_load_counter: 0,
        };
        segment_index += 1;
    }

    // Refresh floorWalkabilityFlags from the just-rebuilt specialLinks so the
    // span scan below sees up-to-date walkability. Without this, the scan reads
    // flags left over from a prior segment set: the server's incremental rebuild
    // path produces records derived from N-1 flags, while a fresh hydrate
    // derives them from the snapshot's current flags — same overlays, different
    // records, breaking the lockstep round-trip.
    world.floor_walkability_flags = vec![0; GRID_HEIGHT as usize];
    for segment in &world.special_links {
        if !segment.active {
            continue;
        }
        let bit = if (segment.flags & 1) != 0 { 2 } else { 1 };
        let extent_minus_one = js::sar(segment.flags, 1);
        // Binary encoding: top_floor = entry_floor + (flags >> 1) + 1.
        let top = segment.entry_floor + extent_minus_one + 1;
        let mut floor = segment.entry_floor;
        while floor <= top {
            if floor >= 0 && floor < GRID_HEIGHT {
                world.floor_walkability_flags[floor as usize] |= bit;
            }
            floor += 1;
        }
    }

    for (record_index, center) in derived_record_centers(world.lobby_mode).into_iter().enumerate() {
        if record_index as i64 >= MAX_SPECIAL_LINK_RECORDS {
            break;
        }
        let lower_floor = scan_special_link_span_bound(world, center, 0);
        let upper_floor = scan_special_link_span_bound(world, center, 1);
        if lower_floor >= upper_floor {
            continue;
        }
        world.special_link_records[record_index] = SpecialLinkRecord {
            active: true,
            lower_floor,
            upper_floor,
            reachability_masks_by_floor: vec![0; GRID_HEIGHT as usize],
        };
    }
}

/// `dir` is upstream's `0 | 1`.
pub fn scan_special_link_span_bound(world: &WorldState, center_floor: i64, dir: i64) -> i64 {
    let mut seen_gap = false;

    if dir != 0 {
        let mut floor = center_floor;
        while floor < center_floor + 6 {
            let flags = js::at_or(&world.floor_walkability_flags, floor, 0);
            if flags == 0 {
                return floor;
            }
            if (flags & 1) == 0 {
                seen_gap = true;
            }
            if seen_gap && floor >= center_floor + 3 {
                return floor;
            }
            floor += 1;
        }
        return center_floor + 6;
    }

    // Binary 11b8:0763 starts at centerFloor-1 (not centerFloor) and returns
    // floor+1 (not floor) on the zero-flag exit, so the lower bound stops one
    // floor above the first non-walkable floor below the center.
    let mut floor = center_floor - 1;
    while floor > center_floor - 6 {
        let flags = js::at_or(&world.floor_walkability_flags, floor, 0);
        if flags == 0 {
            return floor + 1;
        }
        if (flags & 1) == 0 {
            seen_gap = true;
        }
        if seen_gap && floor <= center_floor - 3 {
            return floor;
        }
        floor -= 1;
    }
    center_floor - 5
}

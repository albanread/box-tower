// ts: events.ts

use crate::commands::apply_remove_elevator_car;
use crate::js;
use crate::ledger::{remove_cashflow_from_family_resource, LedgerState};
use crate::resources::{FAMILY_METRO_BOTTOM, FAMILY_METRO_MIDDLE, FAMILY_METRO_TOP};
use crate::sims::parking::{rebuild_parking_coverage, rebuild_parking_demand_log};
use crate::sims::release_service_request;
use crate::time::TimeState;
use crate::world::{
    sample_rng, y_to_floor, EventState, FireRescueHelper, SidecarRecord, SimNotification, SimNotificationKind,
    SimPrompt, SimPromptKind, WorldState, GRID_HEIGHT,
};

// ─── Tuning constants ────────────────────────────────────────────────────────
// From startup tuning resource (type 0xff05, id 1000) at DS:0xe6xx.

const BOMB_DEADLINE_TICKS: i64 = 0x4b0; // 1200
/// Ransom amounts indexed by star count (2, 3, 4). Stars 1 and 5 cannot trigger.
const BOMB_RANSOM: &[(i64, i64)] = &[(2, 200_000), (3, 300_000), (4, 1_000_000)];
/// Blast rectangle dimensions.
const BLAST_HALF_TILES: i64 = 20;

const FIRE_SPREAD_RATE: i64 = 7; // DS:0xe644 — ticks per tile advance
const FIRE_VERTICAL_DELAY: i64 = 80; // DS:0xe646 — ticks per floor
const HELICOPTER_EXTINGUISH_RATE: i64 = 1; // DS:0xe648 — ticks per tile
const HELICOPTER_PROMPT_DELAY: i64 = 2; // DS:0xe64a — ticks after fire start / bomb extension
const RESCUE_COUNTDOWN_WITH_SECURITY: i64 = 80; // DS:0xe64c
const HELICOPTER_RESCUE_COST: i64 = 500_000; // DS:0xe688

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// `n.toLocaleString()` as node writes it in the "en-US" locale: digits in
/// groups of three, separated by commas.
fn to_locale_string(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// `Number(s)` of one coordinate of an "x,y" key (`None` where JavaScript
/// would make NaN; every key the simulation writes is two integers).
fn key_part(key: &str, part: usize) -> Option<i64> {
    key.split(',').nth(part).and_then(|s| s.parse().ok())
}

/// Find the highest populated floor (has at least one placed object).
fn highest_populated_floor(world: &WorldState) -> i64 {
    let mut max_floor = -1;
    for key in world.placed_objects.keys() {
        let Some(y) = key_part(key, 1) else { continue };
        let floor = y_to_floor(y);
        if floor > max_floor {
            max_floor = floor;
        }
    }
    max_floor
}

/// Upstream returns `{ left, right } | null`: here `Some((left, right))`.
fn floor_tile_bounds_for_floor(world: &WorldState, floor: i64) -> Option<(i64, i64)> {
    // Match the binary's per-floor header bounds (field0_0x0[+2]/[+4],
    // initialized by `setup_floor_support` and read by the fire spread loop
    // at 10f0:03f2 / 10f0:0379). We approximate by scanning `world.cells` —
    // floor support + facilities — and rely on the augmented
    // `deleteObjectCoveringFloorTile` below to remove cells as fire eats
    // structure, so subsequent reads see the shrunk extent.
    //
    // Firefighter-rescue helper sims (modeled in `tickFireRescueHelpers`
    // below) extinguish fronts via `extinguish_fire_front_at_tile` on the
    // decline path. The bounds check here is the secondary clear (when fire
    // outruns the floor's structural extent) and the cell-deletion side
    // effect of `deleteObjectCoveringFloorTile` shrinks bounds as fire eats
    // through structure.
    let mut bounds: Option<(i64, i64)> = None;
    let obj_y = GRID_HEIGHT - 1 - floor;
    for key in world.cells.keys() {
        if key_part(key, 1) != Some(obj_y) {
            continue;
        }
        let Some(x) = key_part(key, 0) else { continue };
        bounds = Some(match bounds {
            None => (x, x),
            Some((left, right)) => (left.min(x), right.max(x)),
        });
    }
    bounds
}

fn seed_bomb_search_cursor(world: &mut WorldState, floor: i64) {
    let bounds = floor_tile_bounds_for_floor(world, floor);
    world.event_state.bomb_search_current_floor = floor;
    world.event_state.bomb_search_scan_tile = match bounds {
        Some((_, right)) => right - 2,
        None => -1,
    };
}

/// Mirrors the binary's `g_security_office_count > 0` guard at
/// `trigger_fire_event` (10f0:003a) and `trigger_bomb_event` (10d0:0086).
/// The binary's counter at DS:0xbc6e is never incremented by placement
/// (`allocate_security_office_table_slot` populates the slot table but not
/// the count) — TS doesn't have that bug because we recompute from anchors.
fn has_emergency_response_coverage(world: &WorldState) -> bool {
    world.placed_objects.values().any(|record| record.object_type_code == 0x0e) // FAMILY_SECURITY
}

// Random-news event: in the binary this drives audio playback only. The
// classifier result feeds play_classified_news_sound (11d0:042c), which loads a
// wave resource by ID and plays it via WAVMIX16.waveOutWrite. There is no
// visual popup on this path. The worker does not model audio; the client will
// own playback once the audio subsystem lands.

/// Family codes the binary's `FUN_1200_3474` (1200:3474) validity gate
/// blocks from `delete_placed_object_and_release_sidecars`. The fire spread
/// code in `advance_fire_frontier_damage` calls
/// `delete_object_covering_floor_tile` (1200:3619) per spread tick, which
/// forwards into the validity gate — meaning fire **cannot destroy** these
/// families. Their tiles survive intact through a fire and bound the
/// spread fronts (the bounded right-front clears that the trace shows are
/// the visible consequence of this gating).
///
/// Includes: security (0x0e), housekeeping (0x0f), parking + parking-aux
/// (0x18/0x19/0x1a), metro (0x1f/0x20), cathedral anchor + cells
/// (0x21/0x24-0x28), parking ramp lobby (0x2d). Floor support / lobby /
/// stairs / elevators are excluded from the placedObjects path entirely
/// (they live in `world.cells` only or in `world.carriers`), so fire's
/// record-deletion never touches them.
const FIRE_INDESTRUCTIBLE_FAMILIES: &[i64] = &[
    0x0e, 0x0f, 0x18, 0x19, 0x1a, 0x1f, 0x20, 0x21, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2d,
];

/// Mirror per-family side effects of `delete_placed_object_and_release_sidecars`
/// (1200:369d) for the subset of behavior the fire teardown depends on:
///
///   - Cash refund for condo (`remove_cashflow_from_family_resource`,
///     1180:0966) — the only family with a direct cash refund on teardown.
///   - Sim eviction sweep: sims whose home pointer matches the destroyed
///     anchor get zeroed (binary `update_sim_tile_span` 1228:1018 second
///     pass writes `sim[+4]=0; sim[+6]=0`).
///   - Commercial-venue sidecar release (0x06/0x0a/0x0c): tag the linked
///     CommercialVenueRecord as VENUE_DORMANT (0xff) — equivalent to the
///     binary's `*(commercial_venue_table + sidecarIdx*0x12 + 1) = 0xFF`
///     freelist push.
///   - Entertainment-link record release (0x12/0x13/0x1d/0x1e/0x22/0x23):
///     reset linked EntertainmentLinkRecord halves to the binary's `0xfe`
///     "free" sentinel (`free_entertainment_link_record` 1188:0ee5).
///
/// Carrier-sidecar GlobalAlloc/GlobalFree handle release (`FUN_1190_0884`,
/// key table at DS:0xe5a8/count DS:0xbc78) is intentionally a no-op in TS:
/// the binary uses the table to track Win16 GMEM handles for hotel/cinema/
/// party-hall/condo cash-flow link records, but TS holds those records
/// directly in `world.sidecars` (released by the
/// commercial/entertainment paths above) without any handle-table layer
/// to clean up.
const FIRE_COMMERCIAL_FAMILIES: &[i64] = &[0x06, 0x0a, 0x0c];
const FIRE_ENTERTAINMENT_FAMILIES: &[i64] = &[0x12, 0x13, 0x1d, 0x1e, 0x22, 0x23];

/// Upstream passes the record itself; here its key in `world.placed_objects`.
/// The record is only read, and every field is read before anything below
/// could change the placed objects, so it is read once at the top.
fn apply_teardown_side_effects(world: &mut WorldState, ledger: &mut LedgerState, record_key: &str, obj_y: i64) {
    let record = world.placed_objects[record_key].clone();
    let family = record.object_type_code;
    let cond = record.unit_status;
    // Cash refund: condo only. `cond < 0x18` matches binary 10f0:026e.
    if family == 0x09 && cond < 0x18 {
        remove_cashflow_from_family_resource(ledger, "condo", record.eval_level, 9);
    }
    // Commercial-venue freelist push: mark the sidecar dormant. The
    // rebuild_linked_facility_records pass on the next 0x0f0 checkpoint
    // will skip this sidecar and the venue won't get reseeded — equivalent
    // to the binary's owner-byte 0xff freelist tag.
    if FIRE_COMMERCIAL_FAMILIES.contains(&family) && record.linked_record_index >= 0 {
        if let Some(SidecarRecord::CommercialVenue(sidecar)) =
            world.sidecars.get_mut(record.linked_record_index as usize)
        {
            sidecar.availability_state = 0xff; // VENUE_DORMANT
        }
    }
    // Entertainment-link freelist push: zero the linked record's halves.
    // Binary writes 0xfe to upper/lowerHalfFloor; we mirror that and reset
    // the active runtime count so the link is treated as unowned.
    if FIRE_ENTERTAINMENT_FAMILIES.contains(&family) && record.linked_record_index >= 0 {
        if let Some(SidecarRecord::EntertainmentLink(sidecar)) =
            world.sidecars.get_mut(record.linked_record_index as usize)
        {
            sidecar.upper_half_floor = 0xfe;
            sidecar.lower_half_floor = 0xfe;
            sidecar.link_phase_state = 0;
            sidecar.active_runtime_count = 0;
        }
    }
    // Sim eviction sweep: find sims with home pointer landing inside the
    // destroyed record and zero them. The binary's `update_sim_tile_span`
    // (1228:1018) second pass writes `sim[+4]=0; sim[+6]=0`. For office
    // (0x07) and hotel families (0x03/0x04/0x05) the first pass also calls
    // `release_service_request_entry` (11a0:0489) — we mirror that here so
    // post-fire dispatch loops don't try to fulfill requests against a
    // destroyed facility.
    let left = record.left_tile_index;
    let right = record.right_tile_index;
    let releases_service = family == 0x07 || family == 0x03 || family == 0x04 || family == 0x05;
    for sim_index in 0..world.sims.len() as i64 {
        let sim = &world.sims[sim_index as usize];
        if sim.family_code != family {
            continue;
        }
        if sim.floor_anchor != obj_y {
            continue;
        }
        if sim.home_column < left || sim.home_column > right {
            continue;
        }
        if releases_service {
            release_service_request(world, sim_index);
        }
        let sim = &mut world.sims[sim_index as usize];
        sim.family_code = 0;
        sim.state_code = 0;
    }
    // Parking ramp coverage rebuild — binary `delete_placed_object_and_release_sidecars`
    // post-pass at 1200:36ec calls `rebuild_parking_ramp_coverage_and_demand_history`
    // (11a0:07b5) when the deleted family is 0x2c (ramp) or 0x0b (lobby). TS
    // doesn't store lobby in placedObjects, so only the ramp branch is
    // reachable here.
    if family == 0x2c {
        rebuild_parking_coverage(world);
        rebuild_parking_demand_log(world);
    }
}

/// Delete the object/structure covering a given floor/tile.
///
/// Mirrors `delete_object_covering_floor_tile` (1200:3619) →
/// `delete_placed_object_and_release_sidecars` (1200:369d), including the
/// validity gate that blocks destruction of indestructible families.
///
/// On non-blocked deletions, also removes the cell at (tile, objY) so the
/// fire spread bounds shrink as the binary's per-floor header bounds do.
fn delete_object_covering_floor_tile(world: &mut WorldState, ledger: &mut LedgerState, floor: i64, tile: i64) {
    let obj_y = GRID_HEIGHT - 1 - floor;
    let hit = world.placed_objects.iter().find_map(|(key, record)| {
        if key_part(key, 1) != Some(obj_y) {
            return None;
        }
        if tile < record.left_tile_index || tile > record.right_tile_index {
            return None;
        }
        Some((key.to_string(), record.object_type_code, record.left_tile_index, record.right_tile_index))
    });
    if let Some((key, family, left, right)) = hit {
        // Validity gate: indestructible families survive fire.
        if FIRE_INDESTRUCTIBLE_FAMILIES.contains(&family) {
            return;
        }
        apply_teardown_side_effects(world, ledger, &key, obj_y);
        // Remove every cell the record covered, mirroring the binary's
        // per-floor blob compaction (FUN_1200_3b78). Doing this only at
        // the spread tile would leave neighboring cells of the same record
        // in the cell map, inflating bounds reads on subsequent ticks.
        //
        // (Upstream reads the record's tile range again here; the teardown
        // above does not change the record.)
        for x in left..=right {
            let k = format!("{x},{obj_y}");
            world.cells.remove(&k);
            world.cell_to_anchor.remove(&k);
        }
        world.detach_object(&key);
        world.placed_objects.remove(&key);
        return;
    }
    // No facility record matched — still remove the cell at this position
    // (e.g. floor support tile placed individually). This is what shrinks
    // bounds when the right front sweeps through structural floor support.
    let cell_key = format!("{tile},{obj_y}");
    world.cells.remove(&cell_key);
    world.cell_to_anchor.remove(&cell_key);
}

/// Shared floor-selection helper matching the binary's contiguous-live-floor scan.
/// Scans upward from lowerBound to find the first non-empty floor, then the first
/// empty floor after that contiguous occupied run. Returns a uniformly chosen floor
/// from [lowerBound, topLiveFloor], or -1 if no occupied floors exist above lowerBound.
fn select_random_live_floor(world: &mut WorldState, lower_bound: i64) -> i64 {
    // Find the first non-empty floor at or above lowerBound
    let mut first_occupied = -1;
    for f in lower_bound..GRID_HEIGHT {
        if floor_tile_bounds_for_floor(world, f).is_some() {
            first_occupied = f;
            break;
        }
    }
    if first_occupied < 0 {
        return -1;
    }

    // Find the end of the contiguous occupied run
    let mut top_live_floor = first_occupied;
    for f in (first_occupied + 1)..GRID_HEIGHT {
        if floor_tile_bounds_for_floor(world, f).is_none() {
            break;
        }
        top_live_floor = f;
    }

    let range = top_live_floor - lower_bound + 1;
    if range <= 0 {
        return -1;
    }
    lower_bound + (sample_rng(world) % range)
}

// ─── Bomb Event ──────────────────────────────────────────────────────────────

/// Check and trigger bomb event. Called at the daily event checkpoint
/// when `day_counter % 60 == 59`.
pub fn try_trigger_bomb_event(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState) {
    // Guard: no active bomb or fire
    if (world.event_state.game_state_flags & 9) != 0 {
        return;
    }
    // Guard: tower has at least one valid floor
    let top_floor = highest_populated_floor(world);
    if top_floor < 0 {
        return;
    }
    // Guard: early part of day
    if time.day_tick >= 0x4b1 {
        return;
    }
    // Guard: star 2, 3, or 4 only
    if world.star_count < 2 || world.star_count > 4 {
        return;
    }

    // Binary: `g_lobby_height + 10` (DS:0xbc5a + 10), no underground term.
    let lobby_height = 1.max(world.lobby_height);
    let min_floor = lobby_height + 10;

    // Select floor via contiguous-live-floor scan
    let selected_floor = select_random_live_floor(world, min_floor);
    if selected_floor < 0 {
        return;
    }

    // Require floor width >= 4 tiles
    let Some((left, right)) = floor_tile_bounds_for_floor(world, selected_floor) else {
        return;
    };
    if right - left < 4 {
        return;
    }

    let tile_range = right - left - 4;
    let selected_tile = left + if tile_range > 0 { sample_rng(world) % (tile_range + 1) } else { 0 };

    let es = &mut world.event_state;
    es.bomb_floor = selected_floor;
    es.bomb_tile = selected_tile;
    es.game_state_flags |= 1; // bomb active
    es.bomb_deadline = BOMB_DEADLINE_TICKS;
    es.bomb_search_lower_bound = selected_floor - 1;
    es.bomb_search_upper_bound = selected_floor;
    if has_emergency_response_coverage(world) {
        seed_bomb_search_cursor(world, selected_floor);
    } else {
        world.event_state.bomb_search_current_floor = -1;
        world.event_state.bomb_search_scan_tile = -1;
    }

    let ransom = js::record_get(BOMB_RANSOM, world.star_count).copied().unwrap_or(0);
    world.pending_prompts.push(SimPrompt {
        prompt_id: format!("bomb_{}", time.day_counter),
        prompt_kind: SimPromptKind::BombRansom,
        message: format!(
            "A bomb threat has been received! Pay ${} ransom to avoid detonation?",
            to_locale_string(ransom)
        ),
        cost: Some(ransom),
    });
}

/// Per-tick bomb handler.
///
/// `time` is written: the cleanup fast-forwards the day (as upstream writes
/// through its `time` parameter).
pub fn tick_bomb_event(world: &mut WorldState, ledger: &mut LedgerState, time: &mut TimeState) {
    // Check found/detonated cleanup
    if (world.event_state.game_state_flags & 0x60) != 0 && time.day_tick >= world.event_state.bomb_deadline {
        bomb_cleanup(world, time);
        return;
    }
    // Check active search deadline → detonation
    if (world.event_state.game_state_flags & 1) != 0 && has_emergency_response_coverage(world) {
        advance_bomb_search(world, ledger, time);
    }
    // BOX: street -- under BOX RULES parked police cars search too.
    if (world.event_state.game_state_flags & 1) != 0 {
        match crate::box_ext::street::police_search(world) {
            crate::box_ext::street::PoliceSearch::Start => {
                let floor = world.event_state.bomb_floor;
                seed_bomb_search_cursor(world, floor);
            }
            crate::box_ext::street::PoliceSearch::Step => advance_bomb_search(world, ledger, time),
            crate::box_ext::street::PoliceSearch::None => {}
        }
    }
    if (world.event_state.game_state_flags & 1) != 0 && time.day_tick >= world.event_state.bomb_deadline {
        resolve_bomb_search(world, ledger, time, false);
    }
}

fn advance_bomb_search(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    if world.event_state.bomb_search_current_floor < 0 {
        return;
    }
    let floor = world.event_state.bomb_search_current_floor;
    let Some((left, right)) = floor_tile_bounds_for_floor(world, floor) else {
        if !advance_bomb_search_floor(world) {
            world.event_state.bomb_search_current_floor = -1;
        }
        return;
    };
    let es = &mut world.event_state;
    if es.bomb_search_scan_tile < 0 {
        es.bomb_search_scan_tile = right - 2;
    }
    if left < es.bomb_search_scan_tile {
        es.bomb_search_scan_tile -= 1;
        let tile = es.bomb_search_scan_tile;
        check_security_patrol_bomb(world, ledger, time, floor, tile);
        return;
    }
    if !advance_bomb_search_floor(world) {
        world.event_state.bomb_search_current_floor = -1;
    }
}

fn advance_bomb_search_floor(world: &mut WorldState) -> bool {
    let es = &world.event_state;
    let current = es.bomb_search_current_floor;
    if current < 0 {
        return false;
    }
    let try_above = if current <= es.bomb_floor { es.bomb_search_upper_bound } else { es.bomb_search_lower_bound };
    let try_below = if current <= es.bomb_floor { es.bomb_search_lower_bound } else { es.bomb_search_upper_bound };
    for candidate in [try_above, try_below] {
        if candidate < 0 || candidate >= GRID_HEIGHT {
            continue;
        }
        if candidate == current {
            continue;
        }
        if floor_tile_bounds_for_floor(world, candidate).is_none() {
            continue;
        }
        if candidate >= world.event_state.bomb_floor {
            world.event_state.bomb_search_upper_bound = candidate + 1;
        } else {
            world.event_state.bomb_search_lower_bound = candidate - 1;
        }
        seed_bomb_search_cursor(world, candidate);
        return true;
    }
    false
}

/// Called when a security guard visits a floor/tile.
/// If it matches the bomb position, the bomb is found.
pub fn check_security_patrol_bomb(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &TimeState,
    floor: i64,
    tile: i64,
) {
    let es = &world.event_state;
    if (es.game_state_flags & 1) == 0 {
        return; // no active bomb search
    }
    if floor == es.bomb_floor && tile == es.bomb_tile {
        resolve_bomb_search(world, ledger, time, true);
    }
}

fn resolve_bomb_search(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, found: bool) {
    let es = &mut world.event_state;
    es.game_state_flags &= !1; // clear active search
    if found {
        es.game_state_flags |= 0x20; // bomb found
        es.bomb_deadline = time.day_tick + HELICOPTER_PROMPT_DELAY; // reuse tuning delay
    } else {
        es.game_state_flags |= 0x40; // bomb detonated
        apply_blast_damage(world, ledger);
        world.event_state.bomb_deadline = time.day_tick + 50; // short delay before cleanup
    }
}

fn apply_blast_damage(world: &mut WorldState, ledger: &mut LedgerState) {
    let es = &world.event_state;
    let floor_min = es.bomb_floor - 2;
    let floor_max = es.bomb_floor + 3;
    let tile_min = es.bomb_tile - BLAST_HALF_TILES;
    let tile_max = es.bomb_tile + BLAST_HALF_TILES - 1;
    for floor in floor_min..=floor_max {
        for tile in tile_min..=tile_max {
            delete_object_covering_floor_tile(world, ledger, floor, tile);
        }
    }
}

fn bomb_cleanup(world: &mut WorldState, time: &mut TimeState) {
    let es = &mut world.event_state;
    // Clear found/detonated flags
    es.game_state_flags &= !0x60;
    es.bomb_search_current_floor = -1;
    es.bomb_search_scan_tile = -1;
    es.bomb_search_lower_bound = -1;
    es.bomb_search_upper_bound = -1;
    // Fast-forward time to 1500 if earlier
    if time.day_tick < 1500 {
        time.day_tick = 1500;
        time.daypart_index = js::floor_div(1500, 400);
    }
}

// ─── Fire Event ──────────────────────────────────────────────────────────────

/// Check and trigger fire event. Called at the daily event checkpoint
/// when `day_counter % 84 == 83`.
pub fn try_trigger_fire_event(world: &mut WorldState, _ledger: &mut LedgerState, time: &TimeState) {
    // Guard: no active bomb or fire
    if (world.event_state.game_state_flags & 9) != 0 {
        return;
    }
    // Guard: tower has at least one valid floor
    let top_floor = highest_populated_floor(world);
    if top_floor < 0 {
        return;
    }
    // Guard: early daypart (< 4)
    if time.daypart_index >= 4 {
        return;
    }
    // Guard: star > 2
    if world.star_count <= 2 {
        return;
    }
    // Guard: no cathedral guest dispatch active
    if world.gate_flags.eval_sim_index >= 0 && world.gate_flags.eval_sim_index != 0xffff {
        return;
    }

    // Binary: `g_lobby_height + 10` (DS:0xbc5a + 10), no underground term.
    let lobby_height = 1.max(world.lobby_height);
    let min_floor = lobby_height + 10;

    // Select floor via contiguous-live-floor scan
    let selected_floor = select_random_live_floor(world, min_floor);
    if selected_floor < 0 {
        return;
    }

    // Require floor width >= 32 tiles
    let Some((left, right)) = floor_tile_bounds_for_floor(world, selected_floor) else {
        return;
    };
    if right - left < 0x20 {
        return;
    }

    let coverage = has_emergency_response_coverage(world);
    let es = &mut world.event_state;
    es.fire_floor = selected_floor;
    es.fire_tile = right - 0x20;
    es.game_state_flags |= 8; // fire active
    es.fire_start_tick = time.day_tick;
    es.fire_left_pos.fill(0xffff);
    es.fire_right_pos.fill(0xffff);
    // Eager-init the fireFloor entries to fireTile — mirrors the binary's
    // trigger writes at `fireFloor*2 + 0xbd7e` / `fireFloor*2 + 0xbc8e`
    // (10f0:00d0-ish region). The spread loop then advances from this seed
    // rather than waiting for an "ignition tick" to lazy-init.
    es.fire_left_pos[selected_floor as usize] = es.fire_tile;
    es.fire_right_pos[selected_floor as usize] = es.fire_tile;

    if coverage {
        es.rescue_countdown = RESCUE_COUNTDOWN_WITH_SECURITY;
    } else {
        es.rescue_countdown = 0;
    }
    es.helicopter_extinguish_pos = 0;
}

/// Per-tick fire spread and resolution.
///
/// `time` is written: the resolution fast-forwards the day.
pub fn tick_fire_event(world: &mut WorldState, ledger: &mut LedgerState, time: &mut TimeState) {
    if (world.event_state.game_state_flags & 8) == 0 {
        return;
    }

    // Rescue countdown
    if world.event_state.rescue_countdown > 0 {
        world.event_state.rescue_countdown -= 1;
        return;
    }

    // Helicopter extinguish
    if world.event_state.helicopter_extinguish_pos > 0 {
        advance_helicopter_extinguish(world, time);
    }

    // Normal fire spread
    advance_fire_spread(world, ledger, time);

    // Firefighter rescue helpers walk + asymmetric clear (binary
    // `firefighter_sim_per_tick_handler` 1100:0bd3). Helpers are spawned by
    // the prompt-decline path; if no helpers exist yet (e.g. before the
    // player has answered) this is a no-op.
    tick_fire_rescue_helpers(world, time);

    // BOX: street -- under BOX RULES parked fire engines put out the floors
    // their ladder reaches, and a fire with nothing burning is out (false
    // at once with BOX RULES off).
    if crate::box_ext::street::fire_out(world) {
        resolve_fire_event(world, time);
        return;
    }

    // Check helicopter prompt timing
    if world.event_state.helicopter_extinguish_pos == 0
        && time.day_tick == world.event_state.fire_start_tick + HELICOPTER_PROMPT_DELAY
    {
        world.pending_prompts.push(SimPrompt {
            prompt_id: format!("fire_{}", time.day_counter),
            prompt_kind: SimPromptKind::FireRescue,
            message: format!(
                "Fire is spreading! Call helicopter rescue for ${}?",
                to_locale_string(HELICOPTER_RESCUE_COST)
            ),
            cost: Some(HELICOPTER_RESCUE_COST),
        });
    }

    // Check resolution. Binary uses exact-equals at 10d0:0049 — cleanup
    // happens once at dayTick==2000 and fast-forwards to 1500. The
    // `isFireExhausted` shortcut a previous TS revision added is NOT in the
    // binary; the fire stays active until the timed cleanup so that
    // firefighter helpers + vertical delay ignitions on later floors can
    // still play out. Removing the shortcut matches binary parity.
    if time.day_tick == 2000 {
        resolve_fire_event(world, time);
    }
}

fn advance_fire_spread(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    // Binary `advance_fire_frontier_damage` (10f0:0306) iterates **all 120
    // floor entries**, not just up to the populated max — vertical-delay
    // ignitions on un-burned floors are governed by the per-floor array
    // state, not by the live-floor scan. Match that.
    for floor in 0..GRID_HEIGHT {
        let f = floor as usize;
        // Vertical spread propagates only upward in the binary trace —
        // fire on internal floor 11 ignites floor 12 at +80 ticks but
        // never floor 10 (the lobby/ground floor). Skip floors below the
        // fire floor.
        if floor < world.event_state.fire_floor {
            continue;
        }
        let floor_delay = (floor - world.event_state.fire_floor) * FIRE_VERTICAL_DELAY;
        let ignition_tick = world.event_state.fire_start_tick + floor_delay;

        if time.day_tick < ignition_tick {
            continue;
        }

        // Lazy ignition for non-fireFloor entries (the fireFloor itself was
        // eager-initialized by `tryTriggerFireEvent`). Binary trace shows
        // the previous floor's fronts get cleared simultaneously with the
        // next floor's ignition — fire moves upward one floor at a time,
        // it doesn't accumulate on multiple floors. Mirror that here.
        if world.event_state.fire_left_pos[f] == 0xffff && world.event_state.fire_right_pos[f] == 0xffff {
            if time.day_tick == ignition_tick {
                // BOX: street -- under BOX RULES a fire climbs only from a
                // burning floor (false at once with BOX RULES off).
                if crate::box_ext::street::fire_ignition_blocked(world, floor)
                    || crate::box_ext::fire::ignition_blocked(world, floor)
                {
                    continue;
                }
                let es = &mut world.event_state;
                es.fire_left_pos[f] = es.fire_tile;
                es.fire_right_pos[f] = es.fire_tile;
                if floor > es.fire_floor {
                    // (With a negative fireFloor upstream would write index -1,
                    // a property JSON never shows; there is nothing to clear.)
                    if f >= 1 {
                        es.fire_left_pos[f - 1] = 0xffff;
                        es.fire_right_pos[f - 1] = 0xffff;
                    }
                    // Re-dispatch firefighter helpers to the new floor
                    // (binary equivalent: reactivate retired helpers when
                    // a new floor ignites). Reset position to the spawn
                    // column (right + 12) so they walk fresh into the
                    // arrival window.
                    if let Some((_, right)) = floor_tile_bounds_for_floor(world, floor) {
                        for helper in world.event_state.fire_rescue_helpers.iter_mut() {
                            helper.floor = floor;
                            helper.column = right + 11;
                            helper.status = 0;
                            helper.windup_remaining = 0;
                        }
                    }
                }
            }
            continue;
        }

        // Spread cadence: binary uses **absolute** `g_day_tick % spread_rate
        // == 0` (10f0:039a, 10f0:0411), not elapsed-since-ignition. The
        // difference matters when `fireStartTick` is not a multiple of the
        // rate — the binary fires on tick boundaries shared across all
        // floors, while the elapsed model phases each floor independently.
        if time.day_tick % FIRE_SPREAD_RATE != 0 {
            continue;
        }
        // BOX: street -- parked fire engines halve the spread within their
        // ladder's reach (false at once with BOX RULES off).
        if crate::box_ext::street::fire_spread_held(world, floor, time.day_tick / FIRE_SPREAD_RATE)
            || crate::box_ext::fire::spread_held(world, floor, time.day_tick / FIRE_SPREAD_RATE)
        {
            continue;
        }

        let Some((left, right)) = floor_tile_bounds_for_floor(world, floor) else {
            continue;
        };

        // Left front: move left. Binary writes 0xffff sentinel when the
        // front crosses the floor's left bound (10f0:03a5).
        if world.event_state.fire_left_pos[f] != 0xffff {
            let tile = world.event_state.fire_left_pos[f];
            delete_object_covering_floor_tile(world, ledger, floor, tile);
            let es = &mut world.event_state;
            es.fire_left_pos[f] -= 1;
            if es.fire_left_pos[f] < left {
                es.fire_left_pos[f] = 0xffff;
            }
        }

        // Right front: move right, delete at position + 12. Binary writes
        // 0xffff when right+12 outruns the floor's right bound (10f0:0420).
        if world.event_state.fire_right_pos[f] != 0xffff {
            let delete_pos = world.event_state.fire_right_pos[f] + 12;
            if delete_pos <= right {
                delete_object_covering_floor_tile(world, ledger, floor, delete_pos);
            }
            let es = &mut world.event_state;
            es.fire_right_pos[f] += 1;
            if es.fire_right_pos[f] + 12 > right {
                es.fire_right_pos[f] = 0xffff;
            }
        }
    }
}

fn advance_helicopter_extinguish(world: &mut WorldState, time: &mut TimeState) {
    let es = &mut world.event_state;
    if es.helicopter_extinguish_pos <= 0 {
        return;
    }

    // Decrement position by 1 every HELICOPTER_EXTINGUISH_RATE ticks
    if time.day_tick % HELICOPTER_EXTINGUISH_RATE != 0 {
        return;
    }
    es.helicopter_extinguish_pos -= 1;

    // `clear_fire_boundaries_past_spread_width` (10f0:0858) — full 120-floor
    // sweep, write 0xffff sentinel.
    for floor in 0..GRID_HEIGHT as usize {
        if es.fire_left_pos[floor] != 0xffff && es.fire_left_pos[floor] > es.helicopter_extinguish_pos {
            es.fire_left_pos[floor] = 0xffff;
        }
        if es.fire_right_pos[floor] != 0xffff && es.fire_right_pos[floor] > es.helicopter_extinguish_pos {
            es.fire_right_pos[floor] = 0xffff;
        }
    }

    if es.helicopter_extinguish_pos <= 0 {
        resolve_fire_event(world, time);
    }
}

fn resolve_fire_event(world: &mut WorldState, time: &mut TimeState) {
    let es = &mut world.event_state;
    es.game_state_flags &= !8; // clear fire flag
    es.fire_left_pos.fill(0xffff);
    es.fire_right_pos.fill(0xffff);
    es.helicopter_extinguish_pos = 0;
    es.rescue_countdown = 0;
    // `initialize_service_response_entities(0)` — drain firefighter helpers
    // when fire resolves (binary `finalize_fire_event_cleanup` 10f0:02a1).
    es.fire_rescue_helpers = Vec::new();
    // BOX: street -- under BOX RULES the day carries on when a fire is put
    // out (classic: upstream's jump to tick 1500).
    if time.day_tick < 1500 && !crate::box_ext::box_rules(world) {
        time.day_tick = 1500;
        time.daypart_index = js::floor_div(1500, 400);
    }
}

// ─── Firefighter rescue helpers (`initialize_service_response_entities(8)` /
// `firefighter_sim_per_tick_handler`) ─────────────────────────────────────────
//
// Tuning words at DS:0xe640/0xe64e (loaded from runtime resource): walk_delay=1,
// extinguish_delay=5. Spawn count = number of placed FAMILY_SECURITY anchors
// (one helper per security office). Movement: right-to-left from each office's
// rightmost tile, one tile per `walk_delay` tick on the office's floor.
//
// Arrival predicate (`FUN_10f0_076c`): tile in the **inner** half of either
// front — `[left, left+6) ∪ [right+6, right+12)`. On arrival the helper waits
// `extinguish_delay` ticks then calls the asymmetric clear (10f0:07d8 wider
// predicate `[left, left+12) ∪ [right, right+12)`).

const FIRE_WALK_DELAY: i64 = 1; // DS:0xe640 (g_tuning_fire_walk_delay)
const FIRE_EXTINGUISH_DELAY: i64 = 5; // DS:0xe64e (g_tuning_fire_extinguish_delay)

/// Spawn one firefighter helper per placed FAMILY_SECURITY anchor. Mirrors
/// the binary's `initialize_service_response_entities(8)` (1100:033d) which
/// walks the security-office table and seeds the slot-0 occupant. Idempotent
/// — replaces any existing helpers (a fresh decline starts a new patrol).
fn spawn_fire_rescue_helpers(world: &mut WorldState) {
    let mut helpers: Vec<FireRescueHelper> = Vec::new();
    for (key, record) in world.placed_objects.iter() {
        if record.object_type_code != 0x0e {
            continue; // FAMILY_SECURITY
        }
        // `GRID_HEIGHT - 1 - Number(yStr)`; the keys are always "x,y".
        let y = key_part(key, 1).expect("placed-object key is \"x,y\"");
        let office_floor = GRID_HEIGHT - 1 - y;
        // Spawn off the right edge of the office's floor and walk left.
        // Binary observation: helpers start at `floor_right_extent + 12` (one
        // fire spread-window past the rightmost cell), so they walk INTO the
        // fire's right arrival window `[right+6, right+12)` from outside the
        // building. With walk_delay=1, extinguish_delay=5 this exactly
        // reproduces the trace's tick-36 clear on a [80,140) floor extent.
        let bounds = floor_tile_bounds_for_floor(world, office_floor);
        // Spawn one tile inside `bounds.right + 12` so the helper's first walk
        // tick lands it at `right + 12` exactly, matching the binary's tick-36
        // clear timing on the canonical [80,140) floor extent.
        let spawn_column = match bounds {
            Some((_, right)) => right + 11,
            None => record.right_tile_index,
        };
        helpers.push(FireRescueHelper {
            floor: office_floor,
            column: spawn_column,
            status: 0,
            windup_remaining: 0,
        });
    }
    world.event_state.fire_rescue_helpers = helpers;
}

/// Per-tick advance for one firefighter helper. Mirrors
/// `firefighter_sim_per_tick_handler` (1100:0bd3) walk + arrival + wind-up
/// + asymmetric clear.
///
/// Upstream passes the helper; here its index in
/// `world.event_state.fire_rescue_helpers`.
fn advance_fire_rescue_helper(world: &mut WorldState, helper_index: i64, time: &TimeState) {
    let es = &mut world.event_state;
    let helper = &mut es.fire_rescue_helpers[helper_index as usize];
    if helper.status == 3 {
        return; // done
    }

    if helper.status == 1 {
        // Winding up to extinguish; on countdown to 0 fire the asymmetric clear
        // then retire — binary trace shows each helper extinguishes one front
        // per spawn cycle and waits for re-dispatch.
        helper.windup_remaining -= 1;
        if helper.windup_remaining <= 0 {
            let (floor, column) = (helper.floor, helper.column);
            helper.status = 3;
            extinguish_fire_front_at_tile(es, floor, column);
        }
        return;
    }

    // Walking. Move one tile per FIRE_WALK_DELAY ticks. With walk_delay=1
    // we move every tick. (When walk_delay > 1 we'd need a per-helper
    // cadence counter, but the binary's tuning is 1 here.)
    if time.day_tick % FIRE_WALK_DELAY != 0 {
        return;
    }

    let left = es.fire_left_pos[helper.floor as usize];
    let right = es.fire_right_pos[helper.floor as usize];
    let left_active = left != 0xffff;
    let right_active = right != 0xffff;
    if !left_active && !right_active {
        // No active front on this floor — try to relocate. Look for any other
        // floor with a live front and move there. (Binary
        // `move_bomb_search_response_sim_to_floor`.) When no fire is active
        // anywhere right now, just idle this tick — helper stays alive until
        // fire resolves so it can reactivate when a vertical-delay ignition
        // brings up a new floor.
        for f in 0..GRID_HEIGHT {
            let es = &world.event_state;
            if es.fire_left_pos[f as usize] != 0xffff || es.fire_right_pos[f as usize] != 0xffff {
                if f != es.fire_rescue_helpers[helper_index as usize].floor {
                    // Reset column to spawn-side for the new floor so the
                    // arrival predicate has room to engage as fronts spread.
                    let bounds = floor_tile_bounds_for_floor(world, f);
                    let helper = &mut world.event_state.fire_rescue_helpers[helper_index as usize];
                    helper.floor = f;
                    helper.column = match bounds {
                        Some((_, right)) => right + 11,
                        None => helper.column,
                    };
                }
                return;
            }
        }
        return;
    }

    // Arrival predicate (FUN_10f0_076c): inner half of either front
    let in_left_window = left_active && helper.column >= left && helper.column < left + 6;
    let in_right_window = right_active && helper.column >= right + 6 && helper.column < right + 12;
    if in_left_window || in_right_window {
        helper.status = 1;
        helper.windup_remaining = FIRE_EXTINGUISH_DELAY;
        return;
    }

    if helper.column > 0 {
        helper.column -= 1;
    }
}

/// Asymmetric front clear at a tile (`extinguish_fire_front_at_tile` 10f0:07d8).
/// Wider window than the arrival predicate: `[left, left+12)` for left front,
/// `[right, right+12)` for right front. Either match writes 0xffff.
fn extinguish_fire_front_at_tile(es: &mut EventState, floor: i64, tile: i64) {
    let f = floor as usize;
    let left = es.fire_left_pos[f];
    if left != 0xffff && tile >= left && tile < left + 12 {
        es.fire_left_pos[f] = 0xffff;
    }
    let right = es.fire_right_pos[f];
    if right != 0xffff && tile >= right && tile < right + 12 {
        es.fire_right_pos[f] = 0xffff;
    }
}

fn tick_fire_rescue_helpers(world: &mut WorldState, time: &TimeState) {
    for helper_index in 0..world.event_state.fire_rescue_helpers.len() {
        advance_fire_rescue_helper(world, helper_index as i64, time);
    }
}

// ─── VIP Special Visitor Event ───────────────────────────────────────────────

/// Per-tick VIP special visitor check. Binary-verified from
/// `trigger_vip_special_visitor` at 11f0:0273.
pub fn tick_vip_special_visitor(world: &mut WorldState, time: &TimeState) {
    let es = &world.event_state;
    // Guards
    if time.day_tick <= 0xf0 {
        return;
    }
    if time.daypart_index >= 4 {
        return;
    }
    if (es.game_state_flags & 9) != 0 {
        return;
    }
    // Eligibility is keyed off the metro placement/floor state.
    if world.gate_flags.metro_station_floor_index < 0 {
        return;
    }

    // 1% chance per tick
    if sample_rng(world) % 100 != 0 {
        return;
    }

    let mut any_arrival = false;
    for object in world.placed_objects.values_mut() {
        if object.object_type_code != FAMILY_METRO_TOP
            && object.object_type_code != FAMILY_METRO_MIDDLE
            && object.object_type_code != FAMILY_METRO_BOTTOM
        {
            continue;
        }
        if object.unit_status == 0 {
            object.unit_status = 2;
            any_arrival = true;
        } else {
            object.unit_status = 0;
        }
        object.dirty_flag = 1;
    }
    if any_arrival {
        world.pending_notifications.push(SimNotification {
            kind: SimNotificationKind::Event,
            message: Some("metro_train_arrival".to_string()),
        });
    }
}

// Random-news event: audio-only in the binary (play_classified_news_sound @
// 11d0:042c → WAVMIX16.waveOutWrite). No visual popup. The worker keeps the
// RNG-consuming gates so the shared RNG stream stays in sync with the binary,
// but emits nothing. When the client audio subsystem lands, it will own
// playback of the classifier's selected wave ID.
pub fn trigger_random_news_event(world: &mut WorldState, time: &TimeState) {
    let es = &world.event_state;
    if es.disable_news_events.unwrap_or(false) {
        return;
    }
    if time.day_tick <= 0xf0 {
        return;
    }
    if time.daypart_index >= 6 {
        return;
    }
    if (es.game_state_flags & 9) != 0 {
        return;
    }
    if sample_rng(world) % 16 != 0 {
        return;
    }
    // TODO(client-audio): viewport-sample 6 buckets, classify the sampled slot
    // per EVENTS.md "Random News Events", and play the resulting wave resource.
    // Until then the gate fires but no sound/notification is emitted.
}

// ─── Daily event checkpoint dispatcher ───────────────────────────────────────

/// Called from runCheckpoints at the daily event check timing.
/// Dispatches bomb/fire triggers based on day_counter modular conditions.
pub fn check_daily_events(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    if time.day_counter % 84 == 83 {
        try_trigger_fire_event(world, ledger, time);
    }
    if time.day_counter % 60 == 59 {
        try_trigger_bomb_event(world, ledger, time);
    }
}

/// Handle a player's response to a prompt.
/// Returns true if the prompt was recognized and handled.
///
/// `time` is written: paying the ransom fast-forwards the day.
pub fn handle_prompt_response(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    time: &mut TimeState,
    prompt_id: &str,
    accepted: bool,
) -> bool {
    if prompt_id.starts_with("bomb_") {
        if !accepted {
            return true; // refused — bomb stays armed (default behavior)
        }
        // Pay ransom: deactivate bomb
        let ransom = js::record_get(BOMB_RANSOM, world.star_count).copied().unwrap_or(0);
        if ransom > 0 && ledger.cash_balance >= ransom {
            ledger.cash_balance -= ransom;
            bomb_cleanup(world, time);
            world.pending_notifications.push(SimNotification {
                kind: SimNotificationKind::Event,
                message: Some("Ransom paid. The bomb threat has been resolved.".to_string()),
            });
        }
        return true;
    }

    if prompt_id.starts_with("fire_") {
        if (world.event_state.game_state_flags & 8) == 0 {
            return true; // fire already resolved
        }
        if !accepted {
            // Decline path mirrors the binary's `resolve_fire_rescue_followup`
            // (10f0:0149) IDOK branch: spawn one firefighter helper per placed
            // security office (`initialize_service_response_entities(8)`). The
            // helpers walk right-to-left from each office's column and clear
            // fire fronts on arrival.
            spawn_fire_rescue_helpers(world);
            return true;
        }
        let bounds = floor_tile_bounds_for_floor(world, world.event_state.fire_floor);
        if let Some((_, right)) = bounds {
            if ledger.cash_balance >= HELICOPTER_RESCUE_COST {
                ledger.cash_balance -= HELICOPTER_RESCUE_COST;
                world.event_state.helicopter_extinguish_pos = right - 12;
                world.pending_notifications.push(SimNotification {
                    kind: SimNotificationKind::Event,
                    message: Some("Helicopter dispatched to fight the fire!".to_string()),
                });
            }
        }
        return true;
    }

    if prompt_id.starts_with("carrier_remove_") {
        let column = world.event_state.pending_carrier_edit_column;
        let y = world.event_state.pending_carrier_edit_y;
        world.event_state.pending_carrier_edit_column = -1;
        world.event_state.pending_carrier_edit_y = -1;
        if !accepted || column < 0 || y < 0 {
            return true;
        }
        apply_remove_elevator_car(world, column, y);
        return true;
    }

    false
}

// ─── BOX: test hook ──────────────────────────────────────────────────────────

/// BOX: test hook -- start a fire (`fire`) or a bomb threat now, for the
/// front end's play tests: the event is set up as `try_trigger_fire_event`
/// and `try_trigger_bomb_event` set it up, but without their guards of the
/// day, the hour, the stars and the height (the highest floor with
/// anything on it above the lobby is chosen; a bomb's ransom is at least
/// the two-star tower's).  Nothing calls it in a game; true if one started.
pub fn test_start_event(world: &mut WorldState, time: &TimeState, fire: bool) -> bool {
    if (world.event_state.game_state_flags & 9) != 0 {
        return false;
    }
    let top = highest_populated_floor(world);
    // (BOX RULES: halfway up, so that people above it go out too)
    let from = if crate::box_ext::box_rules(world) && fire { (11 + top) / 2 } else { top };
    let floor = (11..=from).rev().find(|&f| floor_tile_bounds_for_floor(world, f).is_some()).unwrap_or(-1);
    let Some((left, right)) = (floor >= 0).then(|| floor_tile_bounds_for_floor(world, floor)).flatten() else {
        return false;
    };
    if fire {
        let coverage = has_emergency_response_coverage(world);
        let es = &mut world.event_state;
        es.fire_floor = floor;
        es.fire_tile = (right - 0x20).max(left);
        es.game_state_flags |= 8;
        es.fire_start_tick = time.day_tick;
        es.fire_left_pos.fill(0xffff);
        es.fire_right_pos.fill(0xffff);
        es.fire_left_pos[floor as usize] = es.fire_tile;
        es.fire_right_pos[floor as usize] = es.fire_tile;
        es.rescue_countdown = if coverage { RESCUE_COUNTDOWN_WITH_SECURITY } else { 0 };
        es.helicopter_extinguish_pos = 0;
        return true;
    }
    let es = &mut world.event_state;
    es.bomb_floor = floor;
    es.bomb_tile = left + (right - left - 4).max(0) / 2;
    es.game_state_flags |= 1;
    es.bomb_deadline = (time.day_tick + BOMB_DEADLINE_TICKS / 2).min(2590);
    es.bomb_search_lower_bound = floor - 1;
    es.bomb_search_upper_bound = floor;
    if has_emergency_response_coverage(world) {
        seed_bomb_search_cursor(world, floor);
    } else {
        world.event_state.bomb_search_current_floor = -1;
        world.event_state.bomb_search_scan_tile = -1;
    }
    let ransom = js::record_get(BOMB_RANSOM, world.star_count.clamp(2, 4)).copied().unwrap_or(0);
    world.pending_prompts.push(SimPrompt {
        prompt_id: format!("bomb_{}", time.day_counter),
        prompt_kind: SimPromptKind::BombRansom,
        message: format!(
            "A bomb threat has been received! Pay ${} ransom to avoid detonation?",
            to_locale_string(ransom)
        ),
        cost: Some(ransom),
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_string_groups_thousands() {
        assert_eq!(to_locale_string(0), "0");
        assert_eq!(to_locale_string(999), "999");
        assert_eq!(to_locale_string(200_000), "200,000");
        assert_eq!(to_locale_string(1_000_000), "1,000,000");
        assert_eq!(to_locale_string(-12_345), "-12,345");
    }

    #[test]
    fn floor_bounds_and_teardown_of_bare_cells() {
        let mut snap = crate::snapshot::create_initial_snapshot("t", "t", 0, None);
        let w = &mut snap.world;
        let y = GRID_HEIGHT - 1 - 20;
        for x in [5, 9, 7] {
            w.cells.insert(format!("{x},{y}"), "floor".to_string());
        }
        assert_eq!(floor_tile_bounds_for_floor(w, 20), Some((5, 9)));
        assert_eq!(floor_tile_bounds_for_floor(w, 21), None);
        assert_eq!(highest_populated_floor(w), -1);
        let mut l = crate::ledger::create_ledger_state(0);
        delete_object_covering_floor_tile(w, &mut l, 20, 9);
        assert_eq!(floor_tile_bounds_for_floor(w, 20), Some((5, 7)));
    }

    #[test]
    fn helicopter_extinguish_resolves_and_fast_forwards() {
        let mut snap = crate::snapshot::create_initial_snapshot("t", "t", 0, None);
        let w = &mut snap.world;
        let mut t = crate::time::create_time_state();
        t.day_tick = 300;
        w.event_state.game_state_flags = 8;
        w.event_state.helicopter_extinguish_pos = 1;
        w.event_state.fire_left_pos[30] = 50;
        w.event_state.fire_rescue_helpers.push(FireRescueHelper { floor: 30, column: 1, status: 0, windup_remaining: 0 });
        advance_helicopter_extinguish(w, &mut t);
        assert_eq!(w.event_state.game_state_flags, 0);
        assert_eq!(w.event_state.fire_left_pos[30], 0xffff);
        assert!(w.event_state.fire_rescue_helpers.is_empty());
        assert_eq!((t.day_tick, t.daypart_index), (1500, 3));
    }

    /// commands.test.ts's metro arrival check, on a hand-built world (the
    /// TowerSim version in commands.rs waits for the merge): from LCG state
    /// 54 the first sample is a multiple of 100.
    #[test]
    fn metro_arrival_toggles_the_stack() {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        for (y, family) in [(110, FAMILY_METRO_TOP), (111, FAMILY_METRO_MIDDLE), (112, FAMILY_METRO_BOTTOM)] {
            let record: crate::world::PlacedObjectRecord = serde_json::from_value(serde_json::json!({
                "leftTileIndex": 100, "rightTileIndex": 129, "objectTypeCode": family, "unitStatus": 0,
                "auxValueOrTimer": 0, "linkedRecordIndex": -1, "dirtyFlag": 1, "occupiedFlag": 1,
                "evalLevel": 255, "evalScore": -1, "rentLevel": 4, "activationTickCount": 0,
                "housekeepingClaimedFlag": 0, "vipFlag": false
            }))
            .unwrap();
            w.placed_objects.insert(format!("100,{y}"), record);
        }
        w.gate_flags.metro_station_floor_index = 9;
        w.rng_state = 54;
        let mut time = crate::time::create_time_state();
        time.day_tick = 0xf1;
        time.daypart_index = 3;
        tick_vip_special_visitor(&mut w, &time);
        assert_eq!(w.rng_call_count, 1);
        assert!(w.placed_objects.values().all(|o| o.unit_status == 2 && o.dirty_flag == 1));
        assert_eq!(
            serde_json::to_value(&w.pending_notifications).unwrap(),
            serde_json::json!([{ "kind": "event", "message": "metro_train_arrival" }])
        );
        // The next arrival roll that succeeds sends the train away again.
        tick_vip_special_visitor(&mut w, &time);
        assert_eq!(w.rng_call_count, 2);
    }
}

// ts: commands.ts

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

use crate::carriers::slot::floor_to_slot;
use crate::carriers::{make_carrier_car, rebuild_carrier_list};
use crate::families::entertainment::recompute_commercial_venue_buckets;
use crate::js::{self, JsObject};
use crate::ledger::{rebuild_facility_ledger, LedgerState};
use crate::queue::cancel::cancel_runtime_route_request;
use crate::reachability::rebuild_tables::{rebuild_route_reachability_tables, rebuild_transfer_group_cache};
use crate::reachability::special_link_records::rebuild_special_link_route_records;
use crate::resources::{
    CARRIER_CAR_CONSTRUCTION_COST, CARRIER_EXTEND_FLOOR_COST, CINEMA_CLASSIC_MOVIE_COST, CINEMA_NEW_MOVIE_COST,
    FAMILY_CATHEDRAL_BASE, FAMILY_CINEMA, FAMILY_CINEMA_LOWER, FAMILY_CINEMA_STAIRS_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER, FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE,
    FAMILY_HOTEL_TWIN, FAMILY_METRO_BOTTOM, FAMILY_METRO_MIDDLE, FAMILY_METRO_TOP, FAMILY_OFFICE, FAMILY_PARTY_HALL,
    FAMILY_PARTY_HALL_LOWER, FAMILY_RECYCLING_CENTER_LOWER, FAMILY_RECYCLING_CENTER_UPPER, FAMILY_RETAIL,
    FAMILY_SECURITY, LEGACY_TILE_ALIASES, LEGACY_VIP_TILE_TO_STANDARD, TILE_COSTS, TILE_TO_FAMILY_CODE,
    TILE_WIDTHS, UNDERGROUND_ALLOWED_TILES, VALID_TILE_TYPES,
};
use crate::sims::medical::invalidate_medical_slots_for_sidecar;
use crate::sims::parking::{rebuild_parking_coverage, rebuild_parking_demand_log};
use crate::sims::population::{
    cleanup_sims_for_removed_tile, clear_sim_route, rebuild_runtime_sims, sim_key,
};
use crate::world::{
    is_valid_lobby_y, sample_rng, y_to_floor, CommercialVenueRecord, EntertainmentLinkRecord, LobbyMode,
    MedicalCenterRecord, PlacedObjectRecord, ServiceRequestEntry, SidecarRecord, SimPrompt, SimPromptKind,
    WorldState, GRID_WIDTH, GROUND_Y, UNDERGROUND_Y, VENUE_DORMANT, VENUE_PARTIAL,
};

// ─── Patch type ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellPatch {
    pub x: i64,
    pub y: i64,
    pub tile_type: String,
    pub is_anchor: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_overlay: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_active_flag: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_status: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_level: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_score: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_flag: Option<i64>,
}

impl CellPatch {
    /// `{ x, y, tileType, isAnchor }`, the optional fields absent.
    pub fn new(x: i64, y: i64, tile_type: &str, is_anchor: bool) -> Self {
        CellPatch {
            x,
            y,
            tile_type: tile_type.to_string(),
            is_anchor,
            is_overlay: None,
            eval_active_flag: None,
            unit_status: None,
            eval_level: None,
            eval_score: None,
            coverage_flag: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<Vec<CellPatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub economy_changed: Option<bool>,
}

/// `{ accepted: false, reason }`.
fn rejected(reason: impl Into<String>) -> CommandResult {
    CommandResult {
        accepted: false,
        patch: None,
        reason: Some(reason.into()),
        economy_changed: None,
    }
}

/// `{ accepted: true, patch }`, with `economyChanged` where upstream writes it.
fn accepted(patch: Vec<CellPatch>, economy_changed: Option<bool>) -> CommandResult {
    CommandResult {
        accepted: true,
        patch: Some(patch),
        reason: None,
        economy_changed,
    }
}

/// `SimCommand`'s `set_cinema_movie_pool.pool` (upstream's inline
/// `"classic" | "new"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CinemaMoviePool {
    Classic,
    New,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimCommand {
    #[serde(rename_all = "camelCase")]
    PlaceTile { x: i64, y: i64, tile_type: String },
    RemoveTile { x: i64, y: i64 },
    #[serde(rename_all = "camelCase")]
    PromptResponse { prompt_id: String, accepted: bool },
    #[serde(rename_all = "camelCase")]
    SetRentLevel { x: i64, y: i64, rent_level: i64 },
    AddElevatorCar { x: i64, y: i64 },
    RemoveElevatorCar { x: i64, y: i64 },
    SetElevatorDwellDelay { x: i64, y: i64, value: i64 },
    SetElevatorWaitingCarResponse { x: i64, y: i64, value: i64 },
    #[serde(rename_all = "camelCase")]
    SetElevatorHomeFloor { x: i64, car_index: i64, floor: i64 },
    ToggleElevatorFloorStop { x: i64, floor: i64 },
    SetCinemaMoviePool { x: i64, y: i64, pool: CinemaMoviePool },
}

// ─── Infrastructure tiles (no PlacedObjectRecord) ─────────────────────────────

const INFRASTRUCTURE_TILES: &[&str] = &["floor", "stairs"];
const CATHEDRAL_ANCHOR_LOGICAL_FLOOR: i64 = 103;
const CATHEDRAL_SLICE_COUNT: i64 = 5;
const METRO_STACK_TILE_TYPE: &str = "metro";
const METRO_STACK_HEIGHT: i64 = 3;
const METRO_FAMILIES: [i64; 3] = [FAMILY_METRO_TOP, FAMILY_METRO_MIDDLE, FAMILY_METRO_BOTTOM];

// Families whose rentLevel initialises to 1; all others initialise to 4 (no payout).
const VARIANT_INIT_ONE_FAMILIES: &[i64] = &[3, 4, 5, 7, 9, FAMILY_RETAIL];

// ─── JavaScript lookups ───────────────────────────────────────────────────────

/// `TILE_WIDTHS[tileType] ?? d`.
fn tile_width(tile_type: &str, d: i64) -> i64 {
    js::record_get(TILE_WIDTHS, tile_type).copied().unwrap_or(d)
}

/// `TILE_COSTS[tileType] ?? d`.
fn tile_cost(tile_type: &str, d: i64) -> i64 {
    js::record_get(TILE_COSTS, tile_type).copied().unwrap_or(d)
}

/// `o[key]` as a condition: a missing key and an empty string are falsy.
fn truthy(o: &JsObject<String>, key: &str) -> bool {
    o.get(key).is_some_and(|s| !s.is_empty())
}

/// `Number(part)` of an "x,y" key; every key the simulation writes is two
/// integers.
fn key_xy(key: &str) -> (i64, i64) {
    let mut it = key.split(',').map(|s| s.parse::<i64>().unwrap_or_else(|_| panic!("key {key:?} is not \"x,y\"")));
    let x = it.next().unwrap();
    let y = it.next().unwrap_or_else(|| panic!("key {key:?} is not \"x,y\""));
    (x, y)
}

/// `world.sidecars[i]` (`None` for a negative or past-the-end index).
fn sidecar_at(world: &WorldState, i: i64) -> Option<&SidecarRecord> {
    if i < 0 {
        return None;
    }
    world.sidecars.get(i as usize)
}

/// A fresh record with the fields every placement path writes the same.
fn new_placed_object(
    left_tile_index: i64,
    right_tile_index: i64,
    object_type_code: i64,
    unit_status: i64,
    linked_record_index: i64,
    rent_level: i64,
    vip_flag: bool,
) -> PlacedObjectRecord {
    PlacedObjectRecord {
        left_tile_index,
        right_tile_index,
        object_type_code,
        unit_status,
        linked_record_index,
        aux_value_or_timer: 0,
        eval_level: 0xff,
        eval_score: -1,
        dirty_flag: 1,
        occupied_flag: 1,
        activation_tick_count: 0,
        rent_level,
        housekeeping_claimed_flag: 0,
        vip_flag: Some(vip_flag),
    }
}

// ─── PlacedObjectRecord helpers ───────────────────────────────────────────────

const HOTEL_INIT_FAMILIES: &[i64] = &[FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_HOTEL_SUITE];

/// `time` is upstream's `{ daypartIndex }`: here its one field.
fn make_placed_object(
    x: i64,
    y: i64,
    tile_type: &str,
    world: &mut WorldState,
    daypart_index: i64,
    vip_flag: bool,
) -> PlacedObjectRecord {
    let width = tile_width(tile_type, 1);
    let family_code = js::record_get(TILE_TO_FAMILY_CODE, tile_type).copied().unwrap_or(0);
    let sidecar_index = alloc_sidecar(tile_type, x, y, world);
    // Spec: hotel/condo start in vacant/unsold band (0x18 or 0x20 by half-day branch).
    // Office starts at 0x10 (unoccupied). Others start at 0.
    let mut unit_status = 0;
    if family_code == FAMILY_OFFICE {
        unit_status = 0x10;
    } else if HOTEL_INIT_FAMILIES.contains(&family_code) || family_code == FAMILY_CONDO {
        unit_status = if daypart_index < 4 { 0x18 } else { 0x20 };
    }
    // Binary placement (place_object_on_floor 1200:1ee3/1ef4,
    // place_mergeable_span_object_on_floor 1200:2e81/2e92,
    // expand_type21_object_layout 11f0:00fb/010c etc.) writes both +0x13
    // and +0x14 = 1 on fresh placement.
    new_placed_object(
        x,
        x + width - 1,
        family_code,
        unit_status,
        sidecar_index,
        if VARIANT_INIT_ONE_FAMILIES.contains(&family_code) { 1 } else { 4 },
        vip_flag,
    )
}

fn place_cathedral_stack(
    x: i64,
    y: i64,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
) -> CommandResult {
    let tile_type = "cathedral";
    let tile_width = tile_width(tile_type, 28);
    let cost = tile_cost(tile_type, 0);
    let logical_floor = GROUND_Y - y;

    if logical_floor != CATHEDRAL_ANCHOR_LOGICAL_FLOOR {
        return rejected("Cathedral must be placed on floor 103");
    }
    if world.gate_flags.eval_sim_index >= 0 && world.gate_flags.eval_sim_index != 0xffff {
        return rejected("Cathedral already placed");
    }
    if x + tile_width - 1 >= world.width {
        return rejected("Out of bounds");
    }
    if !free_build && cost > ledger.cash_balance {
        return rejected("Insufficient funds");
    }

    let rows: Vec<i64> = (0..CATHEDRAL_SLICE_COUNT).map(|i| y + i).collect();
    for &row_y in &rows {
        if row_y < 0 || row_y >= world.height {
            return rejected("Out of bounds");
        }
        for dx in 0..tile_width {
            let key = format!("{},{}", x + dx, row_y);
            if truthy(&world.cell_to_anchor, &key) {
                return rejected("Cell already occupied");
            }
            if let Some(existing) = world.cells.get(&key) {
                if !existing.is_empty() && existing != "floor" {
                    return rejected("Cell already occupied");
                }
            }
        }
    }

    let bottom_y = y + CATHEDRAL_SLICE_COUNT - 1;
    let support_y = bottom_y + 1;
    for dx in 0..tile_width {
        if support_y < 0 || support_y >= world.height || !truthy(&world.cells, &format!("{},{}", x + dx, support_y)) {
            return rejected("No support");
        }
    }

    for &row_y in &rows {
        for dx in 0..tile_width {
            world.cells.remove(&format!("{},{}", x + dx, row_y));
        }
    }

    let visual_anchor_key = format!("{x},{y}");
    let mut patch: Vec<CellPatch> = Vec::new();
    for slice in 0..CATHEDRAL_SLICE_COUNT {
        let row_y = bottom_y - slice;
        let family_code = FAMILY_CATHEDRAL_BASE + slice;
        let object_key = format!("{x},{row_y}");
        for dx in 0..tile_width {
            let key = format!("{},{}", x + dx, row_y);
            world.cells.insert(key.clone(), tile_type.to_string());
            if key != visual_anchor_key {
                world.cell_to_anchor.insert(key.clone(), visual_anchor_key.clone());
            }
            patch.push(CellPatch::new(x + dx, row_y, tile_type, key == visual_anchor_key));
        }
        world
            .placed_objects
            .insert(object_key, new_placed_object(x, x + tile_width - 1, family_code, 0, -1, 4, false));
    }

    world.gate_flags.eval_sim_index = y_to_floor(bottom_y);
    if !free_build {
        ledger.cash_balance -= cost;
    }

    for &row_y in &rows {
        fill_row_gaps(row_y, world, &mut patch);
    }
    run_global_rebuilds(world, ledger);

    accepted(patch, Some(cost > 0))
}

fn place_metro_station_stack(
    x: i64,
    y: i64,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
    daypart_index: i64,
) -> CommandResult {
    let tile_width = tile_width("metro", 30);
    let cost = tile_cost("metro", 0);
    if world.gate_flags.metro_station_floor_index >= 0 {
        return rejected("Metro station already placed");
    }
    if y < UNDERGROUND_Y {
        return rejected("Metro station may only be placed underground");
    }
    if y + METRO_STACK_HEIGHT - 1 >= world.height || x + tile_width - 1 >= world.width {
        return rejected("Out of bounds");
    }

    let mut stack_cells: IndexSet<String> = IndexSet::new();
    for row_offset in 0..METRO_STACK_HEIGHT {
        let row_y = y + row_offset;
        for dx in 0..tile_width {
            stack_cells.insert(format!("{},{}", x + dx, row_y));
        }
    }

    let mut floor_to_remove: Vec<String> = Vec::new();
    for key in &stack_cells {
        if truthy(&world.cell_to_anchor, key) {
            return rejected("Cell already occupied");
        }
        let Some(existing) = world.cells.get(key).filter(|s| !s.is_empty()) else {
            continue;
        };
        if existing == "floor" {
            floor_to_remove.push(key.clone());
        } else {
            return rejected("Cell already occupied");
        }
    }

    for row_offset in 0..METRO_STACK_HEIGHT {
        let row_y = y + row_offset;
        for dx in 0..tile_width {
            let support_y = row_y - 1;
            let support_key = format!("{},{}", x + dx, support_y);
            if support_y < 0 || (!truthy(&world.cells, &support_key) && !stack_cells.contains(&support_key)) {
                return rejected("No support");
            }
        }
    }

    let initial_status = if daypart_index < 4 { 0 } else { 1 };
    let should_charge = !free_build;
    if should_charge && cost > ledger.cash_balance {
        return rejected("Insufficient funds");
    }

    for key in &floor_to_remove {
        world.cells.remove(key);
    }

    let visual_anchor_key = format!("{x},{y}");
    let mut patch: Vec<CellPatch> = Vec::new();
    for row_offset in 0..METRO_STACK_HEIGHT {
        let row_y = y + row_offset;
        let object_key = format!("{x},{row_y}");
        for dx in 0..tile_width {
            let key = format!("{},{}", x + dx, row_y);
            world.cells.insert(key.clone(), METRO_STACK_TILE_TYPE.to_string());
            if key != visual_anchor_key {
                world.cell_to_anchor.insert(key.clone(), visual_anchor_key.clone());
            }
            patch.push(CellPatch::new(x + dx, row_y, METRO_STACK_TILE_TYPE, key == visual_anchor_key));
        }
        world.detach_object(&object_key);
        world.placed_objects.insert(
            object_key,
            new_placed_object(
                x,
                x + tile_width - 1,
                METRO_FAMILIES[row_offset as usize],
                initial_status,
                -1,
                4,
                false,
            ),
        );
    }

    if should_charge {
        ledger.cash_balance -= cost;
    }
    world.gate_flags.metro_placed = 1;
    world.gate_flags.metro_station_floor_index = y_to_floor(y);
    for row_offset in 0..METRO_STACK_HEIGHT {
        fill_row_gaps(y + row_offset, world, &mut patch);
    }
    run_global_rebuilds(world, ledger);

    accepted(patch, Some(should_charge && cost > 0))
}

/// Allocate a sidecar for tiles that need one. Returns index or −1.
fn alloc_sidecar(tile_type: &str, x: i64, y: i64, world: &mut WorldState) -> i64 {
    let mut record: Option<SidecarRecord> = None;

    if tile_type == "restaurant" || tile_type == "fastFood" || tile_type == "retail" {
        let r = CommercialVenueRecord {
            owner_subtype_index: x,
            capacity: 48,
            visit_count: 0,
            // Binary `allocate_facility_record` (11b0:07d3) link-disabled branch
            // seeds `record[+7] = 10`. For restaurant (family 6) this branch is
            // always taken; for fast-food/retail it's taken when (daypart != 0
            // || dayTick <= 240) — i.e. game-startup placement. The rebuild
            // at tick 240 (fastfood/retail) / tick 1600 (restaurant) rolls
            // this into yesterdayVisitCount and pushes +10 into the bucket.
            today_visit_count: 10,
            yesterday_visit_count: 0,
            acquire_count: 0,
            // Retail starts dormant (unrented); restaurant/fast-food start active
            availability_state: if tile_type == "retail" { VENUE_DORMANT } else { VENUE_PARTIAL },
            current_population: 0,
            last_acquire_tick: 0,
            eligibility_threshold: 0,
            remaining_capacity: 0,
            phase_a_seed: 10,
            phase_b_seed: 10,
            override_seed: 10,
        };
        record = Some(SidecarRecord::CommercialVenue(r));
    } else if tile_type == "recyclingCenterUpper" || tile_type == "recyclingCenterLower" || tile_type == "parking" {
        let r = ServiceRequestEntry {
            owner_subtype_index: x,
            floor_index: if tile_type == "parking" { Some(y_to_floor(y)) } else { None },
            coverage_flag: Some(0),
        };
        record = Some(SidecarRecord::ServiceRequest(r));
    } else if tile_type == "medical" {
        let r = MedicalCenterRecord {
            owner_subtype_index: x,
            pending_visitors_count: 0,
        };
        record = Some(SidecarRecord::MedicalCenter(r));
    }
    // Cinema and party hall allocate their sidecar in placeEntertainmentVenue
    // (they share one sidecar across 4 / 2 sub-records).

    let Some(record) = record else { return -1 };
    world.sidecars.push(record);
    world.sidecars.len() as i64 - 1
}

/// Mark a sidecar as invalid (demolished).
fn free_sidecar(index: i64, world: &mut WorldState) {
    if index < 0 {
        return;
    }
    if let Some(rec) = world.sidecars.get_mut(index as usize) {
        match rec {
            SidecarRecord::CommercialVenue(r) => r.owner_subtype_index = 0xff,
            SidecarRecord::ServiceRequest(r) => r.owner_subtype_index = 0xff,
            SidecarRecord::EntertainmentLink(r) => r.owner_subtype_index = 0xff,
            SidecarRecord::MedicalCenter(r) => r.owner_subtype_index = 0xff,
        }
    }
}

fn get_overlay_anchor_key(world: &WorldState, x: i64, y: i64) -> Option<String> {
    let key = format!("{x},{y}");
    if let Some(anchor) = world.overlay_to_anchor.get(&key) {
        return Some(anchor.clone());
    }
    if truthy(&world.overlays, &key) {
        Some(key)
    } else {
        None
    }
}

/// Upstream's `"standard" | "express" | "service" | null`.
fn elevator_mode_for_overlay(t: &str) -> Option<&'static str> {
    match t {
        "elevator" => Some("standard"),
        "elevatorExpress" => Some("express"),
        "elevatorService" => Some("service"),
        _ => None,
    }
}

/// Stairs may not horizontally overlap each other unless they share the
/// exact same anchor column (i.e. they're stacked floor-to-floor). Returns
/// true if any existing stair overlay shares column range with the proposed
/// placement at a different x.
fn has_overlapping_misaligned_stairs(world: &WorldState, x: i64, y: i64, width: i64) -> bool {
    let new_right = x + width - 1;
    let other_width = tile_width("stairs", width);
    for (key, t) in world.overlays.iter() {
        if t != "stairs" {
            continue;
        }
        let (ox, oy) = key_xy(key);
        if ox == x && oy == y {
            continue;
        }
        if ox == x {
            continue;
        }
        let other_right = ox + other_width - 1;
        if other_right < x || ox > new_right {
            continue;
        }
        return true;
    }
    false
}

fn has_misaligned_adjacent_overlay(world: &WorldState, x: i64, y: i64, t: &str, width: i64) -> bool {
    for adjacent_y in [y - 1, y + 1] {
        if adjacent_y < 0 || adjacent_y >= world.height {
            continue;
        }
        let mut adjacent_anchors: IndexSet<String> = IndexSet::new();
        for dx in 0..width {
            let Some(anchor_key) = get_overlay_anchor_key(world, x + dx, adjacent_y) else {
                continue;
            };
            if anchor_key.is_empty() {
                continue;
            }
            if world.overlays.get(&anchor_key).map(String::as_str) != Some(t) {
                continue;
            }
            adjacent_anchors.insert(anchor_key);
        }
        if adjacent_anchors.is_empty() {
            continue;
        }
        if adjacent_anchors.len() > 1 {
            return true;
        }
        let anchor_key = &adjacent_anchors[0];
        if anchor_key.is_empty() {
            continue;
        }
        let (anchor_x, _) = key_xy(anchor_key);
        if anchor_x != x {
            return true;
        }
    }
    false
}

/// Minimum empty-tile gap a shaft requires on each side, edge-to-edge.
/// Express shafts want 8 clear tiles; standard and service want 4. When
/// two shafts are adjacent, the required gap is `max` of the two.
const ELEVATOR_STANDARD_MIN_GAP: i64 = 4;
const ELEVATOR_EXPRESS_MIN_GAP: i64 = 8;

/// Max contiguous floors a standard or service shaft may span. Express unlimited.
const ELEVATOR_NONEXPRESS_MAX_FLOORS: i64 = 31;
/// Max total elevator shafts per tower.
const ELEVATOR_MAX_SHAFTS: i64 = 24;

fn shaft_min_gap(mode: i64) -> i64 {
    if mode == 0 {
        ELEVATOR_EXPRESS_MIN_GAP
    } else {
        ELEVATOR_STANDARD_MIN_GAP
    }
}

/// Resolve a carrier whose served-floor range covers `floor` at column `x`.
/// Returns null when nothing is served there — disconnected segments at the
/// same column are independent carriers.
///
/// (Returns the carrier's index in `world.carriers`.)
fn find_carrier_by_column_and_floor(world: &WorldState, x: i64, floor: i64) -> Option<usize> {
    world
        .carriers
        .iter()
        .position(|c| c.column == x && floor >= c.bottom_served_floor && floor <= c.top_served_floor)
}

/// Resolve a carrier from a click coordinate. Two carriers can share a column
/// if they are vertically separated, so the floor at `y` is required to pick
/// the right one — falls back to the closest segment when the click landed
/// outside any served range.
///
/// (Returns the carrier's index in `world.carriers`.)
fn find_carrier_at_click(world: &WorldState, x: i64, y: i64) -> Option<usize> {
    let floor = y_to_floor(y);
    let direct = find_carrier_by_column_and_floor(world, x, floor);
    if direct.is_some() {
        return direct;
    }
    let mut best: Option<usize> = None;
    // `Infinity`: every distance is a finite integer below it.
    let mut best_distance = i64::MAX;
    for (i, c) in world.carriers.iter().enumerate() {
        if c.column != x {
            continue;
        }
        let distance = if floor < c.bottom_served_floor {
            c.bottom_served_floor - floor
        } else {
            floor - c.top_served_floor
        };
        if distance < best_distance {
            best = Some(i);
            best_distance = distance;
        }
    }
    best
}

fn overlay_type_to_mode(t: &str) -> i64 {
    if t == "elevatorExpress" {
        return 0;
    }
    if t == "elevatorService" {
        return 2;
    }
    1
}

fn carrier_overlay_width(mode: i64) -> i64 {
    if mode == 0 {
        return tile_width("elevatorExpress", 6);
    }
    if mode == 2 {
        return tile_width("elevatorService", 4);
    }
    tile_width("elevator", 4)
}

fn too_close_to_existing_shaft(world: &WorldState, x: i64, width: i64, new_mode: i64) -> bool {
    let new_right = x + width - 1;
    for carrier in &world.carriers {
        if carrier.column == x {
            continue;
        }
        let other_width = carrier_overlay_width(carrier.carrier_mode);
        let other_right = carrier.column + other_width - 1;
        let gap = if carrier.column > new_right {
            carrier.column - new_right - 1
        } else if x > other_right {
            x - other_right - 1
        } else {
            -1
        };
        let required_gap = shaft_min_gap(new_mode).max(shaft_min_gap(carrier.carrier_mode));
        if gap < required_gap {
            return true;
        }
    }
    false
}

fn has_adjacent_elevator_mode_conflict(world: &WorldState, x: i64, y: i64, t: &str, width: i64) -> bool {
    let Some(mode) = elevator_mode_for_overlay(t) else {
        return false;
    };

    for adjacent_y in [y - 1, y + 1] {
        if adjacent_y < 0 || adjacent_y >= world.height {
            continue;
        }
        for dx in 0..width {
            let Some(anchor_key) = get_overlay_anchor_key(world, x + dx, adjacent_y) else {
                continue;
            };
            if anchor_key.is_empty() {
                continue;
            }
            let adjacent_mode = world
                .overlays
                .get(&anchor_key)
                .filter(|s| !s.is_empty())
                .and_then(|adjacent_type| elevator_mode_for_overlay(adjacent_type));
            let Some(adjacent_mode) = adjacent_mode else {
                continue;
            };
            if adjacent_mode != mode {
                return true;
            }
        }
    }

    false
}

// ─── Global rebuilds ──────────────────────────────────────────────────────────

/// Run all post-build / post-demolish global rebuilds.
/// Order matters: carriers → special_links → walkability → transfer_cache.
///
/// Append a CellPatch for every parking-space anchor with its current
/// coverageFlag. Used after place/remove of a parkingRamp or parking tile so
/// the client can refresh distant parking visuals whose coverage changed.
// BOX: car elevator -- pub(crate) (upstream's is module-private).
pub(crate) fn append_parking_coverage_patches(world: &WorldState, patch: &mut Vec<CellPatch>) {
    let parking = js::record_get(TILE_TO_FAMILY_CODE, "parking").copied();
    for (key, obj) in world.placed_objects.iter() {
        if Some(obj.object_type_code) != parking {
            continue;
        }
        let (x, y) = key_xy(key);
        let Some(SidecarRecord::ServiceRequest(sidecar)) = sidecar_at(world, obj.linked_record_index) else {
            continue;
        };
        let mut p = CellPatch::new(x, y, "parking", true);
        p.coverage_flag = Some(sidecar.coverage_flag.unwrap_or(0));
        patch.push(p);
    }
}

pub fn run_global_rebuilds(world: &mut WorldState, ledger: &mut LedgerState) {
    world.gate_flags.office_placed = 0;
    world.gate_flags.metro_placed = 0;
    world.gate_flags.metro_station_floor_index = -1;
    world.gate_flags.security_placed = 0;
    world.gate_flags.vip_suite_floor = 0xffff;
    world.gate_flags.recycling_center_count = 0;
    for (key, object) in world.placed_objects.iter() {
        let flags = &mut world.gate_flags;
        if object.object_type_code == FAMILY_OFFICE {
            flags.office_placed = 1;
        }
        if object.object_type_code == FAMILY_METRO_TOP {
            flags.metro_placed = 1;
            let (_, y) = key_xy(key);
            flags.metro_station_floor_index = y_to_floor(y);
        }
        if object.object_type_code == FAMILY_SECURITY {
            flags.security_placed = 1;
        }
        if object.object_type_code == FAMILY_RECYCLING_CENTER_UPPER {
            flags.recycling_center_count += 1;
        }
    }

    rebuild_facility_ledger(ledger, world);
    rebuild_runtime_sims(world);
    rebuild_parking_coverage(world);
    rebuild_parking_demand_log(world);
    rebuild_carrier_list(world);
    rebuild_special_link_route_records(world);
    rebuild_route_reachability_tables(world);
    rebuild_transfer_group_cache(world);
    recompute_commercial_venue_buckets(world);
}

fn has_recycling_stack_overlap(world: &WorldState, proposed_floor: i64) -> bool {
    let mut has_existing = false;
    let mut overlaps = false;
    for (key, obj) in world.placed_objects.iter() {
        if obj.object_type_code != FAMILY_RECYCLING_CENTER_UPPER && obj.object_type_code != FAMILY_RECYCLING_CENTER_LOWER
        {
            continue;
        }
        has_existing = true;
        let (_, oy) = key_xy(key);
        let existing_floor = y_to_floor(oy);
        if proposed_floor >= existing_floor - 2 && proposed_floor <= existing_floor + 1 {
            overlaps = true;
            break;
        }
    }
    !has_existing || overlaps
}

fn place_recycling_center_stack(
    x: i64,
    y: i64,
    normalized_tile_type: &str,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
    daypart_index: i64,
) -> CommandResult {
    let tile_width = tile_width("recyclingCenterUpper", 2);
    let upper_y = if normalized_tile_type == "recyclingCenterUpper" { y } else { y - 1 };
    let lower_y = upper_y + 1;
    // `TILE_COSTS.recyclingCenter`, which the table always has.
    let cost = js::record_get(TILE_COSTS, "recyclingCenter").copied().expect("TILE_COSTS.recyclingCenter");

    if upper_y < 0 || lower_y >= world.height || x + tile_width - 1 >= world.width {
        return rejected("Out of bounds");
    }
    if !free_build && cost > ledger.cash_balance {
        return rejected("Insufficient funds");
    }
    if !has_recycling_stack_overlap(world, y_to_floor(upper_y)) {
        return rejected("Recycling center must be placed near an existing recycling-center stack");
    }

    let mut stack_cells: IndexSet<String> = IndexSet::new();
    for row_y in [upper_y, lower_y] {
        for dx in 0..tile_width {
            stack_cells.insert(format!("{},{}", x + dx, row_y));
        }
    }

    let mut floor_to_remove: Vec<String> = Vec::new();
    for key in &stack_cells {
        if truthy(&world.cell_to_anchor, key) {
            return rejected("Cell already occupied");
        }
        if let Some(existing) = world.cells.get(key).filter(|s| !s.is_empty()) {
            if existing == "floor" {
                floor_to_remove.push(key.clone());
            } else {
                return rejected("Cell already occupied");
            }
        }
    }

    for row_y in [upper_y, lower_y] {
        let support_y = if row_y >= UNDERGROUND_Y { row_y - 1 } else { row_y + 1 };
        for dx in 0..tile_width {
            let support_key = format!("{},{}", x + dx, support_y);
            if support_y < 0
                || support_y >= world.height
                || (!truthy(&world.cells, &support_key) && !stack_cells.contains(&support_key))
            {
                return rejected("No support");
            }
        }
    }

    for key in &floor_to_remove {
        world.cells.remove(key);
    }
    for (row_y, tile_type) in [(upper_y, "recyclingCenterUpper"), (lower_y, "recyclingCenterLower")] {
        world.cells.insert(format!("{x},{row_y}"), tile_type.to_string());
        for dx in 1..tile_width {
            world.cells.insert(format!("{},{}", x + dx, row_y), tile_type.to_string());
            world.cell_to_anchor.insert(format!("{},{}", x + dx, row_y), format!("{x},{row_y}"));
        }
        let record = make_placed_object(x, row_y, tile_type, world, daypart_index, false);
        world.detach_object(&format!("{x},{row_y}"));
        world.placed_objects.insert(format!("{x},{row_y}"), record);
    }
    if !free_build {
        ledger.cash_balance -= cost;
    }

    let mut patch: Vec<CellPatch> = Vec::new();
    for (row_y, tile_type) in [(upper_y, "recyclingCenterUpper"), (lower_y, "recyclingCenterLower")] {
        let record = world.placed_objects.get(&format!("{x},{row_y}"));
        for dx in 0..tile_width {
            let mut p = CellPatch::new(x + dx, row_y, tile_type, dx == 0);
            if dx == 0 {
                if let Some(record) = record {
                    // evalActiveFlag is the user-facing "scored/operational" bit:
                    // binary +0x14 (occupied).
                    p.eval_active_flag = Some(record.occupied_flag);
                    p.unit_status = Some(record.unit_status);
                }
            }
            patch.push(p);
        }
    }

    fill_row_gaps(upper_y, world, &mut patch);
    fill_row_gaps(lower_y, world, &mut patch);
    run_global_rebuilds(world, ledger);

    accepted(patch, Some(cost > 0))
}

/// Sub-record layout for a cinema or party-hall placement. The binary splits
/// cinema placements through `split_entertainment_object_into_link_pair`
/// (1188:0352) into a stairway sub-object and a theater sub-object per floor,
/// yielding 4 PlacedObjectRecords per cinema. Party halls are not split —
/// they keep a single sub-object per floor (2 total).
///
/// All sub-records for a single placement share one EntertainmentLinkRecord
/// sidecar (allocated by `register_entertainment_upper_half` @ 1188:01ad on
/// first upper-half dispatch; theater sub-records inherit the link index via
/// the adjacent-record read in `recompute_object_runtime_links_by_type`).
struct EntertainmentSubRecord {
    offset_x: i64,
    row_y: i64,
    width: i64,
    family_code: i64,
}

fn cinema_sub_records(x: i64, upper_y: i64, lower_y: i64) -> Vec<EntertainmentSubRecord> {
    [
        (0, upper_y, 7, FAMILY_CINEMA_STAIRS_UPPER),
        (7, upper_y, 24, FAMILY_CINEMA_LOWER),
        (0, lower_y, 7, FAMILY_CINEMA_STAIRS_LOWER),
        (7, lower_y, 24, FAMILY_CINEMA),
    ]
    .into_iter()
    .map(|(offset_x, row_y, width, family_code)| EntertainmentSubRecord {
        offset_x: x + offset_x,
        row_y,
        width,
        family_code,
    })
    .collect()
}

fn party_hall_sub_records(x: i64, upper_y: i64, lower_y: i64) -> Vec<EntertainmentSubRecord> {
    vec![
        EntertainmentSubRecord {
            offset_x: x,
            row_y: upper_y,
            width: 27,
            family_code: FAMILY_PARTY_HALL,
        },
        EntertainmentSubRecord {
            offset_x: x,
            row_y: lower_y,
            width: 27,
            family_code: FAMILY_PARTY_HALL_LOWER,
        },
    ]
}

fn sample_cinema_initial_selector(world: &mut WorldState) -> i64 {
    // Binary placement reaches `allocate_entertainment_link_record` after the
    // deferred object rebuild path has consumed 13 placement-time RNG samples.
    for _ in 0..13 {
        sample_rng(world);
    }
    sample_rng(world) % 14
}

/// `tile_type` is upstream's `"cinema" | "partyHall"`.
fn place_entertainment_venue(
    x: i64,
    y: i64,
    tile_type: &str,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
    _daypart_index: i64,
) -> CommandResult {
    let tile_width = tile_width(tile_type, 1);
    let cost = tile_cost(tile_type, 0);
    let upper_y = y - 1;
    let lower_y = y;

    if upper_y < 0 || lower_y >= world.height || x + tile_width - 1 >= world.width {
        return rejected("Out of bounds");
    }
    if !free_build && cost > ledger.cash_balance {
        return rejected("Insufficient funds");
    }

    let mut structure_cells: IndexSet<String> = IndexSet::new();
    for row_y in [upper_y, lower_y] {
        for dx in 0..tile_width {
            structure_cells.insert(format!("{},{}", x + dx, row_y));
        }
    }

    let mut floor_to_remove: Vec<String> = Vec::new();
    for key in &structure_cells {
        if truthy(&world.cell_to_anchor, key) {
            return rejected("Cell already occupied");
        }
        if let Some(existing) = world.cells.get(key).filter(|s| !s.is_empty()) {
            if existing == "floor" {
                floor_to_remove.push(key.clone());
            } else {
                return rejected("Cell already occupied");
            }
        }
    }

    let support_y = if lower_y >= UNDERGROUND_Y { lower_y - 1 } else { lower_y + 1 };
    if support_y < 0 || support_y >= world.height {
        return rejected("No support");
    }
    for dx in 0..tile_width {
        let support_key = format!("{},{}", x + dx, support_y);
        if !truthy(&world.cells, &support_key) && !structure_cells.contains(&support_key) {
            return rejected("No support");
        }
    }

    // Binary `allocate_entertainment_link_record` @ 1188:0073: `venue_selector`
    // (offset 7) is `rand() % 14` iff the registering sub-type is 0x22/0x23
    // (cinema stairway). Party hall (and any other caller) gets 0xff.
    let upper_half_floor = y_to_floor(lower_y);
    let lower_half_floor = upper_half_floor - 1;
    let sidecar = EntertainmentLinkRecord {
        owner_subtype_index: x,
        paired_subtype_index: 0xff,
        family_selector_or_single_link_flag: if tile_type == "cinema" {
            sample_cinema_initial_selector(world)
        } else {
            0xff
        },
        link_age_counter: 0,
        upper_budget: 0,
        lower_budget: 0,
        link_phase_state: 0,
        pending_transition_flag: 0,
        attendance_counter: 0,
        active_runtime_count: 0,
        lower_half_floor,
        upper_half_floor,
    };
    world.sidecars.push(SidecarRecord::EntertainmentLink(sidecar));
    let sidecar_index = world.sidecars.len() as i64 - 1;

    for key in &floor_to_remove {
        world.cells.remove(key);
    }

    // Lay down cells with the facility's visual tile string. All cells route
    // to the upper-left visual anchor for rendering/removal lookups.
    let visual_anchor_key = format!("{x},{upper_y}");
    for row_y in [upper_y, lower_y] {
        for dx in 0..tile_width {
            let key = format!("{},{}", x + dx, row_y);
            world.cells.insert(key.clone(), tile_type.to_string());
            if key != visual_anchor_key {
                world.cell_to_anchor.insert(key, visual_anchor_key.clone());
            }
        }
    }

    // Emit one PlacedObjectRecord per binary sub-record, all sharing the
    // single sidecar. Keys use each sub-record's top-left position.
    let subs = if tile_type == "cinema" {
        cinema_sub_records(x, upper_y, lower_y)
    } else {
        party_hall_sub_records(x, upper_y, lower_y)
    };
    for sub in subs {
        let key = format!("{},{}", sub.offset_x, sub.row_y);
        // split_entertainment_object_into_stairway_pair (1188:03b9/03ca,
        // 054f/0560) sets both flags on placement.
        world.detach_object(&key);
        world.placed_objects.insert(
            key,
            new_placed_object(
                sub.offset_x,
                sub.offset_x + sub.width - 1,
                sub.family_code,
                0,
                sidecar_index,
                4,
                false,
            ),
        );
    }

    if !free_build {
        ledger.cash_balance -= cost;
    }

    let anchor_record = world.placed_objects.get(&visual_anchor_key);
    let mut patch: Vec<CellPatch> = Vec::new();
    for row_y in [upper_y, lower_y] {
        for dx in 0..tile_width {
            let is_anchor = dx == 0 && row_y == upper_y;
            let mut p = CellPatch::new(x + dx, row_y, tile_type, is_anchor);
            if is_anchor {
                if let Some(anchor_record) = anchor_record {
                    p.eval_active_flag = Some(anchor_record.occupied_flag);
                    p.unit_status = Some(anchor_record.unit_status);
                }
            }
            patch.push(p);
        }
    }

    fill_row_gaps(upper_y, world, &mut patch);
    fill_row_gaps(lower_y, world, &mut patch);
    run_global_rebuilds(world, ledger);

    accepted(patch, Some(cost > 0))
}

// ─── Place tile ───────────────────────────────────────────────────────────────

/// `free_build` is upstream's `freeBuild = false`; `daypart_index` its
/// `time: { daypartIndex } = { daypartIndex: 0 }`.
pub fn handle_place_tile(
    x: i64,
    y: i64,
    tile_type: &str,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: Option<bool>,
    daypart_index: Option<i64>,
) -> CommandResult {
    let free_build = free_build.unwrap_or(false);
    let daypart_index = daypart_index.unwrap_or(0);
    let vip_standard = js::record_get(LEGACY_VIP_TILE_TO_STANDARD, tile_type).copied();
    let normalized_tile_type: &str = js::record_get(LEGACY_TILE_ALIASES, vip_standard.unwrap_or(tile_type))
        .copied()
        .or(vip_standard)
        .unwrap_or(tile_type);
    let vip_flag = vip_standard.is_some();

    // BOX: car elevator -- builds a car elevator, or refuses an overlay laid
    // over one; nothing at all for other tiles while none is built.
    if let Some(result) =
        crate::box_ext::car_elevator::intercept_place(x, y, normalized_tile_type, world, ledger, free_build)
    {
        return result;
    }
    // BOX: amenities -- builds an amenity; nothing for other tiles.
    if let Some(result) =
        crate::box_ext::amenities::intercept_place(x, y, normalized_tile_type, world, ledger, free_build)
    {
        return result;
    }

    if !VALID_TILE_TYPES.contains(&normalized_tile_type) {
        return rejected("Invalid tile type");
    }
    if x < 0 || x >= world.width || y < 0 || y >= world.height {
        return rejected("Out of bounds");
    }
    if y >= UNDERGROUND_Y && !UNDERGROUND_ALLOWED_TILES.contains(&normalized_tile_type) {
        return rejected("This tile cannot be placed underground");
    }

    if normalized_tile_type == METRO_STACK_TILE_TYPE {
        return place_metro_station_stack(x, y, world, ledger, free_build, daypart_index);
    }

    let metro_floor = world.gate_flags.metro_station_floor_index;
    let target_floor = y_to_floor(y);
    if metro_floor >= 0
        && (normalized_tile_type == "cinema"
            || normalized_tile_type == "partyHall"
            || normalized_tile_type == "recyclingCenter"
            || normalized_tile_type == "recyclingCenterUpper")
        && target_floor < metro_floor
    {
        return rejected("This facility cannot be placed below the metro station");
    }

    if normalized_tile_type == "cinema" || normalized_tile_type == "partyHall" {
        return place_entertainment_venue(x, y, normalized_tile_type, world, ledger, free_build, daypart_index);
    }

    if normalized_tile_type == "cathedral" {
        return place_cathedral_stack(x, y, world, ledger, free_build);
    }

    if normalized_tile_type == "recyclingCenter"
        || normalized_tile_type == "recyclingCenterUpper"
        || normalized_tile_type == "recyclingCenterLower"
    {
        return place_recycling_center_stack(x, y, normalized_tile_type, world, ledger, free_build, daypart_index);
    }

    // ── Elevator: overlay on a floor/lobby tile ─────────────────────────────────
    if normalized_tile_type == "elevator"
        || normalized_tile_type == "elevatorExpress"
        || normalized_tile_type == "elevatorService"
    {
        let mut patch: Vec<CellPatch> = Vec::new();
        let overlay_width = tile_width(normalized_tile_type, 1);
        if world.gate_flags.metro_station_floor_index >= 0
            && y_to_floor(y) < world.gate_flags.metro_station_floor_index - 1
        {
            return rejected("Elevator cannot extend below the metro station");
        }
        if x + overlay_width - 1 >= world.width {
            return rejected("Out of bounds");
        }
        for dx in 0..overlay_width {
            let key = format!("{},{}", x + dx, y);
            if truthy(&world.overlays, &key) || truthy(&world.overlay_to_anchor, &key) {
                return rejected("Cell already has an overlay");
            }
        }
        // Shaft creation charges the base tile cost on the first segment
        // at a column. Later floors extending an existing shaft charge
        // a per-floor extension cost (binary: `charge_floor_range_construction_cost`
        // at 1180:02e5, called from `FUN_10a8_0819`/`FUN_10a8_0b87`).
        // A placement only counts as an extension when it is vertically
        // adjacent to (or inside) an existing same-column shaft — disconnected
        // segments at the same column form independent carriers.
        let placed_floor = y_to_floor(y);
        let is_new_shaft = !world.carriers.iter().any(|c| {
            c.column == x && placed_floor >= c.bottom_served_floor - 1 && placed_floor <= c.top_served_floor + 1
        });
        let shaft_mode = overlay_type_to_mode(normalized_tile_type);
        let shaft_cost = if is_new_shaft {
            tile_cost(normalized_tile_type, 0)
        } else {
            js::record_get(CARRIER_EXTEND_FLOOR_COST, shaft_mode).copied().unwrap_or(0)
        };
        if !free_build && shaft_cost > ledger.cash_balance {
            return rejected("Insufficient funds");
        }
        if is_new_shaft && world.carriers.len() as i64 >= ELEVATOR_MAX_SHAFTS {
            return rejected(format!("Maximum {ELEVATOR_MAX_SHAFTS} elevator shafts per tower"));
        }
        if normalized_tile_type != "elevatorExpress" {
            let mut top_y = y;
            let mut bottom_y = y;
            while top_y > 0
                && world.overlays.get(&format!("{},{}", x, top_y - 1)).map(String::as_str) == Some(normalized_tile_type)
            {
                top_y -= 1;
            }
            while bottom_y < world.height - 1
                && world.overlays.get(&format!("{},{}", x, bottom_y + 1)).map(String::as_str)
                    == Some(normalized_tile_type)
            {
                bottom_y += 1;
            }
            if bottom_y - top_y + 1 > ELEVATOR_NONEXPRESS_MAX_FLOORS {
                let kind = if normalized_tile_type == "elevatorService" { "Service" } else { "Standard" };
                return rejected(format!(
                    "{kind} shaft may span at most {ELEVATOR_NONEXPRESS_MAX_FLOORS} floors"
                ));
            }
        }
        if is_new_shaft && too_close_to_existing_shaft(world, x, overlay_width, shaft_mode) {
            return rejected("Elevator too close to an adjacent shaft");
        }
        if (normalized_tile_type == "elevator"
            || normalized_tile_type == "elevatorExpress"
            || normalized_tile_type == "elevatorService")
            && has_misaligned_adjacent_overlay(world, x, y, "elevator", overlay_width)
        {
            return rejected("Elevator must align with adjacent shaft segments");
        }
        if has_adjacent_elevator_mode_conflict(world, x, y, normalized_tile_type, overlay_width) {
            return rejected("Elevator shaft mode must match adjacent segments");
        }
        // Auto-place floor tiles where empty but supported
        // (below for above-ground; above for underground floors).
        //
        // (As upstream: floors laid for the first columns stay laid when a
        // later column has no support and the placement is refused.)
        for dx in 0..overlay_width {
            let key = format!("{},{}", x + dx, y);
            if truthy(&world.cells, &key) || truthy(&world.cell_to_anchor, &key) {
                continue;
            }
            let support_y = if y >= UNDERGROUND_Y { y - 1 } else { y + 1 };
            let support_key = format!("{},{}", x + dx, support_y);
            if support_y < 0 || support_y >= world.height || !truthy(&world.cells, &support_key) {
                return rejected("Elevator requires a base tile or support");
            }
            world.cells.insert(key, "floor".to_string());
            patch.push(CellPatch::new(x + dx, y, "floor", true));
        }
        world.overlays.insert(format!("{x},{y}"), normalized_tile_type.to_string());
        for dx in 1..overlay_width {
            world.overlay_to_anchor.insert(format!("{},{}", x + dx, y), format!("{x},{y}"));
        }
        let mut p = CellPatch::new(x, y, normalized_tile_type, true);
        p.is_overlay = Some(true);
        patch.push(p);
        if !free_build {
            ledger.cash_balance -= shaft_cost;
        }
        run_global_rebuilds(world, ledger);
        return accepted(patch, None);
    }

    // ── Stairs / Escalator: bridge overlay on existing base tiles ─────────────
    if normalized_tile_type == "stairs" || normalized_tile_type == "escalator" {
        let overlay_width = tile_width(normalized_tile_type, 1);
        let overlay_cost = tile_cost(normalized_tile_type, 0);
        if x + overlay_width - 1 >= GRID_WIDTH {
            return rejected("Out of bounds");
        }
        if !free_build && overlay_cost > ledger.cash_balance {
            return rejected("Insufficient funds");
        }
        let base_required_label = if normalized_tile_type == "stairs" { "Stairs" } else { "Escalators" };
        for dx in 0..overlay_width {
            let key = format!("{},{}", x + dx, y);
            if !truthy(&world.cells, &key) && !truthy(&world.cell_to_anchor, &key) {
                return rejected(format!("{base_required_label} require a base tile"));
            }
            if truthy(&world.overlays, &key) || truthy(&world.overlay_to_anchor, &key) {
                return rejected("Cell already has an overlay");
            }
        }
        if y == GROUND_Y {
            for dx in 0..overlay_width {
                let upper_landing_key = format!("{},{}", x + dx, y - 1);
                if !truthy(&world.cells, &upper_landing_key) && !truthy(&world.cell_to_anchor, &upper_landing_key) {
                    return rejected(format!("{base_required_label} require a base tile on the floor above"));
                }
            }
        }
        if normalized_tile_type == "stairs" && has_overlapping_misaligned_stairs(world, x, y, overlay_width) {
            return rejected("Stairs must align with existing stairs in the same column or not overlap them");
        }
        world.overlays.insert(format!("{x},{y}"), normalized_tile_type.to_string());
        for dx in 1..overlay_width {
            world.overlay_to_anchor.insert(format!("{},{}", x + dx, y), format!("{x},{y}"));
        }
        if !free_build {
            ledger.cash_balance -= overlay_cost;
        }
        run_global_rebuilds(world, ledger);
        let mut p = CellPatch::new(x, y, normalized_tile_type, true);
        p.is_overlay = Some(true);
        return accepted(vec![p], None);
    }

    // ── Standard tile placement ───────────────────────────────────────────────
    let tile_width = tile_width(normalized_tile_type, 1);
    let cost = tile_cost(normalized_tile_type, 0);

    if x + tile_width - 1 >= world.width {
        return rejected("Out of bounds");
    }
    if normalized_tile_type == "lobby" && !is_valid_lobby_y(y, world.lobby_mode) {
        let cadence = if world.lobby_mode == LobbyMode::Modern { "15" } else { "14, then 15" };
        return rejected(format!(
            "Lobby only allowed on ground floor or every 15 floors above (offsets +{cadence})"
        ));
    }
    if !free_build && cost > ledger.cash_balance {
        return rejected("Insufficient funds");
    }

    let can_replace_floor = normalized_tile_type != "floor";
    let mut floor_to_remove: Vec<String> = Vec::new();
    for dx in 0..tile_width {
        let key = format!("{},{}", x + dx, y);
        if truthy(&world.cell_to_anchor, &key) {
            return rejected("Cell already occupied");
        }
        if let Some(existing) = world.cells.get(&key).filter(|s| !s.is_empty()) {
            if can_replace_floor && existing == "floor" {
                floor_to_remove.push(key);
            } else {
                return rejected("Cell already occupied");
            }
        }
    }

    // Tiles need support from the adjacent row (below for above-ground; above
    // for underground). Only the ground-floor lobby is exempt — sky lobbies
    // still require support like any other tile.
    if !(normalized_tile_type == "lobby" && y == GROUND_Y) {
        let support_y = if y >= UNDERGROUND_Y { y - 1 } else { y + 1 };
        for dx in 0..tile_width {
            let support_key = format!("{},{}", x + dx, support_y);
            if support_y < 0 || support_y >= world.height || !truthy(&world.cells, &support_key) {
                // BOX: car elevator -- under BOX RULES a basement may be dug
                // out sideways (to a car elevator under the street, say): an
                // underground tile beside built cells on its row stands.
                if crate::box_ext::car_elevator::side_support(world, x, y, tile_width) {
                    break;
                }
                return rejected("No support");
            }
        }
    }

    // Parking ramps live underground and stack downward: the topmost ramp sits
    // at the row just below the ground floor (y === UNDERGROUND_Y), and each
    // ramp below must hang off the ramp one row above it.
    if normalized_tile_type == "parkingRamp" {
        if y < UNDERGROUND_Y {
            return rejected("Parking ramps may only be placed underground");
        }
        let is_top_of_stack = y == UNDERGROUND_Y;
        let ramp_above_key = format!("{},{}", x, y - 1);
        let has_ramp_above = world.cells.get(&ramp_above_key).map(String::as_str) == Some("parkingRamp");
        if !is_top_of_stack && !has_ramp_above {
            return rejected("Parking ramp must hang from the row above (top of stack at floor B1)");
        }
    }

    // Recycling-center stacks must overlap an existing 0x14/0x15 stack within
    // the recovered search band (anchor-2 .. anchor+1).
    let family_code = js::record_get(TILE_TO_FAMILY_CODE, normalized_tile_type).copied().unwrap_or(0);
    if family_code == FAMILY_RECYCLING_CENTER_UPPER || family_code == FAMILY_RECYCLING_CENTER_LOWER {
        let proposed_floor = y_to_floor(y);
        let mut has_existing = false;
        let mut overlaps = false;
        for (key, obj) in world.placed_objects.iter() {
            if obj.object_type_code != FAMILY_RECYCLING_CENTER_UPPER
                && obj.object_type_code != FAMILY_RECYCLING_CENTER_LOWER
            {
                continue;
            }
            has_existing = true;
            let (_, oy) = key_xy(key);
            let existing_floor = y_to_floor(oy);
            if proposed_floor >= existing_floor - 2 && proposed_floor <= existing_floor + 1 {
                overlaps = true;
                break;
            }
        }
        if has_existing && !overlaps {
            return rejected("Recycling center must be placed near an existing recycling-center stack");
        }
    }

    // Apply placement
    for key in &floor_to_remove {
        world.cells.remove(key);
    }
    world.cells.insert(format!("{x},{y}"), normalized_tile_type.to_string());
    for dx in 1..tile_width {
        world.cells.insert(format!("{},{}", x + dx, y), normalized_tile_type.to_string());
        world.cell_to_anchor.insert(format!("{},{}", x + dx, y), format!("{x},{y}"));
    }
    if !free_build {
        ledger.cash_balance -= cost;
    }

    // PlacedObjectRecord
    if !INFRASTRUCTURE_TILES.contains(&normalized_tile_type) {
        let record = make_placed_object(x, y, normalized_tile_type, world, daypart_index, vip_flag);
        world.detach_object(&format!("{x},{y}"));
        world.placed_objects.insert(format!("{x},{y}"), record);
    }

    let record = world.placed_objects.get(&format!("{x},{y}"));
    let mut patch: Vec<CellPatch> = (0..tile_width)
        .map(|dx| {
            let mut p = CellPatch::new(x + dx, y, normalized_tile_type, dx == 0);
            if dx == 0 {
                if let Some(record) = record {
                    p.eval_active_flag = Some(record.occupied_flag);
                    p.unit_status = Some(record.unit_status);
                }
            }
            p
        })
        .collect();

    fill_row_gaps(y, world, &mut patch);

    run_global_rebuilds(world, ledger);

    if normalized_tile_type == "parking" || normalized_tile_type == "parkingRamp" {
        append_parking_coverage_patches(world, &mut patch);
    }

    accepted(patch, Some(cost > 0))
}

// ─── Remove tile ──────────────────────────────────────────────────────────────

pub fn handle_remove_tile(x: i64, y: i64, world: &mut WorldState, ledger: &mut LedgerState) -> CommandResult {
    if x < 0 || x >= world.width || y < 0 || y >= world.height {
        return rejected("Out of bounds");
    }
    let clicked_key = format!("{x},{y}");

    // BOX: car elevator -- a click on a car elevator removes all of it.
    if let Some(result) = crate::box_ext::car_elevator::intercept_remove(x, y, world, ledger) {
        return result;
    }
    // BOX: amenities -- a click on an amenity removes the whole room.
    if let Some(result) = crate::box_ext::amenities::intercept_remove(x, y, world, ledger) {
        return result;
    }

    // Remove overlay first if present
    let overlay_anchor_key = match world.overlay_to_anchor.get(&clicked_key) {
        Some(anchor) => Some(anchor.clone()),
        None => truthy(&world.overlays, &clicked_key).then(|| clicked_key.clone()),
    };
    if let Some(overlay_anchor_key) = overlay_anchor_key {
        let overlay_type = world.overlays.get(&overlay_anchor_key).cloned();
        let ow = overlay_type.as_deref().map_or(1, |t| tile_width(t, 1));
        let (ax, _) = key_xy(&overlay_anchor_key);
        world.overlays.remove(&overlay_anchor_key);
        for dx in 1..ow {
            world.overlay_to_anchor.remove(&format!("{},{}", ax + dx, y));
        }
        let (oax, oay) = key_xy(&overlay_anchor_key);
        // Carrier overlays require a routing rebuild on removal
        if matches!(
            overlay_type.as_deref(),
            Some("elevator" | "elevatorExpress" | "elevatorService" | "escalator" | "stairs")
        ) {
            run_global_rebuilds(world, ledger);
        }
        let mut p = CellPatch::new(oax, oay, "empty", true);
        p.is_overlay = Some(true);
        return accepted(vec![p], None);
    }

    let anchor_key = world.cell_to_anchor.get(&clicked_key).cloned().unwrap_or(clicked_key);
    let tile_type = match world.cells.get(&anchor_key) {
        Some(t) if !t.is_empty() => t.clone(),
        _ => return rejected("Cell is empty"),
    };

    let (ax, ay) = key_xy(&anchor_key);
    let tile_width = tile_width(&tile_type, 1);
    let is_two_floor = tile_type == "cinema" || tile_type == "partyHall";
    let is_cathedral = tile_type == "cathedral";
    let is_metro = tile_type == METRO_STACK_TILE_TYPE;
    let occupied_rows: Vec<i64> = if is_cathedral {
        (0..CATHEDRAL_SLICE_COUNT).map(|i| ay + i).collect()
    } else if is_metro {
        (0..METRO_STACK_HEIGHT).map(|i| ay + i).collect()
    } else if is_two_floor {
        vec![ay, ay + 1]
    } else {
        vec![ay]
    };
    // "Above" for turnToFloor considers the row above the topmost occupied row.
    let top_row = occupied_rows[0];
    // The lowest occupied row decides neighbour-in-row (left/right) logic and
    // is where the replacement floor would sit.
    let base_row = occupied_rows[occupied_rows.len() - 1];

    // Determine replacement: floor if anything sits above or tile is between neighbours
    let mut has_above = false;
    let mut dx = 0;
    while dx < tile_width && !has_above {
        if truthy(&world.cells, &format!("{},{}", ax + dx, top_row - 1)) {
            has_above = true;
        }
        dx += 1;
    }
    let mut has_left = false;
    let mut lx = ax - 1;
    while lx >= 0 && !has_left {
        if truthy(&world.cells, &format!("{lx},{base_row}")) {
            has_left = true;
        }
        lx -= 1;
    }
    let mut has_right = false;
    let mut rx = ax + tile_width;
    while rx < world.width && !has_right {
        if truthy(&world.cells, &format!("{rx},{base_row}")) {
            has_right = true;
        }
        rx += 1;
    }
    let turn_to_floor = has_above || (has_left && has_right);

    for &row_y in &occupied_rows {
        for dx in 0..tile_width {
            let key = format!("{},{}", ax + dx, row_y);
            world.cells.remove(&key);
            if key != anchor_key {
                world.cell_to_anchor.remove(&key);
            }
        }
    }

    // Remove PlacedObjectRecord(s) and free sidecar.
    // Entertainment venues (cinema / party hall) store 4 / 2 sub-records that
    // share one sidecar; collect all records referencing the same sidecar
    // within the placement footprint before deleting.
    let sub_record_keys: Vec<String> = if is_two_floor || is_cathedral || is_metro {
        world
            .placed_objects
            .keys()
            .filter(|key| {
                let (kx, ky) = key_xy(key);
                ky >= ay && ky <= occupied_rows[occupied_rows.len() - 1] && kx >= ax && kx <= ax + tile_width - 1
            })
            .map(str::to_string)
            .collect()
    } else {
        vec![anchor_key.clone()]
    };

    let mut freed_sidecars: IndexSet<i64> = IndexSet::new();
    for key in &sub_record_keys {
        let Some(rec) = world.placed_objects.get(key) else { continue };
        let linked = rec.linked_record_index;
        if linked >= 0 && !freed_sidecars.contains(&linked) {
            if let Some(SidecarRecord::MedicalCenter(_)) = sidecar_at(world, linked) {
                invalidate_medical_slots_for_sidecar(world, linked);
            }
            free_sidecar(linked, world);
            freed_sidecars.insert(linked);
        }
        world.detach_object(key);
        world.placed_objects.remove(key);
    }
    if is_cathedral {
        world.gate_flags.eval_sim_index = 0xffff;
    }
    if is_metro {
        world.gate_flags.metro_placed = 0;
        world.gate_flags.metro_station_floor_index = -1;
    }

    cleanup_sims_for_removed_tile(world, ax, ay);

    let mut patch: Vec<CellPatch> = Vec::new();
    for &row_y in &occupied_rows {
        let emit_floor = turn_to_floor && row_y == base_row;
        for dx in 0..tile_width {
            let result_type = if emit_floor { "floor" } else { "empty" };
            if emit_floor {
                world.cells.insert(format!("{},{}", ax + dx, row_y), "floor".to_string());
            }
            patch.push(CellPatch::new(ax + dx, row_y, result_type, true));
        }
    }

    run_global_rebuilds(world, ledger);

    if tile_type == "parking" || tile_type == "parkingRamp" {
        append_parking_coverage_patches(world, &mut patch);
    }

    accepted(patch, None)
}

// ─── Gap-fill helper ──────────────────────────────────────────────────────────

// ─── Rent level adjustment ────────────────────────────────────────────────────

/// Families that support rent level changes (rent_level 0-3).
const RENT_ADJUSTABLE_FAMILIES: &[i64] = &[3, 4, 5, 6, 7, 9, FAMILY_RETAIL, FAMILY_FAST_FOOD];

/// `time` is upstream's `{ daypartIndex }` (unused there too).
pub fn handle_set_rent_level(x: i64, y: i64, rent_level: i64, world: &mut WorldState, _daypart_index: i64) -> CommandResult {
    if !(0..=3).contains(&rent_level) {
        return rejected("Rent level must be 0-3");
    }
    let anchor_key = world.cell_to_anchor.get(&format!("{x},{y}")).cloned().unwrap_or_else(|| format!("{x},{y}"));
    let Some(record) = world.placed_objects.get_mut(&anchor_key) else {
        return rejected("No facility here");
    };
    if !RENT_ADJUSTABLE_FAMILIES.contains(&record.object_type_code) {
        return rejected("This facility does not have adjustable rent");
    }
    if record.object_type_code == FAMILY_CONDO && record.unit_status < 0x18 {
        return rejected("Sold condos cannot change rent");
    }
    record.rent_level = rent_level;
    // Immediate recompute keeps the inspected facility in sync with the command.
    accepted(Vec::new(), None)
}

// ─── Cinema movie pool ───────────────────────────────────────────────────────

const CINEMA_FAMILY_CODES: &[i64] = &[
    FAMILY_CINEMA,
    FAMILY_CINEMA_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CINEMA_STAIRS_LOWER,
];

/// Cycle the cinema's movie selector within the chosen pool and charge the
/// player. Mirrors `MOVIETITLEDIALOGFILTER` WM_COMMAND case 1 (new) / case 3
/// (classic) at 0x1108:45C1: classic = `(cur+1)%7`, new = `((cur+1)%7)+7`,
/// both reset `link_age_counter` so the next 240 rebuild reseeds budgets from
/// age tier 0.
pub fn handle_set_cinema_movie_pool(
    x: i64,
    y: i64,
    pool: CinemaMoviePool,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
) -> CommandResult {
    let anchor_key = world.cell_to_anchor.get(&format!("{x},{y}")).cloned().unwrap_or_else(|| format!("{x},{y}"));
    let Some(record) = world.placed_objects.get(&anchor_key) else {
        return rejected("Not a cinema");
    };
    if !CINEMA_FAMILY_CODES.contains(&record.object_type_code) {
        return rejected("Not a cinema");
    }
    let linked = record.linked_record_index;
    let sidecar = if linked >= 0 { world.sidecars.get_mut(linked as usize) } else { None };
    let Some(SidecarRecord::EntertainmentLink(sidecar)) = sidecar else {
        return rejected("Cinema sidecar missing");
    };
    if sidecar.family_selector_or_single_link_flag == 0xff {
        return rejected("Not a cinema");
    }

    let cost = if pool == CinemaMoviePool::Classic { CINEMA_CLASSIC_MOVIE_COST } else { CINEMA_NEW_MOVIE_COST };
    if !free_build && ledger.cash_balance < cost {
        return rejected("Insufficient funds");
    }

    let cur = sidecar.family_selector_or_single_link_flag;
    let next = if pool == CinemaMoviePool::Classic { (cur + 1) % 7 } else { ((cur + 1) % 7) + 7 };
    sidecar.family_selector_or_single_link_flag = next;
    sidecar.link_age_counter = 0;

    if !free_build {
        ledger.cash_balance -= cost;
    }

    accepted(Vec::new(), Some(!free_build))
}

// ─── Elevator car management ─────────────────────────────────────────────────

pub fn handle_add_elevator_car(
    x: i64,
    y: i64,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
) -> CommandResult {
    let Some(carrier_index) = find_carrier_at_click(world, x, y) else {
        return rejected("No elevator at this column");
    };
    let carrier = &mut world.carriers[carrier_index];
    let active_cars = carrier.cars.iter().filter(|c| c.active).count();
    if active_cars >= 8 {
        return rejected("Maximum 8 cars per shaft");
    }
    let cost = js::record_get(CARRIER_CAR_CONSTRUCTION_COST, carrier.carrier_mode).copied().unwrap_or(0);
    if !free_build && cost > ledger.cash_balance {
        return rejected("Insufficient funds");
    }
    if !free_build {
        ledger.cash_balance -= cost;
    }
    // Binary add-car branch (place_carrier_shaft at an existing column)
    // stores `param_3` — the click's floor — at each car's home_floor byte
    // (carrier +0xBA..0xC1). Use the same convention so per-car home
    // tracking matches the emulator's build_carrier output.
    let clicked = y_to_floor(y);
    let home_floor = carrier.bottom_served_floor.max(carrier.top_served_floor.min(clicked));
    // Activate first inactive car
    if let Some(car) = carrier.cars.iter_mut().find(|car| !car.active) {
        car.active = true;
        car.current_floor = home_floor;
        car.target_floor = home_floor;
        car.prev_floor = home_floor;
        car.home_floor = home_floor;
        return accepted(Vec::new(), None);
    }
    // All existing cars are active — add a new one
    let new_car = make_carrier_car(carrier.top_served_floor - carrier.bottom_served_floor + 1, home_floor);
    carrier.cars.push(new_car);
    accepted(Vec::new(), None)
}

pub fn handle_remove_elevator_car(x: i64, y: i64, world: &mut WorldState) -> CommandResult {
    let Some(carrier_index) = find_carrier_at_click(world, x, y) else {
        return rejected("No elevator at this column");
    };
    let carrier = &world.carriers[carrier_index];
    let active_cars = carrier.cars.iter().filter(|c| c.active).count();
    if active_cars <= 1 {
        return rejected("Must keep at least 1 car");
    }
    let has_active_traffic = !carrier.pending_routes.is_empty()
        || carrier.cars.iter().any(|car| {
            car.assigned_count > 0
                || car.pending_assignment_count > 0
                || car.active_route_slots.iter().any(|slot| slot.active)
        });
    if has_active_traffic {
        world.event_state.pending_carrier_edit_column = x;
        world.event_state.pending_carrier_edit_y = y;
        world.pending_prompts.push(SimPrompt {
            prompt_id: format!("carrier_remove_{x}_{y}"),
            prompt_kind: SimPromptKind::CarrierEditConfirmation,
            message: "Removing this elevator car will disrupt active traffic. Continue?".to_string(),
            cost: None,
        });
        return accepted(Vec::new(), None);
    }
    apply_remove_elevator_car(world, x, y)
}

pub fn apply_remove_elevator_car(world: &mut WorldState, x: i64, y: i64) -> CommandResult {
    let Some(carrier_index) = find_carrier_at_click(world, x, y) else {
        return rejected("No elevator at this column");
    };
    let carrier = &mut world.carriers[carrier_index];
    let active_cars: Vec<usize> = (0..carrier.cars.len()).filter(|&i| carrier.cars[i].active).collect();
    if active_cars.len() <= 1 {
        return rejected("Must keep at least 1 car");
    }
    let last_car = &mut carrier.cars[active_cars[active_cars.len() - 1]];
    last_car.active = false;
    last_car.assigned_count = 0;
    last_car.pending_assignment_count = 0;
    last_car.pending_route_ids = Vec::new();
    accepted(Vec::new(), None)
}

pub fn handle_set_elevator_dwell_delay(x: i64, y: i64, value: i64, world: &mut WorldState) -> CommandResult {
    let Some(carrier_index) = find_carrier_at_click(world, x, y) else {
        return rejected("No elevator at this column");
    };
    world.carriers[carrier_index].dwell_delay = vec![value; 14];
    accepted(Vec::new(), None)
}

pub fn handle_set_elevator_waiting_car_response(x: i64, y: i64, value: i64, world: &mut WorldState) -> CommandResult {
    let Some(carrier_index) = find_carrier_at_click(world, x, y) else {
        return rejected("No elevator at this column");
    };
    world.carriers[carrier_index].waiting_car_response_threshold = value;
    accepted(Vec::new(), None)
}

pub fn handle_set_elevator_home_floor(x: i64, car_index: i64, floor: i64, world: &mut WorldState) -> CommandResult {
    let Some(carrier_index) = find_carrier_by_column_and_floor(world, x, floor) else {
        return rejected("No elevator at this column");
    };
    let carrier = &world.carriers[carrier_index];
    let active_cars: Vec<usize> = (0..carrier.cars.len()).filter(|&i| carrier.cars[i].active).collect();
    let Some(&car) = (if car_index >= 0 { active_cars.get(car_index as usize) } else { None }) else {
        return rejected("Car not found");
    };
    if floor_to_slot(carrier, floor) < 0 {
        return rejected("Floor not served by this elevator");
    }
    world.carriers[carrier_index].cars[car].home_floor = floor;
    accepted(Vec::new(), None)
}

pub fn handle_toggle_elevator_floor_stop(x: i64, floor: i64, world: &mut WorldState) -> CommandResult {
    let Some(carrier_index) = find_carrier_by_column_and_floor(world, x, floor) else {
        return rejected("No elevator at this column");
    };
    let carrier = &mut world.carriers[carrier_index];
    let slot = floor_to_slot(carrier, floor);
    if slot < 0 {
        return rejected("Floor not served by this elevator");
    }
    let s = slot as usize;
    // (carriers.ts sizes stopFloorEnabled to the served span, so `slot` is
    // in range; the `?? 1` is upstream's.)
    let was_enabled = js::at_or(&carrier.stop_floor_enabled, slot, 1) != 0;
    carrier.stop_floor_enabled[s] = if was_enabled { 0 } else { 1 };

    // Binary FUN_10a8_0085 follow-up: when disabling, FUN_10a8_14cc clears
    // route-status slots so cars en route to the disabled floor get
    // re-targeted on the next recompute. Mirror by zeroing primary/secondary
    // per-floor status and decrementing the assigned car's pending count.
    if was_enabled {
        let primary_tag = js::at_or(&carrier.primary_route_status_by_floor, slot, 0);
        if primary_tag != 0 {
            carrier.primary_route_status_by_floor[s] = 0;
            if primary_tag >= 1 {
                if let Some(car) = carrier.cars.get_mut((primary_tag - 1) as usize) {
                    if car.pending_assignment_count > 0 {
                        car.pending_assignment_count -= 1;
                    }
                }
            }
        }
        let secondary_tag = js::at_or(&carrier.secondary_route_status_by_floor, slot, 0);
        if secondary_tag != 0 {
            carrier.secondary_route_status_by_floor[s] = 0;
            if secondary_tag >= 1 {
                if let Some(car) = carrier.cars.get_mut((secondary_tag - 1) as usize) {
                    if car.pending_assignment_count > 0 {
                        car.pending_assignment_count -= 1;
                    }
                }
            }
        }

        // Sims with a pending route touching the disabled floor would otherwise
        // stay in line forever — visually, riders queue up and the elevator
        // never arrives (or arrives without being able to drop them off).
        // Evict every not-yet-boarded route whose source OR destination is the
        // disabled floor, then clear each sim's route so the family state
        // machine re-resolves on its next tick (which now scores the disabled
        // floor as ineligible and picks a peer carrier or transfer).
        let evicted_ids: Vec<String> = carrier
            .pending_routes
            .iter()
            .filter(|route| !route.boarded && (route.source_floor == floor || route.destination_floor == floor))
            .map(|route| route.sim_id.clone())
            .collect();
        for evicted_id in &evicted_ids {
            cancel_runtime_route_request(carrier, evicted_id);
        }
        if !evicted_ids.is_empty() {
            let evicted_set: IndexSet<&String> = evicted_ids.iter().collect();
            for sim in world.sims.iter_mut() {
                if evicted_set.contains(&sim_key(sim)) {
                    clear_sim_route(sim);
                }
            }
        }
    }

    // Binary FUN_10a8_0085 unconditionally rebuilds the transfer-group cache
    // and route-reachability tables after any toggle.
    rebuild_transfer_group_cache(world);
    rebuild_route_reachability_tables(world);

    accepted(Vec::new(), None)
}

// ─── Gap-fill helper ──────────────────────────────────────────────────────────

/// After a placement on row y, fill supported horizontal gaps with free floor tiles.
pub fn fill_row_gaps(y: i64, world: &mut WorldState, patch: &mut Vec<CellPatch>) {
    let support_y = if y >= UNDERGROUND_Y { y - 1 } else { y + 1 };
    if support_y < 0 || support_y >= world.height {
        return;
    }

    let mut leftmost = -1;
    let mut rightmost = -1;
    for x in 0..world.width {
        if truthy(&world.cells, &format!("{x},{y}")) {
            if leftmost == -1 {
                leftmost = x;
            }
            rightmost = x;
        }
    }
    if leftmost == -1 {
        return;
    }

    for x in leftmost..=rightmost {
        let key = format!("{x},{y}");
        if truthy(&world.cells, &key) {
            continue;
        }
        if !truthy(&world.cells, &format!("{x},{support_y}")) {
            continue;
        }
        world.cells.insert(key, "floor".to_string());
        patch.push(CellPatch::new(x, y, "floor", true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_deserialise_with_upstream_names() {
        let c: SimCommand =
            serde_json::from_str(r#"{"type":"place_tile","x":3,"y":109,"tileType":"lobby"}"#).unwrap();
        assert_eq!(c, SimCommand::PlaceTile { x: 3, y: 109, tile_type: "lobby".into() });
        let c: SimCommand =
            serde_json::from_str(r#"{"type":"set_elevator_home_floor","x":3,"carIndex":1,"floor":12}"#).unwrap();
        assert_eq!(c, SimCommand::SetElevatorHomeFloor { x: 3, car_index: 1, floor: 12 });
        let c: SimCommand =
            serde_json::from_str(r#"{"type":"set_cinema_movie_pool","x":1,"y":2,"pool":"new"}"#).unwrap();
        assert_eq!(c, SimCommand::SetCinemaMoviePool { x: 1, y: 2, pool: CinemaMoviePool::New });
        let c = SimCommand::SetElevatorWaitingCarResponse { x: 1, y: 2, value: 3 };
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"type":"set_elevator_waiting_car_response","x":1,"y":2,"value":3}"#
        );
        let c = SimCommand::PromptResponse { prompt_id: "fire_83".into(), accepted: false };
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"{"type":"prompt_response","promptId":"fire_83","accepted":false}"#);
    }

    #[test]
    fn results_serialise_as_upstream_builds_them() {
        assert_eq!(serde_json::to_string(&rejected("No support")).unwrap(), r#"{"accepted":false,"reason":"No support"}"#);
        let mut p = CellPatch::new(1, 2, "parking", true);
        p.coverage_flag = Some(0);
        assert_eq!(
            serde_json::to_string(&accepted(vec![p], Some(true))).unwrap(),
            r#"{"accepted":true,"patch":[{"x":1,"y":2,"tileType":"parking","isAnchor":true,"coverageFlag":0}],"economyChanged":true}"#
        );
    }

    fn world() -> WorldState {
        crate::snapshot::create_initial_snapshot("t", "t", 0, None).world
    }

    #[test]
    fn fill_row_gaps_fills_supported_holes_only() {
        let mut w = world();
        let y = GROUND_Y - 1;
        for x in [10, 11, 12, 13, 14] {
            w.cells.insert(format!("{x},{}", y + 1), "lobby".to_string());
        }
        w.cells.insert(format!("10,{y}"), "office".to_string());
        w.cells.insert(format!("15,{y}"), "office".to_string());
        let mut patch = Vec::new();
        fill_row_gaps(y, &mut w, &mut patch);
        let xs: Vec<i64> = patch.iter().map(|p| p.x).collect();
        assert_eq!(xs, [11, 12, 13, 14]);
        assert_eq!(w.cells.get(&format!("12,{y}")).map(String::as_str), Some("floor"));
    }

    #[test]
    fn placement_rejections_before_any_rebuild() {
        let mut w = world();
        let mut l = crate::ledger::create_ledger_state(1_000);
        let r = handle_place_tile(0, 0, "nonsense", &mut w, &mut l, None, None);
        assert_eq!(r.reason.as_deref(), Some("Invalid tile type"));
        let r = handle_place_tile(0, UNDERGROUND_Y, "office", &mut w, &mut l, None, None);
        assert_eq!(r.reason.as_deref(), Some("This tile cannot be placed underground"));
        let r = handle_place_tile(10, GROUND_Y, "office", &mut w, &mut l, None, None);
        assert_eq!(r.reason.as_deref(), Some("Insufficient funds"));
        let r = handle_place_tile(10, GROUND_Y - 1, "lobby", &mut w, &mut l, Some(true), None);
        assert_eq!(
            r.reason.as_deref(),
            Some("Lobby only allowed on ground floor or every 15 floors above (offsets +15)")
        );
        let r = handle_place_tile(10, GROUND_Y - 1, "floor", &mut w, &mut l, Some(true), None);
        assert_eq!(r.reason.as_deref(), Some("No support"));
        let r = handle_place_tile(10, GROUND_Y - 1, "vipSuite", &mut w, &mut l, Some(true), None);
        assert_eq!(r.reason.as_deref(), Some("No support"));
        assert!(w.cells.is_empty() && w.placed_objects.is_empty());
    }

    #[test]
    fn shaft_spacing() {
        let mut w = world();
        let mut add = |column: i64, mode: i64| {
            let mut c: crate::world::CarrierRecord = serde_json::from_value(serde_json::json!({
                "carrierId": 0, "column": column, "carrierMode": mode, "topServedFloor": 12,
                "bottomServedFloor": 10, "servedFloorFlags": [], "primaryRouteStatusByFloor": [],
                "secondaryRouteStatusByFloor": [], "serviceScheduleFlags": [], "dwellDelay": [],
                "expressDirectionFlags": [], "waitingCarResponseThreshold": 0, "assignmentCapacity": 0,
                "floorQueues": [], "pendingRoutes": [], "completedRouteIds": [],
                "suppressedFloorAssignments": [], "stopFloorEnabled": [], "cars": []
            }))
            .unwrap();
            c.carrier_id = column;
            w.carriers.push(c);
        };
        add(100, 1);
        // A standard shaft is 4 wide: 104..107 is a gap of 0..3, 108 of 4.
        assert!(too_close_to_existing_shaft(&w, 107, 4, 1));
        assert!(!too_close_to_existing_shaft(&w, 108, 4, 1));
        assert!(too_close_to_existing_shaft(&w, 108, 6, 0)); // express wants 8
        assert!(!too_close_to_existing_shaft(&w, 112, 6, 0));
        assert!(!too_close_to_existing_shaft(&w, 100, 4, 1)); // same column
        assert_eq!(find_carrier_at_click(&w, 100, GROUND_Y - 30), Some(0));
        assert_eq!(find_carrier_at_click(&w, 101, GROUND_Y), None);
    }

    // ts: commands.test.ts, through TowerSim.  They need every group's
    // modules (each placement runs the global rebuilds), so they are
    // ignored until the merge: `cargo test -- --ignored` then runs them.
    mod upstream {
        use crate::commands::SimCommand;
        use crate::events::tick_vip_special_visitor;
        use crate::index::{CommandResult, TowerSim};
        use crate::resources::{FAMILY_METRO_BOTTOM, FAMILY_METRO_MIDDLE, FAMILY_METRO_TOP, TILE_COSTS};
        use crate::time::create_time_state;

        const GROUND_Y: i64 = 109;

        fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) -> CommandResult {
            sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() })
        }

        fn make_sim_with_lobby_strip(from: i64, to: i64) -> TowerSim {
            let mut sim = TowerSim::create("spacing-test", "Spacing", None);
            for x in from..=to {
                let result = place(&mut sim, x, GROUND_Y, "lobby");
                assert!(result.accepted, "lobby placement at x={x} failed: {:?}", result.reason);
            }
            sim
        }

        fn place_elevator_segment(sim: &mut TowerSim, x: i64, floor: i64, tile_type: &str) -> CommandResult {
            place(sim, x, GROUND_Y - floor, tile_type)
        }

        fn reason(r: &CommandResult) -> String {
            r.reason.clone().unwrap_or_default()
        }

        // describe("elevator shaft spacing")

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn allows_extending_the_same_shaft_with_additional_segments() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            assert!(place_elevator_segment(&mut sim, 100, 0, "elevator").accepted);
            assert!(place_elevator_segment(&mut sim, 100, 1, "elevator").accepted);
            assert!(place_elevator_segment(&mut sim, 100, 2, "elevator").accepted);
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn rejects_a_second_shaft_within_4_tiles_of_an_existing_shaft() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            assert!(place_elevator_segment(&mut sim, 100, 0, "elevator").accepted);
            // Standard elevator width=4 → right edge at x=103. Spacing requires a
            // 4-tile gap, so the next shaft's left must be ≥108. x=107 leaves only
            // 3 empty tiles (104,105,106) between shafts.
            let rejection = place_elevator_segment(&mut sim, 107, 0, "elevator");
            assert!(!rejection.accepted);
            assert!(reason(&rejection).contains("too close"));
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn accepts_a_second_standard_shaft_exactly_4_tiles_away() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            assert!(place_elevator_segment(&mut sim, 100, 0, "elevator").accepted);
            assert!(place_elevator_segment(&mut sim, 108, 0, "elevator").accepted);
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn requires_8_clear_tiles_on_either_side_of_an_express_shaft() {
            // Express (width 6) at column 100 → right=105. A standard shaft needs
            // ≥8 empty tiles from the express, so its left must be ≥114
            // (that's 14 tiles left-edge-to-left-edge).
            let mut sim = make_sim_with_lobby_strip(90, 180);
            assert!(place_elevator_segment(&mut sim, 100, 0, "elevatorExpress").accepted);
            assert!(!place_elevator_segment(&mut sim, 113, 0, "elevator").accepted);
            assert!(place_elevator_segment(&mut sim, 114, 0, "elevator").accepted);
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn requires_8_clear_tiles_when_the_new_shaft_is_express() {
            // Standard at 100 (right=103). Next express needs ≥8 gap, so left ≥112.
            let mut sim = make_sim_with_lobby_strip(90, 180);
            assert!(place_elevator_segment(&mut sim, 100, 0, "elevator").accepted);
            assert!(!place_elevator_segment(&mut sim, 111, 0, "elevatorExpress").accepted);
            assert!(place_elevator_segment(&mut sim, 112, 0, "elevatorExpress").accepted);
        }

        fn build_floors(sim: &mut TowerSim, floor: i64, from: i64, to: i64) {
            for x in from..=to {
                let result = place(sim, x, GROUND_Y - floor, "floor");
                assert!(result.accepted, "floor placement at x={x},floor={floor} failed: {:?}", result.reason);
            }
        }

        fn place_stairs(sim: &mut TowerSim, x: i64, floor: i64) -> CommandResult {
            place(sim, x, GROUND_Y - floor, "stairs")
        }

        // describe("underground placement restrictions")

        const UNDERGROUND_Y: i64 = 110;

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn rejects_rooms_underground() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            let blocked = ["hotelSingle", "hotelTwin", "hotelSuite", "office", "condo", "housekeeping", "lobby"];
            for tile_type in blocked {
                let r = place(&mut sim, 100, UNDERGROUND_Y, tile_type);
                assert!(!r.accepted);
                assert!(reason(&r).to_lowercase().contains("underground"));
            }
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn allows_infrastructure_and_parking_recycling_underground() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            // Floor underground (hangs from ground lobby above).
            assert!(place(&mut sim, 100, UNDERGROUND_Y, "floor").accepted);
            // Parking underground.
            assert!(place(&mut sim, 120, UNDERGROUND_Y, "parking").accepted);
            // Elevator overlay extending into underground.
            assert!(place(&mut sim, 140, UNDERGROUND_Y - 1, "elevator").accepted);
            assert!(place(&mut sim, 140, UNDERGROUND_Y, "elevator").accepted);
        }

        // describe("metro station placement")

        fn place_metro(sim: &mut TowerSim, x: i64) -> CommandResult {
            place(sim, x, GROUND_Y + 1, "metro")
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn places_a_singleton_three_floor_stack_with_the_top_floor_as_anchor() {
            let mut sim = make_sim_with_lobby_strip(90, 150);
            let result = place_metro(&mut sim, 100);
            assert!(result.accepted);
            assert_eq!(result.patch.as_ref().map(Vec::len), Some(90));

            let snap = sim.save_state();
            assert_eq!(snap.world.gate_flags.metro_placed, 1);
            assert_eq!(snap.world.gate_flags.metro_station_floor_index, 9);
            assert_eq!(snap.world.placed_objects.get("100,110").map(|o| o.object_type_code), Some(FAMILY_METRO_TOP));
            assert_eq!(snap.world.placed_objects.get("100,111").map(|o| o.object_type_code), Some(FAMILY_METRO_MIDDLE));
            assert_eq!(snap.world.placed_objects.get("100,112").map(|o| o.object_type_code), Some(FAMILY_METRO_BOTTOM));
            assert_eq!(snap.world.cell_to_anchor.get("129,112").map(String::as_str), Some("100,110"));
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn rejects_a_second_metro_and_resets_the_anchor_floor_when_removed() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            assert!(place_metro(&mut sim, 100).accepted);
            let second = place_metro(&mut sim, 140);
            assert!(!second.accepted);
            assert!(reason(&second).to_lowercase().contains("already placed"));

            let removed = sim.submit_command(&SimCommand::RemoveTile { x: 110, y: GROUND_Y + 2 });
            assert!(removed.accepted);
            let snap = sim.save_state();
            assert_eq!(snap.world.gate_flags.metro_placed, 0);
            assert_eq!(snap.world.gate_flags.metro_station_floor_index, -1);
            assert!(snap.world.placed_objects.get("100,110").is_none());
            assert!(snap.world.placed_objects.get("100,111").is_none());
            assert!(snap.world.placed_objects.get("100,112").is_none());
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn charges_regardless_of_daypart_and_seeds_the_binary_status_word() {
            let metro_cost = crate::js::record_get(TILE_COSTS, "metro").copied().unwrap();
            let paid = make_sim_with_lobby_strip(90, 150);
            let mut paid_snap = paid.save_state();
            paid_snap.time.daypart_index = 4;
            let mut paid_sim = TowerSim::from_snapshot(&paid_snap);
            let paid_before = paid_sim.cash();
            assert!(place_metro(&mut paid_sim, 100).accepted);
            assert_eq!(paid_sim.cash(), paid_before - metro_cost);
            assert_eq!(paid_sim.save_state().world.placed_objects.get("100,110").map(|o| o.unit_status), Some(1));

            let early = make_sim_with_lobby_strip(90, 150);
            let mut early_snap = early.save_state();
            early_snap.time.day_tick = 1200;
            early_snap.time.daypart_index = 3;
            let mut early_sim = TowerSim::from_snapshot(&early_snap);
            let early_before = early_sim.cash();
            assert!(place_metro(&mut early_sim, 100).accepted);
            assert_eq!(early_sim.cash(), early_before - metro_cost);
            assert_eq!(early_sim.save_state().world.placed_objects.get("100,110").map(|o| o.unit_status), Some(0));
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn blocks_carrier_extension_below_the_metro_middle_floor() {
            let mut sim = make_sim_with_lobby_strip(90, 150);
            assert!(place_metro(&mut sim, 100).accepted);
            assert!(place_elevator_segment(&mut sim, 140, -1, "elevator").accepted);
            assert!(place_elevator_segment(&mut sim, 140, -2, "elevator").accepted);
            let blocked = place_elevator_segment(&mut sim, 140, -3, "elevator");
            assert!(!blocked.accepted);
            assert!(reason(&blocked).to_lowercase().contains("below the metro"));
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn toggles_the_metro_display_variant_and_emits_the_arrival_event() {
            let base = make_sim_with_lobby_strip(90, 150);
            let mut base_snap = base.save_state();
            base_snap.time.daypart_index = 3;
            let mut sim = TowerSim::from_snapshot(&base_snap);
            assert!(place_metro(&mut sim, 100).accepted);
            let mut snap = sim.save_state();
            snap.world.rng_state = 54;
            snap.world.rng_call_count = 0;
            let mut time = create_time_state();
            time.day_tick = 0xf1;
            time.daypart_index = 3;

            tick_vip_special_visitor(&mut snap.world, &time);

            for key in ["100,110", "100,111", "100,112"] {
                assert_eq!(snap.world.placed_objects.get(key).map(|o| o.unit_status), Some(2));
            }
            assert!(serde_json::to_value(&snap.world.pending_notifications)
                .unwrap()
                .as_array()
                .unwrap()
                .contains(&serde_json::json!({ "kind": "event", "message": "metro_train_arrival" })));
        }

        // describe("stairs alignment")

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn allows_stacking_stairs_at_the_same_column_on_consecutive_floors() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            build_floors(&mut sim, 1, 90, 180);
            build_floors(&mut sim, 2, 90, 180);
            build_floors(&mut sim, 3, 90, 180);
            assert!(place_stairs(&mut sim, 100, 0).accepted);
            assert!(place_stairs(&mut sim, 100, 1).accepted);
            assert!(place_stairs(&mut sim, 100, 2).accepted);
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn allows_stairs_that_do_not_horizontally_overlap() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            build_floors(&mut sim, 1, 90, 180);
            build_floors(&mut sim, 2, 90, 180);
            // Stairs width=8 → cols [100..107] and [108..115] do not overlap.
            assert!(place_stairs(&mut sim, 100, 0).accepted);
            assert!(place_stairs(&mut sim, 108, 1).accepted);
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn rejects_stairs_that_horizontally_overlap_an_unaligned_neighbor() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            build_floors(&mut sim, 1, 90, 180);
            build_floors(&mut sim, 2, 90, 180);
            assert!(place_stairs(&mut sim, 100, 0).accepted);
            // cols [104..111] overlap [100..107] but anchor x=104 ≠ 100.
            let rejection = place_stairs(&mut sim, 104, 1);
            assert!(!rejection.accepted);
            assert!(reason(&rejection).contains("align"));
        }

        #[test]
        #[ignore = "needs every group's modules (the global rebuilds)"]
        fn rejects_stairs_when_no_base_tile_exists_on_the_floor_above() {
            let mut sim = make_sim_with_lobby_strip(90, 180);
            // Lobby strip exists on floor 0 but no floor 1 above.
            let rejection = place_stairs(&mut sim, 100, 0);
            assert!(!rejection.accepted);
            assert!(reason(&rejection).contains("floor above"));
        }
    }
}

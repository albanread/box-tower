// ts: world.ts

use serde::{Deserialize, Serialize};

use crate::js::{self, JsObject};
use crate::queue::route_record::RouteRequestRing;

// Grid and floor model constants
pub const GRID_WIDTH: i64 = 375;
pub const GRID_HEIGHT: i64 = 120; // floor indices 0–119; floor 10 = ground ("0"), floor 119 = top

/// Convert grid Y coordinate to floor index (0=bottom underground, 119=top).
pub fn y_to_floor(y: i64) -> i64 {
    GRID_HEIGHT - 1 - y
}

/// Convert floor index to grid Y coordinate.
pub fn floor_to_y(floor: i64) -> i64 {
    GRID_HEIGHT - 1 - floor
}
pub const UNDERGROUND_FLOORS: i64 = 10; // floors 0–9 underground; floor 10 = ground ("0")
pub const UNDERGROUND_Y: i64 = GRID_HEIGHT - UNDERGROUND_FLOORS; // Y=110: first underground row
pub const GROUND_Y: i64 = GRID_HEIGHT - 1 - UNDERGROUND_FLOORS; // Y=109: ground lobby row

/// Lobby placement mode.
///   "perfect-parity": ground + sky lobbies at floors 0, 14, 29, 44, ...
///     (matches the original binary's express-stop convention; required for
///     trace-test parity).
///   "modern": ground + sky lobbies at floors 0, 15, 30, 45, ... (cleaner
///     every-15 cadence; used for new towers in modern play).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LobbyMode {
    #[serde(rename = "perfect-parity")]
    PerfectParity,
    #[serde(rename = "modern")]
    Modern,
}

pub const DEFAULT_LOBBY_MODE: LobbyMode = LobbyMode::Modern;

/// True iff the given Y is a valid lobby row for the given lobby mode.
pub fn is_valid_lobby_y(y: i64, mode: LobbyMode) -> bool {
    let floors_above_ground = GROUND_Y - y;
    if floors_above_ground < 0 {
        return false;
    }
    if floors_above_ground == 0 {
        return true;
    }
    let offset = if mode == LobbyMode::Modern { 0 } else { 14 };
    floors_above_ground % 15 == offset
}

// ─── PRNG ────────────────────────────────────────────────────────────────────

/// Sample a 15-bit LCG value from the world RNG and advance its state.
/// 32-bit LCG: state = state * 0x15a4e35 + 1 (mod 2^32).
/// Returns upper 15 bits: (state >>> 16) & 0x7fff.
// (Upstream's __RNG_TRACE debugging shim is left out: TRANSLATION.md.)
pub fn sample_rng(world: &mut WorldState) -> i64 {
    world.rng_state = js::to_int32(js::imul(world.rng_state, 0x15a4e35) + 1);
    world.rng_call_count += 1;
    js::shr(world.rng_state, 16) & 0x7fff
}

// ─── Carrier types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierCar {
    pub active: bool,
    pub current_floor: i64,
    pub settle_counter: i64,
    pub dwell_counter: i64,
    pub assigned_count: i64,
    pub pending_assignment_count: i64,
    pub dwell_start_pending_assignment_count: i64,
    /// 0 = downward, 1 = upward (matches binary convention).
    pub direction_flag: i64,
    pub target_floor: i64,
    pub prev_floor: i64,
    pub home_floor: i64,
    /// Binary -0x51 nearest_work_floor: scan-direction nearest floor with
    /// pending work for this car (queued rider OR primary/secondary slot ==
    /// carIndex+1), with `homeFloor` as the no-work fallback. Maintained at
    /// the end of `recomputeCarTargetAndDirection`. Used as the wrap-cost
    /// "turn floor" in `findBestAvailableCarForFloor` and as the idle-home
    /// test (current == nearest_work_floor) in the same routine.
    pub nearest_work_floor: i64,
    pub schedule_flag: i64,
    /// Binary -0x57: latch set once car reaches a target; gates A1 dwell write.
    pub arrival_seen: i64,
    /// Binary -0x56: dayTick latched when A1 fires.
    pub arrival_tick: i64,
    pub arrival_dispatch_this_tick: bool,
    pub arrival_dispatch_starting_assigned_count: i64,
    pub suppress_dwell_opposite_direction_flip: bool,
    /// Waiting sim count indexed by floor slot.
    pub waiting_count: Vec<i64>,
    pub destination_count_by_floor: Vec<i64>,
    pub nonempty_destination_count: i64,
    pub active_route_slots: Vec<CarrierRouteSlot>,
    pub pending_route_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierRouteSlot {
    pub route_id: String,
    pub source_floor: i64,
    pub destination_floor: i64,
    pub boarded: bool,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierFloorQueue {
    pub up: RouteRequestRing,
    pub down: RouteRequestRing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierPendingRoute {
    pub sim_id: String,
    pub source_floor: i64,
    pub destination_floor: i64,
    pub boarded: bool,
    pub direction_flag: i64,
    pub assigned_car_index: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierRecord {
    pub carrier_id: i64,
    /// X column of the shaft.
    pub column: i64,
    /// 0 = Express Elevator, 1 = Standard Elevator, 2 = Service Elevator.
    /// The route scorer treats modes 0/1 as local-mode and mode 2 as the
    /// express long-hop carrier. Escalators are NOT carriers — they are
    /// special-link segments.
    ///
    /// (Upstream's type is the literal union `0 | 1 | 2`; a plain i64 here,
    /// compared with the numbers as upstream does.)
    pub carrier_mode: i64,
    pub top_served_floor: i64,
    pub bottom_served_floor: i64,
    /// 14 entries: 7 dayparts × 2 calendar phases. 1 = floor served, 0 = skipped.
    pub served_floor_flags: Vec<i64>,
    pub primary_route_status_by_floor: Vec<i64>,
    pub secondary_route_status_by_floor: Vec<i64>,
    pub service_schedule_flags: Vec<i64>,
    /// 14 entries: 7 dayparts × 2 calendar phases. Holds the per-daypart
    /// departure-dwell multiplier; the actual dwell is `multiplier * 30` ticks.
    /// Distinct from `serviceScheduleFlags` which is the schedule-enable byte.
    /// Default = 1.
    pub dwell_delay: Vec<i64>,
    /// 14 entries: 7 dayparts × 2 calendar phases. Controls alternate-direction
    /// queue drain behavior per daypart. 0 = normal (both directions),
    /// 1 = express to top (prefer upward, skip alternate downward drain),
    /// 2 = express to bottom (prefer downward, skip alternate upward drain).
    pub express_direction_flags: Vec<i64>,
    pub waiting_car_response_threshold: i64,
    pub assignment_capacity: i64,
    pub floor_queues: Vec<CarrierFloorQueue>,
    pub pending_routes: Vec<CarrierPendingRoute>,
    pub completed_route_ids: Vec<String>,
    pub suppressed_floor_assignments: Vec<String>,
    /// Per-slot (floor) stop flag. 1 = elevator stops here, 0 = passes through.
    pub stop_floor_enabled: Vec<i64>,
    pub cars: Vec<CarrierCar>,
}

// ─── Runtime sims ────────────────────────────────────────────────────────

/// `RouteState`'s carrier direction (upstream's inline `"up" | "down"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteDirection {
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum RouteState {
    Idle,
    #[serde(rename_all = "camelCase")]
    Segment { segment_id: i64, destination: i64 },
    #[serde(rename_all = "camelCase")]
    Carrier {
        carrier_id: i64,
        direction: RouteDirection,
        source: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimRecord {
    pub floor_anchor: i64,
    pub home_column: i64,
    pub base_offset: i64,
    /// Per-floor local object index (binary sim+1 / BP+0xc): index of this sim's home object within its floor's object list, in placement (x-ascending) order.
    pub facility_slot: i64,
    pub family_code: i64,
    /// SimRecord+5 state_code byte (ROUTING-BINARY-MAP.md §4.1):
    ///   bits 0..3: phase (0..7)
    ///   bit 5 (0x20): currently waiting (queue-full parked)
    ///   bit 6 (0x40): route queued / in-transit on carrier or segment
    /// Phase 5b makes the two mode bits the authoritative source for
    /// routing-mode branching (via `sim-access/state-bits.ts`). The
    /// `route` auxiliary struct stays as bookkeeping for carrier/segment
    /// ids and direction.
    pub state_code: i64,
    pub route: RouteState,
    pub selected_floor: i64,
    pub origin_floor: i64,
    pub destination_floor: i64,
    pub venue_return_state: i64,
    pub queue_tick: i64,
    /// Current elapsed ticks for the in-progress service visit (maps to low 10 bits of elapsed_packed).
    pub elapsed_ticks: i64,
    pub transit_ticks_remaining: i64,
    pub last_demand_tick: i64,
    pub trip_count: i64,
    pub accumulated_ticks: i64,
    /// Housekeeping helper (family 0x0f): selected hotel room's floor, with -1 as the searching sentinel.
    pub target_room_floor: i64,
    /// Housekeeping helper: leftTileIndex (column) of the claimed room. Paired with targetRoomFloor to uniquely identify the room at cleanup time, mirroring binary sim+0xc (the floor-local object id written by find_matching_vacant_unit_floor).
    pub target_room_column: i64,
    /// Housekeeping helper: recorded spawn floor used as the candidate-search seed and modulo class.
    pub spawn_floor: i64,
    /// Housekeeping helper: 3-tick post-claim countdown.
    pub post_claim_countdown: i64,
    /// Housekeeping helper: encoded subtype/slot of the claimed room (`(0 - floor) * 0x400`).
    pub encoded_target_floor: i64,
    /// Binary sim[+6] commercial-venue slot index for entertainment guests
    /// (handle_entertainment_service_acquisition at 1228:57e2). Holds the
    /// sidecar index of the picked CommercialVenueRecord; -1 means lobby
    /// fallback (no candidate). Other families overload sim[+6] differently
    /// and use `selectedFloor` for that.
    pub commercial_venue_slot: i64,
}

// ─── Routing types ────────────────────────────────────────────────────────────

pub const MAX_SPECIAL_LINKS: i64 = 64;
pub const MAX_SPECIAL_LINK_RECORDS: i64 = 8;
pub const MAX_TRANSFER_GROUPS: i64 = 16;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecialLinkSegment {
    pub active: bool,
    /// bit 0 = stairs cost bit (0 = Escalator branch, 1 = Stairs branch); bits 7:1 = inclusive span length in floors.
    pub flags: i64,
    /// Height-based cost metric used by the routing pathfinder.
    pub height_metric: i64,
    pub entry_floor: i64,
    pub reserved_byte: i64,
    pub descending_load_counter: i64,
    pub ascending_load_counter: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecialLinkRecord {
    pub active: bool,
    pub lower_floor: i64,
    pub upper_floor: i64,
    pub reachability_masks_by_floor: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferGroupEntry {
    pub active: bool,
    pub tagged_floor: i64,
    pub carrier_mask: i64,
}

// ─── PlacedObjectRecord ───────────────────────────────────────────────────────

/// Per-object simulation record for every placed non-infrastructure tile.
/// Keyed in WorldState.placedObjects by "anchorX,y".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedObjectRecord {
    /// Leftmost tile x (anchor column).
    pub left_tile_index: i64,
    /// Rightmost tile x (anchor x + width − 1).
    pub right_tile_index: i64,
    /// The original game's family code (e.g. FAMILY_HOTEL_SINGLE, FAMILY_RESTAURANT).
    pub object_type_code: i64,
    /// Per-family lifecycle byte (unit status / open-close state).
    pub unit_status: i64,
    /// Runtime cycle counter for the per-family refresh rotation.
    pub aux_value_or_timer: i64,
    /// Index into WorldState.sidecars; −1 when no sidecar is attached.
    pub linked_record_index: i64,
    /// Binary PlacedObjectRecord +0x13 (byte): "dirty" / cashflow-needed flag.
    /// Set to 1 by most activation paths (office/retail/condo/hotel
    /// `activate_*_cashflow`, stay-phase advance/decrement/sync, service-request
    /// allocate/release, parking/coverage recalc, placement init). Read by the
    /// ledger rollover sweep to know which placed objects need a cashflow pass
    /// in the current 3-day cycle. Independent of operational scoring.
    pub dirty_flag: i64,
    /// Binary PlacedObjectRecord +0x14 (byte): "occupied" / scored flag.
    /// Set by the scoring sweep (`recompute_object_operational_status` at
    /// 1138:09d6) once `evalLevel > 0`, and by placement init. Cleared by
    /// `deactivate_*_cashflow` paths and `handle_extended_vacancy_expiry`.
    /// Read by the office state-0x20 morning gate at 1228:1df3 and the retail
    /// state-0x20 gate at 1228:4044 (both are RNG gates that skip the dispatch
    /// unless the object has been scored).
    pub occupied_flag: i64,
    /// Operational rating: 0 = bad/refund-eligible, 1 = ok, 2 = good. −1 until first scoring sweep.
    pub eval_level: i64,
    /// Raw average stress score (per-occupant trip stress, before rent/noise
    /// modifiers and threshold bucketing). -1 when no occupant has logged a
    /// trip yet. Display-only; evalLevel is the value that drives game logic.
    pub eval_score: i64,
    /// Pricing tier 0–3 (0 = best payout, 3 = worst); 4 = no payout.
    pub rent_level: i64,
    /// Cumulative activation count, capped.
    pub activation_tick_count: i64,
    /// Housekeeping has claimed this room for turnover service.
    pub housekeeping_claimed_flag: i64,
    /// VIP suite flag normalized onto standard hotel room types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vip_flag: Option<bool>,
}

// ─── Gate flags ───────────────────────────────────────────────────────────────

/// Global simulation gate flags. These drive the per-star qualitative
/// advancement conditions and recycling-center adequacy state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateFlags {
    /// Purpose unresolved; initialized to all-ones.
    pub unknown_c198: i64,
    /// Set when a metro object is placed.
    pub metro_placed: i64,
    /// Top/anchor floor index of the metro stack; -1 when absent.
    pub metro_station_floor_index: i64,
    /// Set when an office object is placed.
    pub office_placed: i64,
    /// Set when a security-office (family 0x0e) object is placed. 2→3 gate.
    pub security_placed: i64,
    /// Updated by office-service evaluation every 9th day.
    pub office_service_ok: i64,
    /// Daily "office medical service ok" flag; latched at day-start (star > 2),
    /// cleared on any failed medical trip, gates star 3→4 and 4→5 advancement.
    pub office_service_ok_medical: i64,
    /// Set by update_recycling_center_state.
    pub recycling_adequate: i64,
    /// Set by the facility rebuild pipeline once enough routes exist.
    pub routes_viable: i64,
    /// Floor index of placed VIP suite; 0xffff = none.
    pub vip_suite_floor: i64,
    /// Cathedral/evaluation placed-object index/floor sentinel; 0xffff = none.
    pub eval_sim_index: i64,
    /// Number of placed recycling-center upper slices.
    pub recycling_center_count: i64,
    /// Set every 8 days while the tower is below 5-star rank.
    pub facility_progress_override: i64,
    /// Daily hotel sale counter, reset at the morning sale checkpoint.
    pub family345_sale_count: i64,
    /// Display/news trigger latch used by hotel checkout milestones.
    pub newspaper_trigger: i64,
}

pub fn create_gate_flags() -> GateFlags {
    GateFlags {
        unknown_c198: 0xffffffff,
        metro_placed: 0,
        metro_station_floor_index: -1,
        office_placed: 0,
        security_placed: 0,
        office_service_ok: 0,
        office_service_ok_medical: 0,
        recycling_adequate: 0,
        routes_viable: 0,
        vip_suite_floor: 0xffff, // −1: no VIP suite
        eval_sim_index: 0xffff,  // −1: no cathedral placed
        recycling_center_count: 0,
        facility_progress_override: 0,
        family345_sale_count: 0,
        newspaper_trigger: 0,
    }
}

// ─── Sidecar records ──────────────────────────────────────────────────────────

/// (`kind: "commercial_venue"`: the tag lives on `SidecarRecord`.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommercialVenueRecord {
    /// 0xff = invalid / demolished.
    pub owner_subtype_index: i64,
    pub capacity: i64,
    pub visit_count: i64,
    /// Today's STAFF-EMIT count (binary record+0x7). Written ONLY by
    /// `try_consume_commercial_venue_capacity` (11b0:11c3 `INC byte +0x7`),
    /// capped against `remainingCapacity` (binary +0x6). Rolled at daily
    /// reseed: `yesterdayVisitCount = todayVisitCount; todayVisitCount = 0`.
    /// `yesterdayVisitCount` (binary +0x8) feeds the population/star ledger
    /// via `add_to_primary_family_ledger_bucket`. NOT used by closure
    /// cashflow — that path reads `acquireCount` (binary +0x10).
    pub today_visit_count: i64,
    pub yesterday_visit_count: i64,
    /// Visitor acquisition count (binary record+0x10, word). Incremented by
    /// BOTH `try_consume_commercial_venue_capacity` (11b0:11cd
    /// `INC word +0x10`) at MORNING_GATE AND
    /// `acquire_commercial_venue_slot` (11b0:0ee3 `INC word +0x10`) on the
    /// arrival success path when the visitor's type/variant differs from
    /// the venue owner's. NOT capped by `remainingCapacity`. Reset to 0 at
    /// daily reseed. Read by `seed_facility_runtime_link_state` →
    /// `derive_commercial_venue_state_code` for closure cashflow bands.
    pub acquire_count: i64,
    pub availability_state: i64,
    /// Number of sims currently inside the venue (binary offset +0x09,
    /// "active_assignment_count"). Incremented by acquireCommercialVenueSlot
    /// when a sim takes a slot, decremented by releaseCommercialVenueSlot on
    /// departure. Saturates at 39 in the binary; when > 39 new arrivals are
    /// queued rather than admitted.
    pub current_population: i64,
    /// dayTick of the most recent slot acquisition (binary offset +0x0a).
    /// Used by releaseCommercialVenueSlot to gate the minimum dwell time
    /// against the family's service-duration ticks before a departure is
    /// accepted.
    pub last_acquire_tick: i64,
    /// Signed eligibility threshold (binary offset +0x0c). When negative, the
    /// sim's current state word must not exceed `1 - eligibilityThreshold` for
    /// tryConsumeCommercialVenueCapacity to succeed. Prevents rapid re-visits.
    pub eligibility_threshold: i64,
    /// Remaining daily slots (binary offset +0x06). Refilled from the active
    /// phase seed at daypart 0 by rebuildCommercialVenueRuntime for fast-food
    /// and retail (restaurants use a separate midday mechanism). Decremented on
    /// each successful dispatch.
    pub remaining_capacity: i64,
    /// Phase A capacity seed (binary offset +0x03). Grows by +2 (low stress)
    /// or +1 (medium stress) after each sim visit via clampVenueSeed, and is
    /// consumed/cleared by the daily rebuild. Used when calendar_phase == 0.
    pub phase_a_seed: i64,
    /// Phase B capacity seed (binary offset +0x04). Used when calendar_phase != 0.
    pub phase_b_seed: i64,
    /// Override capacity seed (binary offset +0x05). Used when
    /// facility_progress_override is active.
    pub override_seed: i64,
}

/// Per-family runtime path buckets. Mirrors the binary's
/// `g_retail_shop_bucket_table` / `g_restaurant_bucket_table` /
/// `g_fast_food_bucket_table` (each 7 zone rows × variable slot count).
/// Each row holds sidecar indices appended by `append_facility_path_bucket_entry`
/// (11b0:161b) when the venue is placed/recomputed. The selector
/// (`select_random_commercial_venue_record_from_bucket`, 11b0:1361) reads from
/// the requested zone row, falling back to row 0 when the requested row is
/// empty, then validates the picked record's availabilityState post-pick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommercialVenueBuckets {
    pub retail: Vec<Vec<i64>>,
    pub restaurant: Vec<Vec<i64>>,
    pub fast_food: Vec<Vec<i64>>,
}

pub const COMMERCIAL_VENUE_BUCKET_ROWS: i64 = 7;

pub fn create_commercial_venue_buckets() -> CommercialVenueBuckets {
    let empty = || -> Vec<Vec<i64>> { (0..COMMERCIAL_VENUE_BUCKET_ROWS).map(|_| Vec::new()).collect() };
    CommercialVenueBuckets {
        retail: empty(),
        restaurant: empty(),
        fast_food: empty(),
    }
}

// CommercialVenueRecord.availabilityState values
pub const VENUE_AVAILABLE: i64 = 0;
pub const VENUE_PARTIAL: i64 = 1; // active_assignment_count 1..9
pub const VENUE_NEAR_FULL: i64 = 2; // active_assignment_count >= 10
pub const VENUE_CLOSED: i64 = 3; // daily off-hours closure
pub const VENUE_DORMANT: i64 = 0xff; // inactive

/// (`kind: "service_request"`: the tag lives on `SidecarRecord`.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRequestEntry {
    pub owner_subtype_index: i64,
    /// Floor index of the service provider (used by parking demand log).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_index: Option<i64>,
    /// 0 = uncovered / active, 1 = covered / suppressed by ramp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_flag: Option<i64>,
}

/// (`kind: "entertainment_link"`: the tag lives on `SidecarRecord`.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntertainmentLinkRecord {
    pub owner_subtype_index: i64,
    /// 0xff = no pair yet.
    pub paired_subtype_index: i64,
    /// 0xff for single-venue records; 0..13 paired selector bucket.
    pub family_selector_or_single_link_flag: i64,
    /// Incremented at 0x0f0 each day; saturates at 0x7f.
    pub link_age_counter: i64,
    /// Upper-half attendance budget (seeded at 0x0f0).
    pub upper_budget: i64,
    /// Lower-half attendance budget (seeded at 0x0f0).
    pub lower_budget: i64,
    /// 0=idle, 1=activated, 2=attendance started, 3=ready.
    pub link_phase_state: i64,
    /// Reserved flag used by the placement/runtime pipeline.
    pub pending_transition_flag: i64,
    /// Cumulative attendance this cycle.
    pub attendance_counter: i64,
    /// Active runtime attendee count (decremented on phase advance).
    pub active_runtime_count: i64,
    /// Binary `EntertainmentLinkRecord.lowerHalfFloor` (offset +0x03).
    /// Floor index of the lower half of the venue. Used as routing source by
    /// `get_entertainment_link_routing_source_floor` (1188:0dce) for
    /// entertainment guest service-acquire / linked-half / dwell-return
    /// routing, and as the destination for party hall in
    /// `get_entertainment_link_venue_floor` (1188:0d98).
    pub lower_half_floor: i64,
    /// Binary `EntertainmentLinkRecord.upperHalfFloor` (offset +0x00). Floor
    /// index of the upper half of the venue. Returned by
    /// `get_entertainment_link_venue_floor` (1188:0d98) for cinema (when
    /// `familySelectorOrSingleLinkFlag` is non-negative as a signed byte).
    pub upper_half_floor: i64,
}

/// (`kind: "medical_center"`: the tag lives on `SidecarRecord`.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MedicalCenterRecord {
    /// 0xff = invalid / demolished.
    pub owner_subtype_index: i64,
    /// Count of office-worker sims currently queued at this center.
    pub pending_visitors_count: i64,
}

/// Upstream's union of the four record interfaces, told apart by `kind`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SidecarRecord {
    #[serde(rename = "commercial_venue")]
    CommercialVenue(CommercialVenueRecord),
    #[serde(rename = "service_request")]
    ServiceRequest(ServiceRequestEntry),
    #[serde(rename = "entertainment_link")]
    EntertainmentLink(EntertainmentLinkRecord),
    #[serde(rename = "medical_center")]
    MedicalCenter(MedicalCenterRecord),
}

/// Global medical service-request slot. Binary allocates 10 fixed slots of
/// (source_floor, subtype_index, retry_counter, _pad); first-fit scan on
/// allocation, retry counter overflows at 0x28 (40) forcing the visit to
/// resolve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MedicalServiceSlot {
    pub active: bool,
    /// Sim key (simKey()) of the queued office worker.
    pub sim_id: String,
    /// Floor the worker is queued from.
    pub source_floor: i64,
    /// Sidecar index of the target medical center.
    pub target_sidecar_index: i64,
    /// Per-tick retry counter; hits 40 → forced serve.
    pub retry_counter: i64,
}

pub const MAX_MEDICAL_SERVICE_SLOTS: i64 = 10;
pub const MEDICAL_RETRY_OVERFLOW: i64 = 0x28;

pub fn create_medical_service_slots() -> Vec<MedicalServiceSlot> {
    (0..MAX_MEDICAL_SERVICE_SLOTS)
        .map(|_| MedicalServiceSlot {
            active: false,
            sim_id: String::new(),
            source_floor: -1,
            target_sidecar_index: -1,
            retry_counter: 0,
        })
        .collect()
}

// ─── Event state ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventState {
    /// Active-event bitfield:
    /// bit 0 = bomb active search, bit 3 = fire active,
    /// bit 5 = bomb found, bit 6 = bomb detonated.
    pub game_state_flags: i64,
    /// Floor where the bomb was placed.
    pub bomb_floor: i64,
    /// Tile where the bomb was placed.
    pub bomb_tile: i64,
    /// Day tick deadline for bomb detonation / post-resolution cleanup.
    pub bomb_deadline: i64,
    /// Floor where the fire started.
    pub fire_floor: i64,
    /// Tile column where fire starts (right_tile - 0x20).
    pub fire_tile: i64,
    /// Day tick when fire started.
    pub fire_start_tick: i64,
    /// Per-floor left-spreading fire front position (120 entries, 0xffff = inactive).
    pub fire_left_pos: Vec<i64>,
    /// Per-floor right-spreading fire front position (120 entries, 0xffff = inactive).
    pub fire_right_pos: Vec<i64>,
    /// Rescue countdown (with emergency coverage); 0 = no countdown active.
    pub rescue_countdown: i64,
    /// Helicopter extinguish position; 0 = not active.
    pub helicopter_extinguish_pos: i64,
    /// LCG15 state for event randomness.
    pub lcg_state: i64,
    /// Deterministic bomb-search helper floor bounds / cursor state.
    pub bomb_search_lower_bound: i64,
    pub bomb_search_upper_bound: i64,
    pub bomb_search_current_floor: i64,
    pub bomb_search_scan_tile: i64,
    /// Pending carrier-edit prompt target column, -1 when idle.
    pub pending_carrier_edit_column: i64,
    /// Pending carrier-edit prompt target y, -1 when idle.
    pub pending_carrier_edit_y: i64,
    /// When true, suppress triggerRandomNewsEvent (trace-test only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_news_events: Option<bool>,
    /// Firefighter rescue helpers, spawned by the binary's
    /// `initialize_service_response_entities(8)` (1100:033d) when the player
    /// declines the helicopter rescue prompt. The binary repurposes the slot-0
    /// occupant of each placed security office; we store an abstract record per
    /// helper here. Each helper walks right-to-left at one tile per tick on its
    /// spawn floor; when its column enters the fire's arrival window
    /// `[left, left+6) ∪ [right+6, right+12)` it pauses for an extinguish-delay
    /// wind-up then calls the asymmetric front clear. Drained when fire
    /// resolves (binary `initialize_service_response_entities(0)`).
    pub fire_rescue_helpers: Vec<FireRescueHelper>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FireRescueHelper {
    /// Internal floor (binary index) the helper patrols. Set on spawn from the
    /// helper's home security office's floor.
    pub floor: i64,
    /// Current tile column. Decrements toward 0 each `walk_delay` tick.
    pub column: i64,
    /// Helper status: 0=walking, 1=winding-up extinguish, 2=relocating to
    /// another floor with active fire, 3=idle/done.
    pub status: i64,
    /// Wind-up countdown (ticks remaining until front clear fires).
    pub windup_remaining: i64,
}

pub fn create_event_state() -> EventState {
    EventState {
        game_state_flags: 0,
        bomb_floor: 0,
        bomb_tile: 0,
        bomb_deadline: 0,
        fire_floor: 0,
        fire_tile: 0,
        fire_start_tick: 0,
        fire_left_pos: vec![0xffff; GRID_HEIGHT as usize],
        fire_right_pos: vec![0xffff; GRID_HEIGHT as usize],
        rescue_countdown: 0,
        helicopter_extinguish_pos: 0,
        lcg_state: 1,
        bomb_search_lower_bound: -1,
        bomb_search_upper_bound: -1,
        bomb_search_current_floor: -1,
        bomb_search_scan_tile: -1,
        pending_carrier_edit_column: -1,
        pending_carrier_edit_y: -1,
        disable_news_events: None,
        fire_rescue_helpers: Vec::new(),
    }
}

// ─── WorldState ───────────────────────────────────────────────────────────────

/// All placed tile data for one tower.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldState {
    pub tower_id: String,
    pub name: String,
    pub width: i64,
    pub height: i64,
    /// Lobby slice height in floors; defaults to 1 until expanded-lobby support exists.
    pub lobby_height: i64,
    /// Lobby placement / express-stop spacing mode.
    ///   "perfect-parity": sky lobbies at floors 0, 14, 29, 44, ... (binary parity).
    ///   "modern": sky lobbies at floors 0, 15, 30, 45, ... (regular every-15 cadence).
    pub lobby_mode: LobbyMode,
    /// Global simulation gate flags (star advancement, recycling adequacy, etc.).
    pub gate_flags: GateFlags,
    /// "x,y" → tileType for every occupied cell (anchors and extensions alike).
    pub cells: JsObject<String>,
    /// Extension cell key → anchor cell key.
    pub cell_to_anchor: JsObject<String>,
    /// Anchor cell key → overlay tileType (e.g. "stairs").
    pub overlays: JsObject<String>,
    /// Extension cell key → anchor cell key for overlays.
    pub overlay_to_anchor: JsObject<String>,
    /// "anchorX,y" → PlacedObjectRecord for every simulated (non-infrastructure)
    /// placed tile. Infrastructure tiles (floor, lobby, stairs) do not have records.
    pub placed_objects: JsObject<PlacedObjectRecord>,
    /// Sidecar records, indexed by PlacedObjectRecord.linkedRecordIndex.
    pub sidecars: Vec<SidecarRecord>,
    /// Runtime sim population rebuilt from placed objects.
    pub sims: Vec<SimRecord>,
    /// One CarrierRecord per elevator/escalator shaft. Rebuilt from cells on mutation.
    pub carriers: Vec<CarrierRecord>,
    /// Special-link segment table (max MAX_SPECIAL_LINKS entries). Rebuilt from carriers.
    pub special_links: Vec<SpecialLinkSegment>,
    /// Special-link record table (max MAX_SPECIAL_LINK_RECORDS entries).
    pub special_link_records: Vec<SpecialLinkRecord>,
    /// Per-floor walkability bitmask (bit 0 = Escalator-branch, bit 1 = Stairs-branch). Size = GRID_HEIGHT.
    pub floor_walkability_flags: Vec<i64>,
    /// Tagged transfer-concourse entries (max MAX_TRANSFER_GROUPS entries).
    pub transfer_group_entries: Vec<TransferGroupEntry>,
    /// Per-floor bitmask of carrier IDs that serve each floor. Size = GRID_HEIGHT.
    pub transfer_group_cache: Vec<i64>,
    /// Sidecar indices of uncovered parking spaces feeding the demand log.
    pub parking_demand_log: Vec<i64>,
    /// Global medical service-request slots (10 fixed slots, first-fit).
    pub medical_service_slots: Vec<MedicalServiceSlot>,
    /// LCG state for general-purpose simulation randomness.
    pub rng_state: i64,
    /// Cumulative count of sampleRng calls (for trace alignment).
    pub rng_call_count: i64,
    /// 1–6 (6 = Tower).
    pub star_count: i64,
    /// Binary `g_primary_family_ledger_total` @ 1288:c13a. Population-weighted
    /// activation total used by `compute_tower_tier_from_ledger` (1148:041d) to
    /// gate star advancement, AND surfaced as the player-facing population
    /// count. Updated via `add_to_primary_family_ledger_bucket` (1068:07f7) and
    /// `clear_primary_family_ledger_bucket` (1068:07b3) on the binary's
    /// family-ledger events:
    ///   - office (family 7) activate/deactivate: ±6
    ///   - hotel single (family 3) activate/deactivate: ±1
    ///   - hotel twin/suite (family 4/5) activate/deactivate: ±2
    ///   - condo (family 9) sale/refund: ±3
    ///   - retail (family 10) activation: +10
    ///   - restaurant/fastfood (family 6/12) and entertainment families: rebuilt
    ///     daily from runtime budgets (clear+seed pattern).
    pub current_population: i64,
    /// Binary `g_per_family_ledger_buckets` @ DS:0xc112 — per-family slot
    /// accumulators that sum to `currentPopulation`. Mirrors the binary's
    /// invariant: `currentPopulation == sum(currentPopulationBuckets)`.
    /// `add_to_primary_family_ledger_bucket` updates bucket+total;
    /// `clear_primary_family_ledger_bucket` subtracts bucket from total then
    /// zeroes the bucket. Used to net out daily resets of fast-food/retail/
    /// restaurant per-day visit-count contributions: clear-then-add-yesterday
    /// gives a delta of (newVisits - oldVisits) on the running total.
    pub current_population_buckets: JsObject<i64>,
    /// Per-family path buckets used by entertainment guests / hotel guests when
    /// picking a commercial venue. Maintained in sync with the binary's
    /// `recompute_facility_runtime_state` (11b0:02f2) and
    /// `append_facility_path_bucket_entry` (11b0:161b). Rebuilt at venue
    /// placement/demolition (`runGlobalRebuilds`) and at the daily restock
    /// checkpoints (tick 240 fast-food/retail, tick 1600 restaurant).
    pub commercial_venue_buckets: CommercialVenueBuckets,
    /// Bomb/fire/VIP event state.
    pub event_state: EventState,
    /// Pending notifications emitted during the current tick (drained by the transport layer).
    pub pending_notifications: Vec<SimNotification>,
    /// Pending prompts requiring player response (drained by the transport layer).
    pub pending_prompts: Vec<SimPrompt>,
    // BOX: car elevator -- not in upstream: the BOX additions' state
    // (box_ext/), left out of a snapshot while empty so that a tower that
    // does not use them saves exactly as upstream's does.
    #[serde(default, skip_serializing_if = "crate::box_ext::BoxExtState::is_empty")]
    pub box_ext: crate::box_ext::BoxExtState,
    /// Not in upstream's WorldState: the state of sims/population.ts's
    /// module-level `simOwnerCache` (a WeakMap from each sim object to the
    /// placed object it first found).  Runtime only, never saved: upstream's
    /// cache is empty after `structuredClone` and after rebuildRuntimeSims
    /// makes new sim objects.  See SimOwnerCache.
    #[serde(skip)]
    pub sim_owner_cache: SimOwnerCache,
    /// The objects sims still hold after they left `placed_objects` (deleted
    /// or replaced): reached as `world.object(key)`, never by iteration.
    #[serde(skip)]
    pub detached_objects: indexmap::IndexMap<String, PlacedObjectRecord>,
}

/// What findObjectForSim's WeakMap holds, kept beside `world.sims` (entry i
/// is sim i's owner key).  Once the object a sim holds is deleted or
/// replaced in `world.placedObjects`, JavaScript's sim goes on reading and
/// writing the old object, which nothing else can see: here the sim's key
/// then names a copy in `world.detached_objects` (WorldState::detach_object).
///
/// Interior mutability is the one exception to TRANSLATION.md's rules:
/// upstream fills the cache from read-only paths too (scoring's records),
/// and those take `&WorldState`.
#[derive(Clone, Debug, Default)]
pub struct SimOwnerCache {
    owners: std::cell::RefCell<Vec<Option<String>>>,
}

/// A detached object's key: never a placed object's ("x,y").
pub const DETACHED_PREFIX: &str = "detached:";

impl PartialEq for SimOwnerCache {
    /// The cache is not part of the tower's state.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl SimOwnerCache {
    pub fn get(&self, sim_index: usize) -> Option<String> {
        self.owners.borrow().get(sim_index).cloned().flatten()
    }

    pub fn set(&self, sim_index: usize, key: String) {
        let mut owners = self.owners.borrow_mut();
        if owners.len() <= sim_index {
            owners.resize(sim_index + 1, None);
        }
        owners[sim_index] = Some(key);
    }

    /// `world.sims` was replaced by new sim objects: nothing is cached.
    pub fn clear(&self) {
        self.owners.borrow_mut().clear();
    }

    /// `world.sims.filter(keep)`: the survivors keep their entries.
    pub fn retain(&self, keep: &[bool]) {
        let mut owners = self.owners.borrow_mut();
        let old = std::mem::take(&mut *owners);
        *owners = old
            .into_iter()
            .chain(std::iter::repeat(None))
            .zip(keep.iter())
            .filter(|(_, k)| **k)
            .map(|(o, _)| o)
            .collect();
    }

    fn holds(&self, key: &str) -> bool {
        self.owners.borrow().iter().any(|o| o.as_deref() == Some(key))
    }

    fn rename(&self, from: &str, to: &str) {
        for owner in self.owners.borrow_mut().iter_mut() {
            if owner.as_deref() == Some(from) {
                *owner = Some(to.to_string());
            }
        }
    }
}

impl WorldState {
    /// The placed object at `key`, or the detached object a sim holds.
    pub fn object(&self, key: &str) -> Option<&PlacedObjectRecord> {
        if key.starts_with(DETACHED_PREFIX) {
            self.detached_objects.get(key)
        } else {
            self.placed_objects.get(key)
        }
    }

    pub fn object_mut(&mut self, key: &str) -> Option<&mut PlacedObjectRecord> {
        if key.starts_with(DETACHED_PREFIX) {
            self.detached_objects.get_mut(key)
        } else {
            self.placed_objects.get_mut(key)
        }
    }

    /// Call before `placed_objects[key]` is deleted or replaced: the sims
    /// that hold the object now hold it detached.
    pub fn detach_object(&mut self, key: &str) {
        if !self.sim_owner_cache.holds(key) {
            return;
        }
        let Some(object) = self.placed_objects.get(key).cloned() else {
            return;
        };
        let new_key = format!("{DETACHED_PREFIX}{}", self.detached_objects.len());
        self.detached_objects.insert(new_key.clone(), object);
        self.sim_owner_cache.rename(key, &new_key);
    }

    /// `world.sims` was replaced by new sim objects (rebuildRuntimeSims).
    pub fn forget_sim_owners(&mut self) {
        self.sim_owner_cache.clear();
        self.detached_objects.clear();
    }
}

/// `SimNotification.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimNotificationKind {
    RouteFailure,
    Event,
    StarAdvanced,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimNotification {
    pub kind: SimNotificationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `SimPrompt.promptKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimPromptKind {
    BombRansom,
    FireRescue,
    CarrierEditConfirmation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimPrompt {
    pub prompt_id: String,
    pub prompt_kind: SimPromptKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_rows() {
        assert!(is_valid_lobby_y(GROUND_Y, LobbyMode::Modern));
        assert!(is_valid_lobby_y(GROUND_Y - 15, LobbyMode::Modern));
        assert!(!is_valid_lobby_y(GROUND_Y - 14, LobbyMode::Modern));
        assert!(is_valid_lobby_y(GROUND_Y - 14, LobbyMode::PerfectParity));
        assert!(!is_valid_lobby_y(GROUND_Y + 1, LobbyMode::PerfectParity));
        assert_eq!(y_to_floor(GROUND_Y), UNDERGROUND_FLOORS);
        assert_eq!(floor_to_y(UNDERGROUND_FLOORS), GROUND_Y);
    }

    #[test]
    fn rng_matches_upstream() {
        // Upstream: s = (Math.imul(s, 0x15a4e35) + 1) | 0; (s >>> 16) & 0x7fff,
        // computed here directly in 32-bit arithmetic.
        let mut s: i64 = 1;
        let mut expected = Vec::new();
        for _ in 0..1000 {
            s = ((s as i32).wrapping_mul(0x15a4e35).wrapping_add(1)) as i64;
            expected.push(((s as u32) >> 16) as i64 & 0x7fff);
        }
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        let got: Vec<i64> = (0..1000).map(|_| sample_rng(&mut w)).collect();
        assert_eq!(got, expected);
        assert_eq!(got[..3], [346, 130, 10982]); // node's first three

        assert_eq!(w.rng_call_count, 1000);
        assert_eq!(w.rng_state, s);
    }

    #[test]
    fn enums_serialise_with_upstream_tags() {
        let r = RouteState::Carrier { carrier_id: 2, direction: RouteDirection::Down, source: 10 };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"mode":"carrier","carrierId":2,"direction":"down","source":10}"#
        );
        assert_eq!(serde_json::to_string(&RouteState::Idle).unwrap(), r#"{"mode":"idle"}"#);
        let s = SidecarRecord::MedicalCenter(MedicalCenterRecord { owner_subtype_index: 3, pending_visitors_count: 0 });
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"kind":"medical_center","ownerSubtypeIndex":3,"pendingVisitorsCount":0}"#
        );
        let p = SimPrompt { prompt_id: "fire_3".into(), prompt_kind: SimPromptKind::CarrierEditConfirmation, message: "m".into(), cost: None };
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"promptId":"fire_3","promptKind":"carrier_edit_confirmation","message":"m"}"#
        );
        assert_eq!(serde_json::to_string(&LobbyMode::PerfectParity).unwrap(), r#""perfect-parity""#);
        let g = serde_json::to_value(create_gate_flags()).unwrap();
        assert_eq!(g["unknownC198"], 0xffffffffu32);
        assert_eq!(g["family345SaleCount"], 0);
    }
}

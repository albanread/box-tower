// ts: resources.ts
//
// Upstream's constant `Record`s and `Set`s are slices here, in JavaScript
// property order (array-index keys ascending, then the others as written):
// `TABLE[key]` is `js::record_get(TABLE, key)`, `SET.has(x)` is
// `SET.contains(&x)`.

pub const STARTING_CASH: i64 = 2_000_000;

// ─── Tile registry ────────────────────────────────────────────────────────────

/// Width in grid cells for each placeable tile type.
pub const TILE_WIDTHS: &[(&str, i64)] = &[
    // Infrastructure
    ("floor", 1),
    ("lobby", 1),
    ("stairs", 8),
    ("elevator", 4),
    ("elevatorExpress", 6),
    ("elevatorService", 4),
    ("escalator", 8),
    // Hotels (families 3/4/5)
    ("hotelSingle", 4),
    ("hotelTwin", 6),
    ("hotelSuite", 10),
    // Commercial (families 6/0x0a/0x0c)
    ("restaurant", 24),
    ("fastFood", 16),
    ("retail", 12),
    // Office (family 7)
    ("office", 9),
    // Condo (family 9)
    ("condo", 16),
    // Entertainment (families 0x12/0x1d)
    ("cinema", 31),
    ("partyHall", 27),
    ("cathedral", 28),
    // Services
    ("recyclingCenter", 25),      // virtual two-floor stack placement
    ("recyclingCenterUpper", 25), // family 0x14 upper slice
    ("recyclingCenterLower", 25), // family 0x15 lower slice
    ("parking", 4),               // family 0x18
    ("parkingRamp", 15),          // family 0x2c
    ("security", 16),             // family 0x0e
    ("metro", 30),                // families 0x1f/0x20/0x21, three-floor stack
    ("housekeeping", 15),         // family 0x0f
    ("medical", 26),              // family 0x0d
];

/// One-time construction cost in dollars.
pub const TILE_COSTS: &[(&str, i64)] = &[
    ("floor", 500),
    ("lobby", 3_000),
    ("stairs", 5_000),
    ("elevator", 200_000),
    ("elevatorExpress", 200_000),
    ("elevatorService", 200_000),
    ("escalator", 20_000),
    ("hotelSingle", 20_000),
    ("hotelTwin", 50_000),
    ("hotelSuite", 100_000),
    ("restaurant", 200_000),
    ("fastFood", 100_000),
    ("retail", 100_000),
    ("office", 40_000),
    ("condo", 80_000),
    ("cinema", 500_000),
    ("partyHall", 100_000),
    ("cathedral", 3_000_000),
    ("recyclingCenter", 500_000), // combined cost
    ("recyclingCenterUpper", 500_000),
    ("recyclingCenterLower", 0),
    ("parking", 5_000),
    ("parkingRamp", 50_000),
    ("security", 100_000),
    ("metro", 45_000),
    ("housekeeping", 50_000),
    ("medical", 500_000),
];

/// One-time cost to add an extra car to a carrier, indexed by carrier mode.
pub const CARRIER_CAR_CONSTRUCTION_COST: &[(i64, i64)] = &[(0, 150_000), (1, 80_000), (2, 50_000)];

/// Per-floor cost charged when extending an existing carrier shaft up or
/// down by one floor. Binary path: `FUN_10a8_0819` (extend_up) /
/// `FUN_10a8_0b87` (extend_down) → `charge_floor_range_construction_cost`
/// (1180:02e5), which sums `width * tile_rate` per newly-served floor.
/// Observed delta in the mixed_elevator_delayed trace: $15,000 for a
/// 3-floor extension of a standard (width=4) shaft → $5,000/floor.
pub const CARRIER_EXTEND_FLOOR_COST: &[(i64, i64)] = &[
    (0, 7_500), // express (width 6)
    (1, 5_000), // standard (width 4)
    (2, 5_000), // service (width 4)
];

/// Minimum star rating required before the binary exposes a build-menu entry.
/// Tiles omitted from this table are treated as always available.
pub const TILE_STAR_REQUIREMENTS: &[(&str, i64)] = &[
    ("lobby", 1),
    ("floor", 1),
    ("stairs", 1),
    ("elevator", 1),
    ("office", 1),
    ("fastFood", 1),
    ("condo", 1),
    ("elevatorService", 2),
    ("hotelSingle", 2),
    ("hotelTwin", 2),
    ("hotelSuite", 2),
    ("housekeeping", 2),
    ("security", 2),
    ("escalator", 3),
    ("elevatorExpress", 3),
    ("restaurant", 3),
    ("retail", 3),
    ("partyHall", 3),
    ("cinema", 3),
    ("parking", 3),
    ("parkingRamp", 3),
    ("recyclingCenter", 3),
    ("recyclingCenterUpper", 3),
    ("recyclingCenterLower", 3),
    ("medical", 3),
    ("metro", 4),
    ("cathedral", 5),
];

pub fn get_tile_star_requirement(tile_type: &str) -> i64 {
    // BOX: car elevator -- not in upstream's table.
    if tile_type == crate::box_ext::car_elevator::CAR_ELEVATOR_TILE {
        return crate::box_ext::car_elevator::CAR_ELEVATOR_STAR_REQUIREMENT;
    }
    // BOX: amenities -- not in upstream's table either.
    if let Some(n) = crate::box_ext::amenities::star_requirement(tile_type) {
        return n;
    }
    crate::js::record_get(TILE_STAR_REQUIREMENTS, tile_type)
        .copied()
        .unwrap_or(1)
}

/// `new Set(Object.keys(TILE_WIDTHS))`: the same keys in the same order
/// (a unit test holds the two together).
pub const VALID_TILE_TYPES: &[&str] = &[
    "floor",
    "lobby",
    "stairs",
    "elevator",
    "elevatorExpress",
    "elevatorService",
    "escalator",
    "hotelSingle",
    "hotelTwin",
    "hotelSuite",
    "restaurant",
    "fastFood",
    "retail",
    "office",
    "condo",
    "cinema",
    "partyHall",
    "cathedral",
    "recyclingCenter",
    "recyclingCenterUpper",
    "recyclingCenterLower",
    "parking",
    "parkingRamp",
    "security",
    "metro",
    "housekeeping",
    "medical",
];

/// Tiles that may be placed at or below the underground row. Everything else is
/// rejected by `handlePlaceTile` when `y >= UNDERGROUND_Y`. Mirrors the original
/// The original game's build-menu restrictions: support/transport, parking/recycling,
/// selected public facilities, and entertainment venues may go underground.
pub const UNDERGROUND_ALLOWED_TILES: &[&str] = &[
    "floor",
    "stairs",
    "escalator",
    "elevator",
    "elevatorExpress",
    "elevatorService",
    "parking",
    "parkingRamp",
    "metro",
    "recyclingCenter",
    "recyclingCenterUpper",
    "recyclingCenterLower",
    "restaurant",
    "fastFood",
    "retail",
    "security",
    "partyHall",
    "cinema",
    "medical",
];

// ─── Family codes (object-type codes from the spec) ─────────────────────────

pub const FAMILY_ELEVATOR: i64 = 1;
pub const FAMILY_ESCALATOR: i64 = 2;
pub const FAMILY_HOTEL_SINGLE: i64 = 3;
pub const FAMILY_HOTEL_TWIN: i64 = 4;
pub const FAMILY_HOTEL_SUITE: i64 = 5;
pub const FAMILY_RESTAURANT: i64 = 6;
pub const FAMILY_OFFICE: i64 = 7;
pub const FAMILY_CONDO: i64 = 9;
pub const FAMILY_RETAIL: i64 = 10;
pub const FAMILY_FAST_FOOD: i64 = 12;
pub const FAMILY_SECURITY: i64 = 14;
pub const FAMILY_CINEMA: i64 = 18;
pub const FAMILY_CINEMA_LOWER: i64 = 19;
pub const FAMILY_RECYCLING_CENTER_UPPER: i64 = 20;
pub const FAMILY_RECYCLING_CENTER_LOWER: i64 = 21;
pub const FAMILY_PARKING: i64 = 24;
pub const FAMILY_PARKING_RAMP: i64 = 44; // 0x2c
pub const FAMILY_METRO_TOP: i64 = 0x1f;
pub const FAMILY_METRO_MIDDLE: i64 = 0x20;
pub const FAMILY_METRO_BOTTOM: i64 = 0x21;
pub const FAMILY_PARTY_HALL: i64 = 29;
pub const FAMILY_PARTY_HALL_LOWER: i64 = 30;
pub const FAMILY_CINEMA_STAIRS_UPPER: i64 = 34;
pub const FAMILY_CINEMA_STAIRS_LOWER: i64 = 35;
pub const FAMILY_CATHEDRAL_ANCHOR: i64 = 0x21;
pub const FAMILY_HOUSEKEEPING: i64 = 15;
pub const FAMILY_MEDICAL: i64 = 13;
// Cathedral placed objects occupy 5 type codes (one per cathedral floor slice).
// Runtime visitors all use family 0x24 and share the parking-style state
// machine at 1228:5b5a / 1228:5cd2, routing through 1228:5ddd / 1228:5e7e.
pub const FAMILY_CATHEDRAL_BASE: i64 = 0x24;
pub const FAMILY_CATHEDRAL_MAX: i64 = 0x28;

// ─── Family code ↔ tile name mappings ────────────────────────────────────────

/// Maps the original game's family/object-type codes to internal tile name strings.
/// (In ascending code order, as JavaScript iterates a numeric-keyed object.)
pub const FAMILY_CODE_TO_TILE: &[(i64, &str)] = &[
    (FAMILY_ELEVATOR, "elevator"),
    (FAMILY_ESCALATOR, "escalator"),
    (FAMILY_HOTEL_SINGLE, "hotelSingle"),
    (FAMILY_HOTEL_TWIN, "hotelTwin"),
    (FAMILY_HOTEL_SUITE, "hotelSuite"),
    (FAMILY_RESTAURANT, "restaurant"),
    (FAMILY_OFFICE, "office"),
    (FAMILY_CONDO, "condo"),
    (FAMILY_RETAIL, "retail"),
    (FAMILY_FAST_FOOD, "fastFood"),
    (FAMILY_MEDICAL, "medical"),
    (FAMILY_SECURITY, "security"),
    (FAMILY_HOUSEKEEPING, "housekeeping"),
    (FAMILY_CINEMA, "cinema"),
    (FAMILY_RECYCLING_CENTER_UPPER, "recyclingCenterUpper"),
    (FAMILY_RECYCLING_CENTER_LOWER, "recyclingCenterLower"),
    (FAMILY_PARKING, "parking"),
    (FAMILY_PARTY_HALL, "partyHall"),
    (FAMILY_METRO_TOP, "metro"),
    (FAMILY_METRO_MIDDLE, "metro"),
    (FAMILY_CATHEDRAL_ANCHOR, "cathedral"),
    (FAMILY_PARKING_RAMP, "parkingRamp"),
];

pub const LEGACY_VIP_TILE_TO_STANDARD: &[(&str, &str)] = &[
    ("vipSingle", "hotelSingle"),
    ("vipTwin", "hotelTwin"),
    ("vipSuite", "hotelSuite"),
];

pub const LEGACY_TILE_ALIASES: &[(&str, &str)] = &[];

/// Upstream computes this from FAMILY_CODE_TO_TILE (inverted, in its
/// iteration order, so "metro" keeps its first place and its last code,
/// FAMILY_METRO_MIDDLE) and adds `lobby: FAMILY_PARKING`.  Written out;
/// a unit test checks it against FAMILY_CODE_TO_TILE.
pub const TILE_TO_FAMILY_CODE: &[(&str, i64)] = &[
    ("elevator", 1),
    ("escalator", 2),
    ("hotelSingle", 3),
    ("hotelTwin", 4),
    ("hotelSuite", 5),
    ("restaurant", 6),
    ("office", 7),
    ("condo", 9),
    ("retail", 10),
    ("fastFood", 12),
    ("medical", 13),
    ("security", 14),
    ("housekeeping", 15),
    ("cinema", 18),
    ("recyclingCenterUpper", 20),
    ("recyclingCenterLower", 21),
    ("parking", 24),
    ("partyHall", 29),
    ("metro", 32),
    ("cathedral", 33),
    ("parkingRamp", 44),
    ("lobby", FAMILY_PARKING),
];

// ─── Cinema movie titles ─────────────────────────────────────────────────────
// Resource RT_TYPE_32518 (name 0x81a4, file offset 0xb9a00): 15 Pascal strings.
// Index 14 ("Under the Apple Tree") is unreachable: placement seeds with
// rand()%14 (0..13) and the picker formulas wrap inside [0..6] and [7..13].
// See specs/facility/ENTERTAINMENT.md → Movie Identity.

pub const MOVIE_TITLES: &[&str] = &[
    "Revenge of the Big Spider",
    "Northwest Romance",
    "Samurai Cop",
    "Big Wave",
    "Farewell to Morocco",
    "Fear of Shark Teeth",
    "Western Sheriff",
    "Dino Wars",
    "The Making of a Star",
    "Love in N.Y.",
    "Waikiki Moon",
    "My Man of War",
    "Christmas for Both of Us",
    "Casual Friends",
];

pub const CINEMA_NEW_MOVIE_COST: i64 = 300_000;
pub const CINEMA_CLASSIC_MOVIE_COST: i64 = 150_000;

// ─── YEN #1001 — payout table ─────────────────────────────────────────────────
// Income per checkout/activation event, indexed by variant tier (0=best, 3=worst).
//
// The values hold fractions (in units of ¥1,000), so they are f64; every
// one times YEN_UNIT (1,000) is an exact integer, which is how they are used.

pub const YEN_1001: &[(&str, [f64; 4])] = &[
    ("hotelSingle", [3.0, 2.0, 1.5, 0.5]),
    ("hotelTwin", [4.5, 3.0, 2.0, 0.8]),
    ("hotelSuite", [9.0, 6.0, 4.0, 1.5]),
    ("office", [15.0, 10.0, 5.0, 2.0]),
    ("condo", [200.0, 150.0, 100.0, 40.0]),
    ("retail", [20.0, 15.0, 10.0, 4.0]),
];

// ─── Commercial closure payouts (derive_commercial_venue_state_code) ────────
// Per-venue payout at daily closure, keyed by visitor-count band.
// Bands: <25, 25..34, 35..49, >=50. Values in YEN_UNIT (×1000).
// Source: 11b0:1731 — restaurant (==6) and fast food (==0xc); retail returns 0.

pub const COMMERCIAL_CLOSURE_PAYOUTS: &[(&str, [i64; 4])] = &[
    ("restaurant", [-6, 4, 6, 10]),
    ("fastFood", [-3, 2, 3, 5]),
    ("retail", [0, 0, 0, 0]),
];

pub const COMMERCIAL_CLOSURE_BANDS: [i64; 3] = [25, 35, 50];

// ─── Per-type capacity tuning caps ──────────────────────────────────────────
// Source: FUN_11b0_17d3 — returns per-type per-phase capacity ceiling.
// [phaseA, phaseB, override]. The daily rebuild caps the seed at this value,
// then floors the result at 10.

pub const COMMERCIAL_CAPACITY_CAPS: &[(i64, [i64; 3])] = &[
    (FAMILY_RESTAURANT, [35, 50, 25]),
    (FAMILY_RETAIL, [25, 30, 18]),
    (FAMILY_FAST_FOOD, [35, 50, 25]),
];

// ─── YEN #1002 — expense table ────────────────────────────────────────────────
// Operating expenses charged every 3 days, indexed by family code.

// Binary-verified YEN resource #1002. Raw table (family code → value):
// [1]=100, [14]=200, [15]=100, [20]=500, [22]=0, [27]=50, [31]=1000,
// [42]=200, [43]=100, [44]=100 (all others 0). Values here are raw/10 so
// that `value * YEN_UNIT (=1000)` matches the binary's cash_balance × 100
// trace scale (YEN_1001 uses the same raw/10 convention).
pub const QUARTERLY_EXPENSES: &[(i64, i64)] = &[
    (1, 10),  // elevatorLocal, per active car per 3-day period
    (14, 20), // security
    (15, 10), // housekeeping
    (20, 50), // recyclingCenterUpper
    (27, 5),  // escalator
    (42, 20), // elevatorExpress
    (43, 10), // elevatorService
];

// ─── Operational score thresholds ─────────────────────────────────────────────
// [low_threshold, high_threshold] → pairing_status 0/1/2 (C/B/A)

pub const OP_SCORE_THRESHOLDS: &[(i64, [i64; 2])] = &[
    (1, [80, 150]),
    (2, [80, 150]),
    (3, [80, 150]),
    (4, [80, 200]),
    (5, [80, 200]),
];

// ─── Parking expense rates ──────────────────────────────────────────────────
// Per-star tier rate in $100 units: expense = (width) * rate / 10.
// Stars <3 → 0, star 3 → 30, stars >=4 → 100.

pub const PARKING_EXPENSE_RATE_BY_STAR: &[(i64, i64)] = &[(1, 0), (2, 0), (3, 30), (4, 100), (5, 100)];

// ─── Activity score star thresholds ──────────────────────────────────────────
// score must exceed STAR_THRESHOLDS[star - 1] to advance from star → star+1

pub const STAR_THRESHOLDS: [i64; 5] = [300, 1000, 5000, 10_000, 15_000];

// ─── Route delay values ───────────────────────────────────────────────────────
// All confirmed from startup tuning resource (type 0xff05, id 1000).

pub const DELAY_WAITING: i64 = 5; // carrier floor-slot status 0x28 (at-capacity)
pub const DELAY_REQUEUE_FAIL: i64 = 0; // assign_request_to_runtime_route finds no transfer floor
pub const DELAY_ROUTE_FAIL: i64 = 300; // select_best_route_candidate returns < 0
pub const DELAY_VENUE_UNAVAIL: i64 = 0; // target venue slot invalid / no path-seed entry
pub const DELAY_STOP_ESCALATOR: i64 = 16; // Escalator-branch per-stop delay
pub const DELAY_STOP_STAIRS: i64 = 35; // Stairs-branch per-stop delay

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::record_get;

    #[test]
    fn derived_tables_match_their_sources() {
        let keys: Vec<&str> = TILE_WIDTHS.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, VALID_TILE_TYPES);
        // Object.fromEntries over FAMILY_CODE_TO_TILE, inverted, then lobby.
        let mut inverted: Vec<(&str, i64)> = Vec::new();
        for &(code, tile) in FAMILY_CODE_TO_TILE {
            match inverted.iter_mut().find(|(t, _)| *t == tile) {
                Some(e) => e.1 = code,
                None => inverted.push((tile, code)),
            }
        }
        inverted.push(("lobby", FAMILY_PARKING));
        assert_eq!(inverted, TILE_TO_FAMILY_CODE);
        // Numeric-keyed tables are in ascending order, as JavaScript iterates them.
        for t in [CARRIER_CAR_CONSTRUCTION_COST, QUARTERLY_EXPENSES, PARKING_EXPENSE_RATE_BY_STAR] {
            assert!(t.windows(2).all(|w| w[0].0 < w[1].0));
        }
        assert!(FAMILY_CODE_TO_TILE.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(COMMERCIAL_CAPACITY_CAPS.windows(2).all(|w| w[0].0 < w[1].0));
        for (_, p) in YEN_1001 {
            for v in p {
                assert_eq!((v * 1000.0).fract(), 0.0);
            }
        }
        assert_eq!(get_tile_star_requirement("metro"), 4);
        assert_eq!(get_tile_star_requirement("nothing"), 1);
        assert_eq!(record_get(QUARTERLY_EXPENSES, 22), None);
    }
}

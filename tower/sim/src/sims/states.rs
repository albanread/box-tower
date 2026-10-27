// ts: sims/states.ts

use crate::resources::{
    FAMILY_CINEMA, FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE,
    FAMILY_HOTEL_TWIN, FAMILY_HOUSEKEEPING, FAMILY_OFFICE, FAMILY_PARTY_HALL_LOWER,
    FAMILY_RECYCLING_CENTER_UPPER, FAMILY_RESTAURANT, FAMILY_RETAIL, FAMILY_SECURITY,
};
use crate::world::RouteState;

/// Internal floor-slot index for the lobby (world floor 0 + UNDERGROUND_FLOORS).
pub const LOBBY_FLOOR: i64 = 10;
pub const EVAL_ZONE_FLOOR: i64 = 109; // floor 0x6d

pub const STATE_TRANSIT_FLAG: i64 = 0x40;
pub const STATE_BASE_MASK: i64 = 0x3f;

pub const fn with_transit_flag(base_state_code: i64) -> i64 {
    STATE_TRANSIT_FLAG | base_state_code
}

pub const fn state_base_code(state_code: i64) -> i64 {
    state_code & STATE_BASE_MASK
}

pub const fn has_transit_flag(state_code: i64) -> bool {
    (state_code & STATE_TRANSIT_FLAG) != 0
}

pub const STATE_COMMUTE: i64 = 0x00; // commuting to destination
pub const STATE_ACTIVE: i64 = 0x01; // active / in-stay / venue selection
pub const STATE_ACTIVE_ALT: i64 = 0x02; // office alternate lunch/venue-selection state
pub const STATE_ARRIVED: i64 = 0x03; // arrived at destination
pub const STATE_CHECKOUT_QUEUE: i64 = 0x04; // hotel checkout queue (non-last sibling)
pub const STATE_DEPARTURE: i64 = 0x05; // departing / returning
pub const STATE_TRANSITION: i64 = 0x10; // unit status transition (hotel checking out)
pub const STATE_MORNING_GATE: i64 = 0x20; // morning activation gate
pub const STATE_AT_WORK: i64 = 0x21; // at work (office, post-commute)
pub const STATE_VENUE_TRIP: i64 = 0x22; // commercial venue trip in transit
const STATE_DWELL_RETURN: i64 = 0x23;
pub const STATE_HOTEL_PARKED: i64 = 0x24; // hotel parked (awaiting guest)
pub const STATE_NIGHT_A: i64 = 0x25; // night park variant A
pub const STATE_NIGHT_B: i64 = 0x26; // night park / venue unavailable
pub const STATE_PARKED: i64 = 0x27; // parked / idle

pub const STATE_COMMUTE_TRANSIT: i64 = with_transit_flag(STATE_COMMUTE);
pub const STATE_ACTIVE_TRANSIT: i64 = with_transit_flag(STATE_ACTIVE);
pub const STATE_VENUE_TRIP_TRANSIT: i64 = with_transit_flag(STATE_VENUE_TRIP);
pub const STATE_DEPARTURE_TRANSIT: i64 = with_transit_flag(STATE_DEPARTURE);
pub const STATE_EVAL_RETURN: i64 = STATE_DEPARTURE_TRANSIT;
pub const STATE_EVAL_OUTBOUND: i64 = with_transit_flag(STATE_MORNING_GATE);
pub const STATE_MORNING_TRANSIT: i64 = STATE_EVAL_OUTBOUND;
pub const STATE_AT_WORK_TRANSIT: i64 = with_transit_flag(STATE_AT_WORK);
pub const STATE_VENUE_HOME_TRANSIT: i64 = with_transit_flag(STATE_VENUE_TRIP);
pub const STATE_DWELL_RETURN_TRANSIT: i64 = with_transit_flag(STATE_DWELL_RETURN);

pub const UNIT_STATUS_OFFICE_OCCUPIED: i64 = 0x0f;
pub const UNIT_STATUS_CONDO_OCCUPIED: i64 = 0x17;
pub const UNIT_STATUS_CONDO_VACANT: i64 = 0x18;
pub const UNIT_STATUS_CONDO_VACANT_EVENING: i64 = 0x20;
pub const UNIT_STATUS_HOTEL_SOLD_OUT: i64 = 0x37;

pub const ROUTE_IDLE: RouteState = RouteState::Idle;

pub const NO_EVAL_ENTITY: i64 = 0xffff;
pub const ENTITY_REFRESH_STRIDE: i64 = 16;
pub const ACTIVATION_TICK_CAP: i64 = 0x78;

// A numeric-keyed `Record`: JavaScript iterates it in ascending code order
// whatever order upstream writes it in, so the pairs are in that order here
// (FAMILY_SECURITY, 14, before FAMILY_HOUSEKEEPING, 15, and so on), with
// upstream's comments beside their entries.  `js::record_get` looks one up.
pub const ENTITY_POPULATION_BY_TYPE: &[(i64, i64)] = &[
    (FAMILY_HOTEL_SINGLE, 2),
    (FAMILY_HOTEL_TWIN, 3),
    (FAMILY_HOTEL_SUITE, 3),
    (FAMILY_RESTAURANT, 48),
    (FAMILY_OFFICE, 6),
    (FAMILY_CONDO, 3),
    (FAMILY_RETAIL, 48),
    (FAMILY_FAST_FOOD, 48),
    (FAMILY_SECURITY, 6),
    // Housekeeping helpers (family 0x0f).
    (FAMILY_HOUSEKEEPING, 6),
    // Cinema primary (0x12) — binary allocates 56 guest sims for the theater
    // span; sidecar phase handling drives attendance/payout state.
    (FAMILY_CINEMA, 56),
    (FAMILY_RECYCLING_CENTER_UPPER, 6),
    // Party hall lower (0x1e) — 40-slot occupant span; upper half (0x1d) is
    // never activated and gets no sims. Binary `get_span_size_for_family` = 0x28.
    (FAMILY_PARTY_HALL_LOWER, 40),
    // Cathedral object slices: 5 floor types x 8 slots = 40 guests. Runtime
    // family is normalized to 0x24 when sims are rebuilt.
    (36, 8), // 0x24
    (37, 8), // 0x25
    (38, 8), // 0x26
    (39, 8), // 0x27
    (40, 8), // 0x28
];

pub const HOTEL_FAMILIES: &[i64] = &[FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_HOTEL_SUITE];

/// Families whose placed objects carry an evaluation score (rentable occupancy).
pub const EVALUATABLE_FAMILIES: &[i64] = &[
    FAMILY_HOTEL_SINGLE,
    FAMILY_HOTEL_TWIN,
    FAMILY_HOTEL_SUITE,
    FAMILY_OFFICE,
    FAMILY_CONDO,
];

pub const COMMERCIAL_FAMILIES: &[i64] = &[FAMILY_RESTAURANT, FAMILY_FAST_FOOD, FAMILY_RETAIL];

pub const CATHEDRAL_FAMILIES: &[i64] = &[0x24, 0x25, 0x26, 0x27, 0x28];

/// Binary entity-table allocation order, derived from reference trace hex codes.
/// Sims must be sorted by this priority so stride residues match the binary.
///
/// (A numeric-keyed `Record`: in ascending code order, as JavaScript
/// iterates it; each entry keeps upstream's comment.)
pub const BINARY_ALLOC_ORDER: &[(i64, i64)] = &[
    (FAMILY_HOTEL_SINGLE, 8),  // 0xfd
    (FAMILY_HOTEL_TWIN, 6),    // 0xfb
    (FAMILY_HOTEL_SUITE, 7),   // 0xfc
    (FAMILY_RESTAURANT, 1),    // 0xf4
    (FAMILY_OFFICE, 4),        // 0xf9
    (FAMILY_CONDO, 3),         // 0xf7
    (FAMILY_RETAIL, 5),        // 0xfa
    (FAMILY_FAST_FOOD, 2),     // 0xf6
    (FAMILY_HOUSEKEEPING, 14),
    (FAMILY_RECYCLING_CENTER_UPPER, 0), // 0xf2
    // Cathedral families (36–40) come after all main families.
    (36, 9),
    (37, 10),
    (38, 11),
    (39, 12),
    (40, 13),
];

pub const ELEVATOR_DEMAND_STATES: &[i64] = &[
    STATE_COMMUTE,
    STATE_ACTIVE,
    STATE_ACTIVE_ALT,
    STATE_CHECKOUT_QUEUE,
    STATE_DEPARTURE,
    STATE_VENUE_TRIP,
    STATE_COMMUTE_TRANSIT,
    STATE_ACTIVE_TRANSIT,
    STATE_VENUE_TRIP_TRANSIT,
    STATE_DEPARTURE_TRANSIT,
    STATE_EVAL_OUTBOUND,
    // STATE_EVAL_RETURN is STATE_DEPARTURE_TRANSIT (a Set holds it once).
    STATE_AT_WORK_TRANSIT,
    // STATE_VENUE_HOME_TRANSIT is STATE_VENUE_TRIP_TRANSIT (likewise).
    STATE_DWELL_RETURN_TRANSIT,
];

// Housekeeping helper (family 0x0f) state codes.
pub const HK_STATE_SEARCH: i64 = 0;
pub const HK_STATE_ROUTE_TO_CANDIDATE: i64 = 1;
pub const HK_STATE_COUNTDOWN: i64 = 2;
pub const HK_STATE_ROUTE_TO_TARGET: i64 = 3;
pub const HK_STATE_ROUTE_TO_CANDIDATE_TRANSIT: i64 = 4;
pub const HK_SEARCHING_SENTINEL: i64 = -1;
pub const HK_POST_CLAIM_COUNTDOWN: i64 = 3;
pub const HK_CLAIM_DAY_TICK_CUTOFF: i64 = 1500;
pub const HK_FLOOR_CLASS_MOD: i64 = 6;

// Sentinel used by carrier car slot cleanup and unset cathedral owner indices.
pub const INVALID_FLOOR: i64 = 0xff;
pub const COMMERCIAL_VENUE_DWELL_TICKS: i64 = 60;
// The binary encodes this as 0x62, which also has the transit bit set.
pub const COMMERCIAL_DWELL_STATE: i64 = STATE_VENUE_HOME_TRANSIT;
/// Binary `select_random_commercial_venue_record_from_bucket` (11b0:1361)
/// service_family_selector mapping: 0 = retail, 1 = restaurant, 2 = fast-food.
/// The condo state-0x01/0x41 handler (1228:3b34–3b54) selects:
///   weekend_flag == 0               → 0 (retail)
///   weekend_flag != 0, slot % 4 == 0 → 1 (restaurant)
///   weekend_flag != 0, slot % 4 != 0 → 2 (fast-food)
pub const CONDO_SELECTOR_RETAIL: &[i64] = &[FAMILY_RETAIL];
pub const CONDO_SELECTOR_RESTAURANT: &[i64] = &[FAMILY_RESTAURANT];
pub const CONDO_SELECTOR_FAST_FOOD: &[i64] = &[FAMILY_FAST_FOOD];
/// Binary hotel state-0x01 handler (1228:3126) pushes selector=1 (restaurant)
/// into route_sim_to_commercial_venue, so hotel rooms only ever visit
/// restaurants. Retail/fast-food buckets are reserved for the hotel-guest
/// (family 0x21) path via `select_random_venue_bucket_for_hotel_guest`.
pub const HOTEL_ROOM_SELECTOR: &[i64] = &[FAMILY_RESTAURANT];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transit_aliases() {
        assert_eq!(STATE_MORNING_TRANSIT, 0x60);
        assert_eq!(STATE_EVAL_RETURN, 0x45);
        assert_eq!(COMMERCIAL_DWELL_STATE, 0x62);
        assert_eq!(STATE_VENUE_HOME_TRANSIT, STATE_VENUE_TRIP_TRANSIT);
        assert_eq!(STATE_DWELL_RETURN_TRANSIT, 0x63);
        assert_eq!(state_base_code(0x62), 0x22);
        assert!(has_transit_flag(0x41) && !has_transit_flag(0x21));
    }

    #[test]
    fn numeric_records_are_in_ascending_order() {
        for t in [ENTITY_POPULATION_BY_TYPE, BINARY_ALLOC_ORDER] {
            assert!(t.windows(2).all(|w| w[0].0 < w[1].0));
        }
        assert_eq!(crate::js::record_get(BINARY_ALLOC_ORDER, FAMILY_RECYCLING_CENTER_UPPER), Some(&0));
        assert_eq!(crate::js::record_get(BINARY_ALLOC_ORDER, 38), Some(&11));
        assert_eq!(BINARY_ALLOC_ORDER.len(), 15);
    }
}

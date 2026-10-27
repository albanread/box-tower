// ts: families/state-tables/hotel.ts

// Binary table: cs:1c41 dispatch_sim_behavior family 3/4/5 (hotel/restaurant/
// fast-food) branch. ROUTING-BINARY-MAP.md §4.2.
//
// These are Map<state_code, HandlerFn> tables. The handlers live in
// sims/hotel.ts. Binary quirk: in-transit states {0x41, 0x45, 0x60, 0x62}
// run decrementRouteQueueDirectionLoad as a prologue (gated in
// families/hotel.ts#refreshObjectFamilyHotelStateHandler), then dispatch
// through HOTEL_REFRESH_HANDLER_TABLE which maps them to the same handler
// as their base state.

// Upstream re-exports `HOTEL_REFRESH_HANDLER_TABLE` and `type HotelHandler`
// from sims/hotel.ts here.  Nothing imports them through this file; the
// re-export is added once sims/hotel.rs defines them:
// pub use crate::sims::hotel::{HotelHandler, HOTEL_REFRESH_HANDLER_TABLE};

/// Shared family 3/4/5 dispatch_sim_behavior table (cs:1c41).
///
/// (A constant `ReadonlyMap` is a slice of pairs in insertion order;
/// `js::record_get` looks one up.)
pub const HOTEL_PROLOGUE_TABLE: &[(i64, &str)] = &[
    (0x41, "1228:1a4f"),
    (0x45, "1228:19f4"),
    (0x60, "1228:1a4f"),
    (0x62, "1228:1a4f"),
    // else (everything not listed above) → 1228:1c24
];

pub const HOTEL_PROLOGUE_DEFAULT: &str = "1228:1c24";

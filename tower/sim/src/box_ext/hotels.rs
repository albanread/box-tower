//! BOX addition (BOX RULES only): hotel rooms fill by their appeal
//! (docs/amenities.md, "Hotels by appeal").
//!
//! Upstream lets every hotel room that is not rated badly every night.
//! Under BOX RULES a room is booked on a night by chance: BASE_BOOKING in a
//! hundred, more for a good rating and for each kind of amenity a guest
//! values that the tower has open (laundry, guest services, a pool, a
//! convention centre, a premier lounge).  A room not booked stays empty
//! that night, as upstream's suites do below three stars; so amenities pay
//! through the hotel's takings as well as its ratings.
//!
//! Hook in sims/hotel.rs's morning gate (`// BOX: hotels`): `stays_empty`.

use crate::box_ext::{amenities, box_rules, fnv};
use crate::resources::{FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN};
use crate::world::WorldState;

/// A night's chance of a booking, in a hundred, before the extras.
pub const BASE_BOOKING: i64 = 70;
/// More for a room rated good.
pub const GOOD_ROOM: i64 = 10;
/// More for each kind of amenity guests value, open in the tower.
pub const PER_AMENITY: i64 = 5;
const GUEST_AMENITIES: [&str; 5] = ["laundry", "guestServices", "pool", "convention", "premierLounge"];

/// The chance of a booking a night, in a hundred, of the hotel room at
/// `key` (None: not a hotel room, or BOX RULES off).
pub fn booking_rate(world: &WorldState, key: &str) -> Option<i64> {
    if !box_rules(world) {
        return None;
    }
    let o = world.placed_objects.get(key)?;
    if ![FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_HOTEL_SUITE].contains(&o.object_type_code) {
        return None;
    }
    let kinds = GUEST_AMENITIES
        .iter()
        .filter(|k| world.box_ext.amenities.rooms.iter().any(|r| r.tile == **k && r.built_day != amenities::NEW))
        .count() as i64;
    let good = if o.eval_level == 2 { GOOD_ROOM } else { 0 };
    Some((BASE_BOOKING + good + kinds * PER_AMENITY).min(100))
}

/// Hook in the hotel morning gate: the room at `key` has no booking
/// tonight (day `day`).
pub fn stays_empty(world: &WorldState, key: &str, day: i64) -> bool {
    match booking_rate(world, key) {
        Some(rate) => fnv(&format!("book/{key}/{day}")) % 100 >= rate,
        None => false,
    }
}

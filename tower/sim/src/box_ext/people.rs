//! BOX addition (BOX RULES): who lives in the tower, who works in it, and
//! where the children go to school (docs/people.md).
//!
//! The 1994 game counts a population: six to an office, three to a
//! condominium, a hotel room's guests.  It does not say who they are.  The
//! register does, from the tower as it stands: each condominium is a
//! household (adults and children), every room open creates jobs, some of
//! the tower's adults hold those jobs and the rest of the staff come in from
//! outside, and the children go to the tower's kindergarten and primary
//! school while there are places, or to school outside.
//!
//! Nothing here is saved or changes the simulation: it is worked out again
//! whenever it is asked for, the same every time for the same tower (the
//! choices are hashes of the rooms' places).

use crate::box_ext::{amenities, box_rules, fnv};
use crate::resources::{
    FAMILY_CINEMA, FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN,
    FAMILY_HOUSEKEEPING, FAMILY_MEDICAL, FAMILY_METRO_TOP, FAMILY_OFFICE, FAMILY_PARKING_RAMP, FAMILY_PARTY_HALL,
    FAMILY_RECYCLING_CENTER_UPPER, FAMILY_RESTAURANT, FAMILY_RETAIL, FAMILY_SECURITY,
};
use crate::world::WorldState;

/// Places a kindergarten has, and a primary school.
pub const KINDERGARTEN_PLACES: i64 = 30;
pub const SCHOOL_PLACES: i64 = 150;

/// The kinds of work, in the order the pane lists them.
pub const TRADES: [&str; 9] =
    ["Offices", "Catering", "Shops", "Hotel", "Entertainment", "Services", "Health", "Education", "Leisure"];

/// The register: households and the people in them, the jobs the tower
/// gives, who holds them, and the children's schools.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Register {
    pub households: i64,
    pub adults: i64,
    pub children: i64,
    /// hotel guests staying tonight (they live elsewhere)
    pub guests: i64,
    /// jobs by trade (TRADES' order)
    pub jobs: [i64; 9],
    pub jobs_total: i64,
    /// jobs held by people who live in the tower; the rest commute in
    pub held_by_residents: i64,
    pub commuters: i64,
    /// the tower's adults working outside it, and at home (retired, caring,
    /// between jobs)
    pub working_outside: i64,
    pub at_home: i64,
    pub kindergarten_here: i64,
    pub kindergarten_outside: i64,
    pub school_here: i64,
    pub school_outside: i64,
}

/// The jobs one room gives: (trade, how many)
fn jobs_of(family: i64) -> Option<(usize, i64)> {
    Some(match family {
        FAMILY_OFFICE => (0, 6),
        FAMILY_RESTAURANT => (1, 6),
        FAMILY_FAST_FOOD => (1, 4),
        FAMILY_RETAIL => (2, 3),
        FAMILY_CINEMA => (4, 5),
        FAMILY_PARTY_HALL => (4, 4),
        FAMILY_SECURITY => (5, 6),
        FAMILY_HOUSEKEEPING => (3, 4),
        FAMILY_RECYCLING_CENTER_UPPER => (5, 4),
        FAMILY_METRO_TOP => (5, 4),
        FAMILY_PARKING_RAMP => (5, 1),
        FAMILY_MEDICAL => (6, 6),
        _ => return None,
    })
}

/// An amenity's staff: (trade, how many)
fn amenity_jobs(tile: &str) -> (usize, i64) {
    match tile {
        "laundry" => (3, 3),
        "guestServices" => (3, 3),
        "premierLounge" => (1, 4),
        "pool" => (8, 3),
        "gym" => (8, 2),
        "convention" => (4, 6),
        "kindergarten" => (7, 4),
        "school" => (7, 12),
        _ => (5, 1),
    }
}

/// A household of three (upstream's condominium): two adults and a child,
/// one adult and two children, or three adults, by its place
pub fn household(key: &str) -> (i64, i64) {
    match fnv(key) % 4 {
        0 | 3 => (2, 1),
        1 => (1, 2),
        _ => (3, 0),
    }
}

/// The register of the tower (empty with BOX RULES off).
pub fn register(world: &WorldState) -> Register {
    let mut r = Register::default();
    if !box_rules(world) {
        return r;
    }
    let mut kids: Vec<(i64, bool)> = Vec::new(); // (hash, under five)
    let mut adults: Vec<i64> = Vec::new();
    let mut hotel_rooms = 0;
    for (key, o) in world.placed_objects.iter() {
        let code = o.object_type_code;
        if code == FAMILY_CONDO && o.unit_status <= 0x17 {
            let (a, c) = household(key);
            r.households += 1;
            r.adults += a;
            r.children += c;
            for i in 0..a {
                adults.push(fnv(&format!("{key}/a{i}")));
            }
            for i in 0..c {
                let h = fnv(&format!("{key}/c{i}"));
                kids.push((h, h % 10 < 4));
            }
            continue;
        }
        if matches!(code, FAMILY_HOTEL_SINGLE | FAMILY_HOTEL_TWIN | FAMILY_HOTEL_SUITE) {
            hotel_rooms += 1;
            if o.unit_status < 0x18 {
                r.guests += if code == FAMILY_HOTEL_SINGLE { 1 } else { 2 };
            }
            continue;
        }
        let open = code != FAMILY_OFFICE || o.unit_status <= 0x0f;
        if let (Some((trade, n)), true) = (jobs_of(code), open) {
            r.jobs[trade] += n;
        }
    }
    // the hotel's own staff: a front desk and room service, one to six rooms
    r.jobs[3] += (hotel_rooms + 5) / 6;
    // amenities open, and their places for the children
    let mut kinder_places = 0;
    let mut school_places = 0;
    for room in &world.box_ext.amenities.rooms {
        if room.built_day == amenities::NEW {
            continue;
        }
        let (trade, n) = amenity_jobs(&room.tile);
        r.jobs[trade] += n;
        match room.tile.as_str() {
            "kindergarten" => kinder_places += KINDERGARTEN_PLACES,
            "school" => school_places += SCHOOL_PLACES,
            _ => {}
        }
    }
    // a car elevator's attendant
    r.jobs[5] += world.box_ext.car_elevators.len() as i64;
    r.jobs_total = r.jobs.iter().sum();

    // the tower's adults: four in ten would work in the tower, three in ten
    // outside, three at home; those who would work here and find no job
    // left work outside
    let mut left = r.jobs_total;
    adults.sort_unstable();
    for h in adults {
        match h % 10 {
            0..=3 if left > 0 => {
                r.held_by_residents += 1;
                left -= 1;
            }
            0..=6 => r.working_outside += 1,
            _ => r.at_home += 1,
        }
    }
    r.commuters = r.jobs_total - r.held_by_residents;

    // the children: the under-fives to the kindergarten, the rest to the
    // primary school, while there are places; outside the tower otherwise
    kids.sort_unstable();
    for (_, little) in kids {
        if little {
            if kinder_places > 0 {
                kinder_places -= 1;
                r.kindergarten_here += 1;
            } else {
                r.kindergarten_outside += 1;
            }
        } else if school_places > 0 {
            school_places -= 1;
            r.school_here += 1;
        } else {
            r.school_outside += 1;
        }
    }
    r
}

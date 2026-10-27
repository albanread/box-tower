//! BOX additions: features of our own that the 1994 game and upstream's
//! translation do not have.  None of this is a translation; the translated
//! files reach it only through hooks marked `// BOX: ...`, each of which
//! does nothing while the feature is unused, so that a tower which does not
//! use it runs exactly as upstream's does (same RNG draws, same iteration
//! orders, same snapshot JSON).
//!
//! The state lives in `WorldState::box_ext`, which a snapshot writes only
//! when it is not empty.
//!
//! Two kinds of addition:
//! - things the player builds (the car elevator), which act only once built;
//! - BOX RULES (`box_rules`), a world option that changes the game's rules:
//!   parking that makes sense, car-elevator busyness, medical incidents.  It
//!   is on for towers made for the BOX (`TowerSim::create_box`), off for
//!   every trace fixture, the lockstep harness and any classic game, and off
//!   when an upstream snapshot is loaded.  It has its own random generator
//!   (`rng`), never the world's LCG, so that with it off nothing changes.

use serde::{Deserialize, Serialize};

use crate::world::WorldState;

pub mod amenities;
pub mod people;
pub mod rating;
pub mod car_elevator;
pub mod emergency;
pub mod escapes;
pub mod fire;
pub mod hotels;
pub mod locals;
pub mod metro;
pub mod street;

pub use car_elevator::{CarElevator, Vehicle};

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// Every BOX addition's saved state.  Field names are camelCase without
/// underscores, so that snapshot.rs's snake_case migration leaves them be.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxExtState {
    /// BOX RULES on.  Saved only when on.
    #[serde(default, skip_serializing_if = "is_false")]
    pub box_rules: bool,
    /// BOX RULES' own random generator's state (box_random).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rng: i64,
    /// Car elevators, in the order they were built.
    #[serde(default)]
    pub car_elevators: Vec<CarElevator>,
    /// Vehicles on the street, in the car elevators and in the bays, in the
    /// order they arrived.
    #[serde(default)]
    pub vehicles: Vec<Vehicle>,
    /// The next vehicle's id.
    #[serde(default)]
    pub next_vehicle_id: i64,
    /// Commuters driving in, held until their cars have parked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commuters: Vec<car_elevator::Commuter>,
    /// Cars that came to park today, and the parking fees taken, all told.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub parked_today: i64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub parking_income: i64,
    /// This three-day period's parking fees, amenities' takings and their
    /// upkeep (the finances pane's, as upstream's ledgers).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub period_parking: i64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub period_amenity_income: i64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub period_amenity_upkeep: i64,
    /// Medical incidents (BOX RULES only).
    #[serde(default, skip_serializing_if = "emergency::EmergencyState::is_empty")]
    pub emergency: emergency::EmergencyState,
    /// Amenities (BOX RULES only): the rooms and their tallies.
    #[serde(default, skip_serializing_if = "amenities::AmenitiesState::is_empty")]
    pub amenities: amenities::AmenitiesState,
    /// The fire engines and police cars in the street: saved under BOX
    /// RULES, runtime only (a picture) in a classic game.
    #[serde(default, skip_serializing_if = "street::StreetState::not_saved")]
    pub street: street::StreetState,
    /// Office workers who live in the tower (BOX RULES only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locals: Vec<locals::Local>,
    /// A fire's evacuation and closed floors (BOX RULES only).
    #[serde(default, skip_serializing_if = "fire::FireState::is_empty")]
    pub fire: fire::FireState,
}

impl BoxExtState {
    /// Nothing built and nothing remembered: the snapshot leaves the field out.
    pub fn is_empty(&self) -> bool {
        !self.box_rules
            && self.rng == 0
            && self.car_elevators.is_empty()
            && self.vehicles.is_empty()
            && self.next_vehicle_id == 0
            && self.commuters.is_empty()
            && self.parked_today == 0
            && self.parking_income == 0
            && self.period_parking == 0
            && self.period_amenity_income == 0
            && self.period_amenity_upkeep == 0
            && self.emergency.is_empty()
            && self.amenities.is_empty()
            && self.fire.is_empty()
            && self.locals.is_empty()
    }
}

/// 32-bit FNV-1a: deterministic choices from names.
pub fn fnv(s: &str) -> i64 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h as i64
}

/// BOX RULES on or off.  Turning them on seeds their random generator from
/// the tower (its id), unless it already runs.
pub fn set_box_rules(world: &mut WorldState, on: bool) {
    world.box_ext.box_rules = on;
    world.box_ext.street.kept = on;
    if on && world.box_ext.rng == 0 {
        world.box_ext.rng = fnv(&world.tower_id) | 1;
    }
}

/// Hook beside upstream's ledger rollover: a new three-day period starts
/// the BOX ledgers afresh, as do_ledger_rollover does upstream's.
pub fn period_rollover(world: &mut WorldState, day_counter: i64) {
    if day_counter % 3 == 0 {
        let b = &mut world.box_ext;
        b.period_parking = 0;
        b.period_amenity_income = 0;
        b.period_amenity_upkeep = 0;
    }
}

/// BOX RULES: what each person already waiting adds to a lift's route
/// cost (route_scoring::select_candidate): ten of them make it as dear as
/// sixteen cells further to walk.
pub const QUEUE_COST_PER_PERSON: i64 = 13;
/// BOX RULES: the walk to a lift or stairs that costs nothing, in cells;
/// each cell beyond is a tick of the trip's stress (sims/scoring.rs).
pub const FREE_WALK_CELLS: i64 = 6;

pub fn box_rules(world: &WorldState) -> bool {
    world.box_ext.box_rules
}

/// BOX RULES' random number in 0..2^24: a 32-bit LCG (Numerical Recipes'
/// constants) on `box_ext.rng`, the high 24 bits.
pub fn box_random(world: &mut WorldState) -> i64 {
    let s = (world.box_ext.rng as u32).wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    world.box_ext.rng = s as i64;
    (s >> 8) as i64
}

/// Everything of the BOX additions the presenter draws, worked out from the
/// state (TowerSim::box_views).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxViews {
    pub vehicles: Vec<car_elevator::VehicleView>,
    pub platforms: Vec<car_elevator::PlatformView>,
    pub people: Vec<emergency::PersonView>,
    pub service_vehicles: Vec<street::ServiceVehicleView>,
    pub amenities: Vec<amenities::AmenityView>,
    pub fire: fire::FireView,
}

pub fn views(world: &WorldState) -> BoxViews {
    BoxViews {
        vehicles: car_elevator::vehicle_views(&world.box_ext),
        platforms: car_elevator::platform_views(&world.box_ext),
        people: emergency::people_views(world),
        service_vehicles: street::service_vehicle_views(world),
        amenities: amenities::amenity_views(world),
        fire: fire::view(world),
    }
}

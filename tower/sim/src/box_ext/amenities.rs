//! BOX addition (BOX RULES only): amenities (docs/amenities.md).
//!
//! Six rooms the 1994 game has not got -- laundry, guest services, a premier
//! lounge, a swimming pool, a gym and a convention centre -- each serving
//! some kinds of unit: it takes some of the score off their evaluation (as
//! a noisy neighbour adds to it), and some earn by their visitors.
//!
//! Hooks (each `// BOX: amenities`):
//! - commands.rs place_tile / handle_remove_tile: `intercept_place`,
//!   `intercept_remove`;
//! - sims/scoring.rs recompute_object_operational_status: `relief`;
//! - tick/day_scheduler.rs checkpoint_ledger_rollover: `daily`, and every
//!   third day `upkeep`;
//! - resources.rs get_tile_star_requirement: `star_requirement`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::box_ext::box_rules;
use crate::commands::{run_global_rebuilds, CellPatch, CommandResult};
use crate::ledger::LedgerState;
use crate::resources::{FAMILY_CONDO, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN, FAMILY_OFFICE};
use crate::time::TimeState;
use crate::world::{y_to_floor, WorldState, GROUND_Y, UNDERGROUND_Y};

/// Where a room may stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// On a floor above the ground floor.
    Above,
    /// In a lobby (the ground floor's or a sky lobby), over its cells:
    /// they are lobby again when it is removed.
    Lobby,
    /// On a basement floor.
    Basement,
}

/// What a room earns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Earns {
    Nothing,
    /// So much a visit: a third of the served units' occupants a day
    /// (`weekend`: twice that on a weekend day; `weekdays`: only then).
    Visits { fee: i64, weekend: bool, weekdays: bool },
    /// A weekday's conference: so much, and so much more an occupied hotel
    /// room in the tower.
    Conference { day: i64, per_room: i64 },
}

/// One kind of amenity (docs/amenities.md's tables).
#[derive(Clone, Copy, Debug)]
pub struct Kind {
    pub tile: &'static str,
    pub title: &'static str,
    pub width: i64,
    pub floors: i64,
    pub site: Site,
    pub stars: i64,
    pub cost: i64,
    /// Every third day.
    pub upkeep: i64,
    /// Families served and the relief each is given.
    pub serves: &'static [(i64, i64)],
    /// Units each room serves.
    pub units: usize,
    pub earns: Earns,
}

const HOTELS: [i64; 3] = [FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_HOTEL_SUITE];

pub const KINDS: &[Kind] = &[
    Kind {
        tile: "laundry",
        title: "Laundry",
        width: 12,
        floors: 1,
        site: Site::Above,
        stars: 2,
        cost: 80_000,
        upkeep: 10_000,
        serves: &[(FAMILY_HOTEL_SINGLE, 20), (FAMILY_HOTEL_TWIN, 20), (FAMILY_HOTEL_SUITE, 20)],
        units: 60,
        earns: Earns::Nothing,
    },
    Kind {
        tile: "guestServices",
        title: "Guest services",
        width: 10,
        floors: 1,
        site: Site::Lobby,
        stars: 3,
        cost: 100_000,
        upkeep: 15_000,
        serves: &[(FAMILY_HOTEL_SINGLE, 20), (FAMILY_HOTEL_TWIN, 20), (FAMILY_HOTEL_SUITE, 20), (FAMILY_CONDO, 10)],
        units: 80,
        earns: Earns::Nothing,
    },
    Kind {
        tile: "premierLounge",
        title: "Premier lounge",
        width: 20,
        floors: 1,
        site: Site::Above,
        stars: 4,
        cost: 300_000,
        upkeep: 30_000,
        serves: &[(FAMILY_OFFICE, 20), (FAMILY_HOTEL_SUITE, 30)],
        units: 40,
        earns: Earns::Visits { fee: 60, weekend: false, weekdays: true },
    },
    Kind {
        tile: "pool",
        title: "Swimming pool",
        width: 30,
        floors: 1,
        site: Site::Basement,
        stars: 3,
        cost: 400_000,
        upkeep: 20_000,
        serves: &[(FAMILY_CONDO, 30), (FAMILY_HOTEL_SINGLE, 20), (FAMILY_HOTEL_TWIN, 20), (FAMILY_HOTEL_SUITE, 20)],
        units: 60,
        earns: Earns::Visits { fee: 40, weekend: true, weekdays: false },
    },
    Kind {
        tile: "gym",
        title: "Gym",
        width: 16,
        floors: 1,
        site: Site::Basement,
        stars: 3,
        cost: 150_000,
        upkeep: 10_000,
        serves: &[(FAMILY_OFFICE, 20), (FAMILY_CONDO, 20)],
        units: 60,
        earns: Earns::Visits { fee: 25, weekend: false, weekdays: false },
    },
    Kind {
        tile: "convention",
        title: "Convention centre",
        width: 30,
        floors: 2,
        site: Site::Above,
        stars: 4,
        cost: 1_000_000,
        upkeep: 50_000,
        serves: &[(FAMILY_HOTEL_SINGLE, 10), (FAMILY_HOTEL_TWIN, 10), (FAMILY_HOTEL_SUITE, 10)],
        units: 120,
        earns: Earns::Conference { day: 20_000, per_room: 500 },
    },
    Kind {
        tile: "kindergarten",
        title: "Kindergarten",
        width: 12,
        floors: 1,
        site: Site::Above,
        stars: 2,
        cost: 120_000,
        upkeep: 12_000,
        serves: &[(FAMILY_CONDO, 20)],
        units: 40,
        // a day's fees for the little ones (a third of a household a day)
        earns: Earns::Visits { fee: 25, weekend: false, weekdays: true },
    },
    Kind {
        tile: "school",
        title: "Primary school",
        width: 30,
        floors: 2,
        site: Site::Above,
        stars: 3,
        cost: 600_000,
        upkeep: 40_000,
        serves: &[(FAMILY_CONDO, 30)],
        units: 120,
        earns: Earns::Nothing,
    },
    Kind {
        // a pump room: floors near it burn slowly (box_ext/fire.rs)
        tile: "sprinklers",
        title: "Sprinkler room",
        width: 8,
        floors: 1,
        site: Site::Above,
        stars: 3,
        cost: 200_000,
        upkeep: 10_000,
        serves: &[],
        units: 0,
        earns: Earns::Nothing,
    },
];

/// A room not yet open: built since the last day's end.
pub const NEW: i64 = -1;

/// The most relief a unit is given, all its amenities together.
pub const MAX_RELIEF: i64 = 60;

pub fn kind(tile: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.tile == tile)
}

/// The front end's star check (resources::get_tile_star_requirement).
pub fn star_requirement(tile: &str) -> Option<i64> {
    kind(tile).map(|k| k.stars)
}

// ─── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Amenity {
    pub tile: String,
    /// Left cell.
    pub x: i64,
    /// Top row (a convention centre's upper floor).
    pub y: i64,
    /// The day_counter of the day's end it opened at (NEW until the first
    /// day's end after it was built).
    pub built_day: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmenitiesState {
    #[serde(default)]
    pub rooms: Vec<Amenity>,
    /// Tallies: visits, income, upkeep paid.
    #[serde(default)]
    pub visits: i64,
    #[serde(default)]
    pub income: i64,
    #[serde(default)]
    pub upkeep: i64,
    /// Each unit's relief and each room's units served (worked out from the
    /// rooms and the units, never saved): rebuilt when a room is built or
    /// removed, each day, and after loading.
    #[serde(skip)]
    pub relief: BTreeMap<String, i64>,
    #[serde(skip)]
    pub served: Vec<Vec<String>>,
    #[serde(skip)]
    pub fresh: bool,
}

impl AmenitiesState {
    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty() && self.visits == 0 && self.income == 0 && self.upkeep == 0
    }
}

// ─── Building ─────────────────────────────────────────────────────────────────

fn rejected(reason: &str) -> CommandResult {
    CommandResult { accepted: false, patch: None, reason: Some(reason.to_string()), economy_changed: None }
}

fn truthy(m: &crate::js::JsObject<String>, key: &str) -> bool {
    m.get(key).is_some_and(|v| !v.is_empty())
}

/// The amenity that covers cell (x, y), by index.
pub fn room_at(world: &WorldState, x: i64, y: i64) -> Option<usize> {
    world.box_ext.amenities.rooms.iter().position(|r| {
        let k = kind(&r.tile).expect("an amenity's tile");
        (r.x..r.x + k.width).contains(&x) && (r.y..r.y + k.floors).contains(&y)
    })
}

/// Hook in place_tile: builds an amenity (`Some`), or `None` for any other tile.
pub fn intercept_place(
    x: i64,
    y: i64,
    tile: &str,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
) -> Option<CommandResult> {
    let k = kind(tile)?;
    if !box_rules(world) {
        return Some(rejected("Invalid tile type"));
    }
    // clicked on the lower floor; the room's top row is above it
    let top = y - (k.floors - 1);
    if x < 0 || x + k.width > world.width || top < 0 || y >= world.height {
        return Some(rejected("Out of bounds"));
    }
    let site_ok = match k.site {
        Site::Above => y < GROUND_Y,
        Site::Basement => top >= UNDERGROUND_Y,
        Site::Lobby => y <= GROUND_Y && world.cells.get(&format!("{x},{y}")).is_some_and(|t| t == "lobby"),
    };
    if !site_ok {
        return Some(rejected(match k.site {
            Site::Above => "Build it on a floor above the lobby",
            Site::Basement => "Build it on a basement floor",
            Site::Lobby => "Build it in a lobby",
        }));
    }
    for row in top..=y {
        for cx in x..x + k.width {
            let key = format!("{cx},{row}");
            if truthy(&world.overlays, &key) || truthy(&world.overlay_to_anchor, &key) {
                return Some(rejected("Cell already has an overlay"));
            }
            if truthy(&world.cell_to_anchor, &key) {
                return Some(rejected("Cell already occupied"));
            }
            match world.cells.get(&key).filter(|s| !s.is_empty()) {
                Some(t) if t == "floor" && k.site != Site::Lobby => {}
                Some(t) if t == "lobby" && k.site == Site::Lobby => {}
                Some(_) => return Some(rejected("Cell already occupied")),
                None => return Some(rejected("Lay floor there first")),
            }
        }
    }
    if !free_build && k.cost > ledger.cash_balance {
        return Some(rejected("Insufficient funds"));
    }
    let anchor = format!("{x},{top}");
    let mut patch = Vec::new();
    for row in top..=y {
        for cx in x..x + k.width {
            let key = format!("{cx},{row}");
            world.cells.insert(key.clone(), k.tile.to_string());
            let is_anchor = key == anchor;
            if !is_anchor {
                world.cell_to_anchor.insert(key, anchor.clone());
            }
            patch.push(CellPatch::new(cx, row, k.tile, is_anchor));
        }
    }
    world.box_ext.amenities.rooms.push(Amenity { tile: k.tile.to_string(), x, y: top, built_day: NEW });
    world.box_ext.amenities.fresh = false;
    if !free_build {
        ledger.cash_balance -= k.cost;
    }
    run_global_rebuilds(world, ledger);
    Some(CommandResult { accepted: true, patch: Some(patch), reason: None, economy_changed: Some(k.cost > 0) })
}

/// Hook in handle_remove_tile: a click on an amenity removes the whole room;
/// its cells are bare floor again (lobby, for guest services).  `None`: not ours.
pub fn intercept_remove(x: i64, y: i64, world: &mut WorldState, ledger: &mut LedgerState) -> Option<CommandResult> {
    let i = room_at(world, x, y)?;
    let r = world.box_ext.amenities.rooms.remove(i);
    let k = kind(&r.tile).expect("an amenity's tile");
    let under = if k.site == Site::Lobby { "lobby" } else { "floor" };
    let mut patch = Vec::new();
    for row in r.y..r.y + k.floors {
        for cx in r.x..r.x + k.width {
            let key = format!("{cx},{row}");
            world.cell_to_anchor.remove(&key);
            world.cells.insert(key, under.to_string());
            patch.push(CellPatch::new(cx, row, under, true));
        }
    }
    world.box_ext.amenities.fresh = false;
    run_global_rebuilds(world, ledger);
    Some(CommandResult { accepted: true, patch: Some(patch), reason: None, economy_changed: None })
}

// ─── Service ──────────────────────────────────────────────────────────────────

/// Work out which units each open room serves, and each unit's relief.
fn rebuild(world: &mut WorldState) {
    let mut relief: BTreeMap<String, i64> = BTreeMap::new();
    let mut served = Vec::new();
    let rooms = world.box_ext.amenities.rooms.clone();
    // a kind's rooms share out the units nearest them: each room in turn,
    // in the order built, takes its nearest not yet taken by its kind
    let mut taken: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for r in &rooms {
        let k = kind(&r.tile).expect("an amenity's tile");
        if !box_rules(world) || r.built_day == NEW {
            served.push(Vec::new());
            continue;
        }
        let floor = y_to_floor(r.y);
        let mid = r.x * 2 + k.width;
        let gone = taken.entry(k.tile).or_default();
        let mut units: Vec<(i64, i64, String, i64)> = world
            .placed_objects
            .iter()
            .filter_map(|(key, o)| {
                let gain = k.serves.iter().find(|(f, _)| *f == o.object_type_code)?.1;
                if gone.iter().any(|g| g == key) {
                    return None;
                }
                let oy: i64 = key.split(',').nth(1)?.parse().ok()?;
                let df = (y_to_floor(oy) - floor).abs();
                let dx = (o.left_tile_index + o.right_tile_index + 1 - mid).abs();
                Some((df, dx, key.to_string(), gain))
            })
            .collect();
        units.sort();
        units.truncate(k.units);
        served.push(units.iter().map(|u| u.2.clone()).collect());
        for (_, _, key, gain) in units {
            *relief.entry(key.clone()).or_default() += gain;
            gone.push(key);
        }
    }
    for v in relief.values_mut() {
        *v = (*v).min(MAX_RELIEF);
    }
    let a = &mut world.box_ext.amenities;
    a.relief = relief;
    a.served = served;
    a.fresh = true;
}

/// Hook in recompute_object_operational_status: what the amenities take
/// off unit `key`'s score (0 with BOX RULES off).
pub fn relief(world: &mut WorldState, key: &str) -> i64 {
    if world.box_ext.amenities.rooms.is_empty() || !box_rules(world) {
        return 0;
    }
    if !world.box_ext.amenities.fresh {
        rebuild(world);
    }
    world.box_ext.amenities.relief.get(key).copied().unwrap_or(0)
}

/// The relief last worked out for the unit at `key`, without working it
/// out again (for the Room pane's breakdown).
pub fn relief_seen(world: &WorldState, key: &str) -> i64 {
    if !box_rules(world) {
        return 0;
    }
    world.box_ext.amenities.relief.get(key).copied().unwrap_or(0)
}

/// People in a served unit, if it is occupied.
fn occupants(world: &WorldState, key: &str) -> i64 {
    let Some(o) = world.placed_objects.get(key) else { return 0 };
    match o.object_type_code {
        c if HOTELS.contains(&c) && o.unit_status < 0x18 => {
            if c == FAMILY_HOTEL_SINGLE {
                1
            } else {
                2
            }
        }
        FAMILY_OFFICE if o.unit_status <= 0x0f => 6,
        FAMILY_CONDO if o.unit_status <= 0x17 => 3,
        _ => 0,
    }
}

/// Hook at the day's ledger rollover: the rooms open, the units they serve
/// worked out again, and the day's visits and conferences paid for.
pub fn daily(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    if world.box_ext.amenities.rooms.is_empty() || !box_rules(world) {
        return;
    }
    for r in world.box_ext.amenities.rooms.iter_mut().filter(|r| r.built_day == NEW) {
        r.built_day = time.day_counter;
    }
    rebuild(world);
    let weekend = time.weekend_flag != 0;
    let mut paid = 0;
    let mut visits = 0;
    let rooms = world.box_ext.amenities.rooms.clone();
    for (i, r) in rooms.iter().enumerate() {
        let k = kind(&r.tile).expect("an amenity's tile");
        let units = world.box_ext.amenities.served.get(i).cloned().unwrap_or_default();
        if units.is_empty() {
            continue;
        }
        match k.earns {
            Earns::Nothing => {}
            Earns::Visits { fee, weekend: more_at_weekend, weekdays } => {
                if weekdays && weekend {
                    continue;
                }
                let people: i64 = units.iter().map(|u| occupants(world, u)).sum();
                let n = people / 3 * if more_at_weekend && weekend { 2 } else { 1 };
                visits += n;
                paid += n * fee;
            }
            Earns::Conference { day, per_room } => {
                if weekend {
                    continue;
                }
                let rooms_let = world
                    .placed_objects
                    .values()
                    .filter(|o| HOTELS.contains(&o.object_type_code) && o.unit_status < 0x18)
                    .count() as i64;
                visits += 1;
                paid += day + per_room * rooms_let;
            }
        }
    }
    if paid > 0 {
        ledger.cash_balance = (ledger.cash_balance + paid).min(ledger.cap());
    }
    world.box_ext.period_amenity_income += paid;
    let a = &mut world.box_ext.amenities;
    a.visits += visits;
    a.income += paid;
}

/// Hook beside the three-day expense sweep: the rooms' upkeep.
pub fn upkeep(world: &mut WorldState, ledger: &mut LedgerState) {
    if world.box_ext.amenities.rooms.is_empty() || !box_rules(world) {
        return;
    }
    let due: i64 = world.box_ext.amenities.rooms.iter().map(|r| kind(&r.tile).map(|k| k.upkeep).unwrap_or(0)).sum();
    ledger.cash_balance = 0.max(ledger.cash_balance - due);
    world.box_ext.amenities.upkeep += due;
    world.box_ext.period_amenity_upkeep += due;
}

// ─── Views ────────────────────────────────────────────────────────────────────

/// An amenity as the presenter draws it.  `open`: it has stood a day and
/// BOX RULES are on; `served`: the units it serves.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmenityView {
    pub tile: &'static str,
    pub x: i64,
    pub row: i64,
    pub width: i64,
    pub floors: i64,
    pub open: bool,
    pub served: usize,
}

pub fn amenity_views(world: &WorldState) -> Vec<AmenityView> {
    let a = &world.box_ext.amenities;
    a.rooms
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let k = kind(&r.tile)?;
            let served = a.served.get(i).map(|v| v.len()).unwrap_or(0);
            Some(AmenityView {
                tile: k.tile,
                x: r.x,
                row: r.y,
                width: k.width,
                floors: k.floors,
                open: box_rules(world) && served > 0,
                served,
            })
        })
        .collect()
}

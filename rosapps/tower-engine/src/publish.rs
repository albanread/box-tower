// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Alban Read
//
// publish.rs -- the publisher (graphics design, section 5.10; unit 4): a
// tower's state turned into the live block the presenter draws
// (rosgd/include/rosgd/tower.h, through tower-surface's mirror of it).
//
// It grew from the seed in tower-surface/tests/seed, the mapping the twin's
// pictures were drawn with, and takes over from it:
//
// - Rooms.  One record an anchor (placed_objects' cells), one for each run
//   of lobby cells on a row; two-floor families (cinema, party hall,
//   recycling) and the taller ones (metro 3, cathedral 5) are one record of
//   that height.  A room keeps its index while it stands (a free list), so
//   that a change of state rewrites one record in one 1024-room chunk.
// - Cells.  `room` from the rooms; `back` from the cell's type and place
//   (floor, basement, street, soil, rock, roof); `front` for fire.
// - Fixtures (the fixtures' pass, P2): one a shaft (from the carriers),
//   from the machine room over its highest floor (`over`, the top piece's
//   height) to the pit under its lowest; one a car elevator (BOX RULES),
//   from the street down; one a stair or escalator.
// - Fire escapes (BOX): a steel stair outside each end of each floor, two
//   cells wide, a landing at every floor.  Where the ends of the floors
//   are level it rises straight from the street to the top floor; at a
//   step-back a new stair stands on the roof below, with a landing at the
//   roof; a stair beside floors that overhang goes on down to the street.
//   The movers use them (`Site`): people go down them when there is a fire.
// - What moves -- the stops table and the movers -- is made a frame at a
//   time by movers.rs and put in the block by `publish_moving`, so that
//   cars and people move at the frame rate while the rooms and cells are
//   made again only a few times a second.
// - The section generations: a section's generation moves when, and only
//   when, its bytes do (each publish is built afresh beside the block and
//   compared with it, section by section, room chunk by room chunk).
//
// The simulation is read only through TowerSim's public calls, as
// upstream's client reads it (cells_to_array, live_carriers, fire_fronts).

use std::collections::{BTreeMap, HashMap, HashSet};

use tower_sim::box_ext::amenities;
pub use tower_sim::box_ext::escapes::Escape;
use tower_sim::box_ext::escapes::{fire_escapes, floor_runs};
use tower_sim::index::TowerSim;
use tower_sim::resources::TILE_WIDTHS;
use tower_sim::time::{advance_one_tick, TimeState};
use tower_surface::block::*;
use tower_surface::signs;

// ---- the art: the block and its entries' names ------------------------------

/// The packed art (towerpack's block) and every entry's number by name, from
/// the manifest towerpack writes beside it.
pub struct Art {
    pub block: Vec<u8>,
    pub art: HashMap<String, u32>,
    #[allow(dead_code)] // the movers' pass (P3) names its sprites
    pub sprites: HashMap<String, u32>,
}

impl Art {
    /// The block and the manifest's text ("art N name ...", "sprite N name ...")
    pub fn new(block: Vec<u8>, manifest: &str) -> Result<Art, String> {
        if block.len() < std::mem::size_of::<TowerArt>() {
            return Err("the art block is too short".into());
        }
        let h = TowerArt::from_bytes(&block);
        if h.magic != TOWER_ART_MAGIC || h.version != TOWER_VERSION {
            return Err("the art block is not a tower's".into());
        }
        if h.total_size() > block.len() {
            return Err("the art block is cut short".into());
        }
        let (mut art, mut sprites) = (HashMap::new(), HashMap::new());
        for line in manifest.lines() {
            let mut w = line.split_whitespace();
            let (Some(kind), Some(n), Some(name)) = (w.next(), w.next(), w.next()) else { continue };
            let Ok(n) = n.parse::<u32>() else { continue };
            match kind {
                "art" => art.insert(name.to_string(), n),
                "sprite" => sprites.insert(name.to_string(), n),
                _ => None,
            };
        }
        if art.is_empty() {
            return Err("the art's manifest names no entries".into());
        }
        Ok(Art { block, art, sprites })
    }

    pub fn head(&self) -> Box<TowerArt> {
        TowerArt::from_bytes(&self.block)
    }

    pub fn gen(&self) -> u32 {
        self.head().gen
    }

    /// an entry's number, 0 (nothing drawn) if the art has none of that name
    pub fn a(&self, name: &str) -> u16 {
        self.art.get(name).copied().unwrap_or(0) as u16
    }
}

// ---- the clock -----------------------------------------------------------------

/// upstream's dayTickToHour (GameScene.ts), 7 .. 31, folded to 0 .. 24
pub fn day_tick_to_hour(day_tick: i64) -> f64 {
    let dp = day_tick / 400;
    let off = (day_tick - dp * 400) as f64;
    let h = match dp {
        0 => 7.0 + off / 400.0 * 5.0,
        1 => 12.0 + off / 400.0 * 0.5,
        2 => 12.5 + off / 400.0 * 0.5,
        3 => 13.0 + off / 400.0 * 4.0,
        4 => 17.0 + off / 400.0 * 4.0,
        5 => 21.0 + off / 400.0 * 4.0,
        6 => 25.0 + off / 200.0 * 6.0,
        _ => 7.0,
    };
    h % 24.0
}

/// The tower's clock, kept beside the simulation without asking it for its
/// whole state: taken once (from the snapshot a tower was made from) and
/// moved on a tick at a time as the simulation's tick count moves, by the
/// simulation's own rule (time::advance_one_tick).
#[derive(Clone, Debug)]
pub struct Clock {
    pub t: TimeState,
}

impl Clock {
    pub fn new(t: TimeState) -> Clock {
        Clock { t }
    }
    pub fn of(sim: &TowerSim) -> Clock {
        Clock { t: sim.save_state().time }
    }
    /// moved on to the simulation's tick
    pub fn sync(&mut self, sim: &TowerSim) {
        let now = sim.sim_time();
        if now < self.t.total_ticks || now - self.t.total_ticks > 100_000 {
            self.t = sim.save_state().time; // a new tower, or far behind: asked again
            return;
        }
        while self.t.total_ticks < now {
            self.t = advance_one_tick(&self.t).0;
        }
    }
    pub fn hour(&self) -> f64 {
        day_tick_to_hour(self.t.day_tick)
    }
    /// the header's hour, in 1/64 hour
    pub fn hour64(&self) -> u16 {
        ((self.hour() * 64.0).round() as u32 % 1536) as u16
    }
    pub fn weekend(&self) -> bool {
        self.t.weekend_flag != 0
    }
}

// ---- the mapping (graphics design 6.3) -------------------------------------------

/// JavaScript's ToInt32 of a double
fn to_int32(x: f64) -> i32 {
    let m = x.rem_euclid(4_294_967_296.0);
    (if m >= 2_147_483_648.0 { m - 4_294_967_296.0 } else { m }) as i64 as i32
}

/// upstream's getRoomVariantIndex, its double-precision products included
pub fn variant(x: i64, y: i64, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let mut h = to_int32(x as f64 * 374_761_393.0 + y as f64 * 668_265_263.0);
    h = to_int32((h ^ ((h as u32) >> 13) as i32) as f64 * 1_274_126_177.0);
    let h = (h ^ ((h as u32) >> 16) as i32) as u32;
    (h % n as u32) as usize
}

const HOTELS: [&str; 3] = ["hotelSingle", "hotelTwin", "hotelSuite"];

/// a family's pictures, as upstream's ROOM_TEXTURES lists them, by our
/// names; its height in floors; its lamp profile
pub fn family(tile: &str) -> Option<(Vec<String>, u32, u32)> {
    let v = |base: &str, n: usize| (0..n).map(|i| format!("rooms/{base}.{i}")).collect::<Vec<_>>();
    let one = |name: &str| vec![format!("rooms/{name}")];
    Some(match tile {
        "office" => (v("office", 5), 1, P_OFFICE),
        "condo" => (v("condo", 1), 1, P_HOME),
        "restaurant" => (v("restaurant", 5), 1, P_RESTAURANT),
        "fastFood" => (v("fast-food", 4), 1, P_FASTFOOD),
        "retail" => (v("retail", 5), 1, P_SHOP),
        "hotelSingle" => (v("hotel-single", 3), 1, P_HOTEL),
        "hotelTwin" => (v("hotel-twin", 3), 1, P_HOTEL),
        "hotelSuite" => (v("hotel-suite", 3), 1, P_HOTEL),
        "cinema" => (one("cinema"), 2, P_ENTERTAINMENT),
        "partyHall" => (one("party-hall"), 2, P_ENTERTAINMENT),
        "recyclingCenterUpper" => (v("recycling", 5), 2, P_SERVICE),
        "parking" => (
            ["empty", "0", "1", "2", "3"].iter().map(|s| format!("rooms/parking.{s}")).collect(),
            1,
            P_PARKING,
        ),
        "parkingRamp" => (one("parking-ramp"), 1, P_PARKING),
        "security" => (v("security", 3), 1, P_SERVICE),
        // BOX RULES' amenities (sim box_ext::amenities); `.busy` in their hours
        "laundry" => (one("laundry"), 1, P_SERVICE),
        "guestServices" => (one("guest-services"), 1, P_LOBBY),
        "premierLounge" => (one("premier-lounge"), 1, P_RESTAURANT),
        "pool" => (one("pool"), 1, P_ALWAYS),
        "gym" => (one("gym"), 1, P_SHOP),
        "convention" => (one("convention"), 2, P_ENTERTAINMENT),
        "kindergarten" => (one("kindergarten"), 1, P_SHOP),
        "school" => (one("school"), 2, P_OFFICE),
        "sprinklers" => (one("sprinklers"), 1, P_SERVICE),
        "housekeeping" => (one("housekeeping"), 1, P_SERVICE),
        "medical" => (one("medical"), 1, P_SERVICE),
        // the station without a train: movers.rs runs the trains
        "metro" => (one("metro.0"), 3, P_ALWAYS),
        "cathedral" => (one("cathedral"), 5, P_ENTERTAINMENT),
        _ => return None,
    })
}

pub fn width_of(tile: &str) -> u32 {
    if let Some(k) = amenities::kind(tile) {
        return k.width as u32;
    }
    TILE_WIDTHS.iter().find(|(n, _)| *n == tile).map(|(_, w)| *w as u32).unwrap_or(1)
}

/// Whether an open amenity has people in it now: its hours, as its users
/// keep them (docs/amenities.md)
pub fn amenity_busy(tile: &str, hour: f64, weekend: bool) -> bool {
    let h = |a: f64, b: f64| (a..b).contains(&hour);
    match tile {
        "laundry" => h(8.0, 18.0),
        "guestServices" => h(7.0, 23.0),
        "premierLounge" => !weekend && (h(11.5, 14.0) || h(17.0, 20.0)),
        "pool" => h(7.0, 9.0) || h(17.0, 22.0) || (weekend && h(9.0, 20.0)),
        "gym" => h(6.0, 9.0) || h(17.0, 21.0) || (weekend && h(9.0, 12.0)),
        "convention" => !weekend && h(9.0, 17.0),
        "kindergarten" => !weekend && h(8.0, 17.5),
        "school" => !weekend && h(8.5, 15.5),
        _ => false,
    }
}

/// The vacant, sale and stuck states and the sign of a room, from its
/// record's display fields (design 6.3's table)
pub fn room_state(tile: &str, unit_status: i64, active: Option<i64>, wc: u32) -> (u32, u32) {
    let hotel = HOTELS.contains(&tile);
    let mut st = 0;
    if (hotel && unit_status >= 0x38) || ((tile == "office" || tile == "condo") && active == Some(0)) {
        st |= S_STUCK;
    }
    if hotel && unit_status >= 0x38 {
        st |= S_INFESTED;
    }
    let vacant = match tile {
        "office" => unit_status > 0x0f,
        "condo" => unit_status > 0x17,
        _ if hotel => false,
        _ => active == Some(0),
    };
    let mut sign = 0;
    if vacant {
        st |= S_VACANT;
        let kind = if tile == "condo" {
            st |= S_SALE;
            "for-sale"
        } else {
            "for-rent"
        };
        sign = signs::banner(kind, wc).unwrap_or(0);
    }
    (st, sign)
}

// ---- the publisher ---------------------------------------------------------------

/// What the user's view adds to the block: the quality, the view, the frame
/// round the tool's cells
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Look {
    pub quality: u8,
    pub view: u8,
    pub outline: [i16; 4],
    pub outline_rgba: u32,
}

/// What changed in a publish: the sections (bit k for gen[k]) and the room
/// chunks (bit k for room_gen[k]) whose bytes moved
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changed {
    pub sections: u32,
    pub chunks: u32,
}

#[allow(dead_code)]
impl Changed {
    pub fn any(&self) -> bool {
        self.sections != 0 || self.chunks != 0
    }
}

/// What the movers need of the building: the lobby's ends on the ground
/// floor (the way in from the street), each row's runs of built cells
/// (first and last cell; where people may walk), and the fire escapes
#[derive(Clone, Debug, Default)]
pub struct Site {
    pub lobby: Option<(i64, i64)>,
    pub floors: Vec<Vec<(i64, i64)>>,
    pub escapes: Vec<Escape>,
    /// the stairs and escalators: (escalator, left cell, the lower floor's
    /// row); each rises left to right, eight cells, to the floor above
    pub links: Vec<(bool, i64, i64)>,
    /// the metro station: its left cell and its top row (three floors, 30
    /// cells: the ticket hall over the platform hall)
    pub metro: Option<(i64, i64)>,
    /// every room: its tile, left cell, width and the row people walk on
    /// (its bottom floor's)
    pub rooms: Vec<(String, i64, i64, i64)>,
}

impl Site {
    /// the run of row `row` that holds cell position `x` (or the nearest)
    pub fn run_at(&self, row: i64, x: f64) -> Option<(i64, i64)> {
        let runs = self.floors.get(usize::try_from(row).ok()?)?;
        runs.iter()
            .copied()
            .min_by(|a, b| {
                let d = |r: &(i64, i64)| if x < r.0 as f64 { r.0 as f64 - x } else if x > (r.1 + 1) as f64 { x - (r.1 + 1) as f64 } else { 0.0 };
                d(a).total_cmp(&d(b))
            })
    }

    /// `x` kept on the floor of row `row`: within the run under it, half a
    /// cell in from its ends
    pub fn on_floor(&self, row: i64, x: f64) -> f64 {
        match self.run_at(row, x) {
            Some((a, b)) => x.clamp(a as f64 + 0.5, (b + 1) as f64 - 0.5),
            None => x,
        }
    }
}

/// a room's identity while it stands: its place, its size and what it is
type RoomKey = (u32, u32, u32, u32, u16);

pub struct Publisher {
    /// the block as last published
    pub block: Box<Tower>,
    /// the lobby, the floors' runs and the fire escapes, for the movers
    pub site: Site,
    /// the car elevators' rows of the stops table (they have no calls)
    lift_stops: Box<[[u8; ROWS]; SHAFTS]>,
    /// the cells with a room in them at the last publish, and the wreckage
    /// a fire or a bomb left where rooms were (burnt, or rubble) until
    /// something is built there again
    rooms_before: HashSet<(i64, i64)>,
    wreck: HashMap<(i64, i64), &'static str>,
    next: Box<Tower>,
    index: HashMap<RoomKey, u16>,
    free: Vec<u16>,
    top: u16,
}

const ROOM_CHUNK: usize = 1024;

fn bytes<T>(v: &[T]) -> &[u8] {
    // SAFETY: plain-integer repr(C) records (tower-surface's block.rs)
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

impl Default for Publisher {
    fn default() -> Self {
        Publisher::new(4)
    }
}

impl Publisher {
    pub fn new(scale: u16) -> Publisher {
        Publisher {
            block: Tower::new(scale),
            next: Tower::new(scale),
            site: Site::default(),
            lift_stops: Box::new([[0; ROWS]; SHAFTS]),
            rooms_before: HashSet::new(),
            wreck: HashMap::new(),
            index: HashMap::new(),
            free: Vec::new(),
            top: 1,
        }
    }

    /// A new tower: every room forgotten (the block's sections move on as
    /// the new tower's bytes differ)
    pub fn reset(&mut self) {
        self.rooms_before.clear();
        self.wreck.clear();
        self.index.clear();
        self.free.clear();
        self.top = 1;
    }

    fn room_index(&mut self, key: RoomKey, used: &mut Vec<bool>) -> Option<u16> {
        if let Some(&i) = self.index.get(&key) {
            used[i as usize] = true;
            return Some(i);
        }
        let i = match self.free.pop() {
            Some(i) => i,
            None if (self.top as usize) < ROOMS => {
                self.top += 1;
                self.top - 1
            }
            None => return None,
        };
        self.index.insert(key, i);
        used[i as usize] = true;
        Some(i)
    }

    /// The tower now into the block: returns what changed
    pub fn publish(&mut self, sim: &TowerSim, clock: &Clock, art: &Art, look: &Look) -> Changed {
        let a = |n: &str| art.a(n);
        let mut t = std::mem::replace(&mut self.next, Tower::zeroed());
        // the cells, rooms, fixtures and stops made afresh
        for row in t.cell.iter_mut() {
            row.fill(Cell::default());
        }
        t.room.fill(Room::default());
        t.fix.fill(Fixture::default());
        // what moves stays as publish_moving last made it
        t.stops = self.block.stops;
        t.mover = self.block.mover;
        t.nmovers = self.block.nmovers;

        // what is in each cell, and the overlays
        let patches = sim.cells_to_array();
        let mut grid: HashMap<(i64, i64), &str> = HashMap::new();
        let mut overlays: Vec<(i64, i64, &str)> = Vec::new();
        for p in &patches {
            if p.is_overlay.unwrap_or(false) {
                overlays.push((p.x, p.y, p.tile_type.as_str()));
            } else {
                grid.insert((p.x, p.y), p.tile_type.as_str());
            }
        }

        // the rooms wanted, in a fixed order (top to bottom, left to right)
        let mut wanted: Vec<(RoomKey, Room)> = Vec::new();
        let mut anchors: Vec<_> = patches.iter().filter(|p| p.is_anchor && !p.is_overlay.unwrap_or(false)).collect();
        let views = sim.box_views();
        let open: Vec<(i64, i64)> = views.amenities.iter().filter(|v| v.open).map(|v| (v.x, v.row)).collect();
        // BOX RULES: the floors a fire reached, closed for the day (rows)
        let closed: Vec<i64> = views.fire.closed.iter().map(|f| tower_sim::world::GRID_HEIGHT - 1 - f).collect();
        let hour = clock.hour64() as f64 / 64.0;
        anchors.sort_by_key(|p| (p.y, p.x));
        let mut site_rooms: Vec<(String, i64, i64, i64)> = Vec::new();
        for p in &anchors {
            let tile = p.tile_type.as_str();
            if tile == "recyclingCenterLower" && grid.get(&(p.x, p.y - 1)).copied() == Some("recyclingCenterUpper") {
                continue; // the upper slice's picture covers both floors
            }
            let (x0, y0) = (p.x as u32, p.y as u32);
            let Some((pics, hf, profile)) = family(tile) else { continue };
            site_rooms.push((tile.to_string(), p.x, width_of(tile) as i64, p.y + hf as i64 - 1));
            let wc = width_of(tile);
            let us = p.unit_status.unwrap_or(0);
            let active = p.eval_active_flag;
            let hotel = HOTELS.contains(&tile);
            let mut name = pics[variant(p.x, p.y, pics.len())].clone();
            if hotel && us >= 0x28 {
                name.push_str(".dirty");
            }
            if amenities::kind(tile).is_some() && open.contains(&(p.x, p.y)) && amenity_busy(tile, hour, clock.weekend()) {
                name.push_str(".busy");
            }
            // BOX RULES: a space is bare until a car parks in it (movers.rs
            // draws the car, nose out)
            if tile == "parking" && sim.box_rules() {
                name = "rooms/parking.empty".into();
            }
            if tile == "parking" && p.coverage_flag.unwrap_or(0) == 0 {
                name = "rooms/parking.no-access".into();
            }
            // a room as tall as its family, its top row the anchor's less
            // the floors above it (upper slices anchor at their top)
            let ytop = y0;
            let (mut st, sign) = room_state(tile, us, active, wc);
            // closed after a fire: grey, its lamps off
            if (ytop as i64..ytop as i64 + hf as i64).any(|r| closed.contains(&r)) {
                st |= S_STUCK | S_DARK;
            }
            let mut room = Room::place(x0, ytop, wc, hf);
            room.set_sign(sign);
            room.set_art(a(&name) as u32);
            room.set_profile(profile);
            room.set_state(st);
            if let Some(e) = p.eval_level.filter(|e| (0..=2).contains(e)) {
                room.set_eval(e as u32 + 1);
            }
            let kind = 2 + family_number(tile);
            wanted.push(((x0, ytop, wc, hf, kind), room));
        }
        // lobby runs, one room each
        for y in 0..GRID_H as i64 {
            let mut x = 0i64;
            while x < GRID_W as i64 {
                if grid.get(&(x, y)).copied() == Some("lobby") {
                    let x0 = x;
                    while grid.get(&(x, y)).copied() == Some("lobby") {
                        x += 1;
                    }
                    let wc = (x - x0) as u32;
                    let mut room = Room::place(x0 as u32, y as u32, wc, 1);
                    room.set_art(a("rooms/lobby") as u32);
                    room.set_profile(P_LOBBY);
                    wanted.push(((x0 as u32, y as u32, wc, 1, 0), room));
                } else {
                    x += 1;
                }
            }
        }
        // their indices: kept while a room stands
        let mut used = vec![false; ROOMS];
        let mut placed: Vec<(u16, Room)> = Vec::with_capacity(wanted.len());
        for (key, room) in wanted {
            if let Some(i) = self.room_index(key, &mut used) {
                placed.push((i, room));
            }
        }
        let gone: Vec<RoomKey> = self.index.iter().filter(|(_, &i)| !used[i as usize]).map(|(k, _)| *k).collect();
        for k in gone {
            if let Some(i) = self.index.remove(&k) {
                self.free.push(i);
            }
        }
        // the free list's lowest first, so that a tower's indices stay small
        self.free.sort_unstable_by(|a, b| b.cmp(a));
        for (i, room) in &placed {
            t.room[*i as usize] = *room;
            for y in room.ytop()..(room.ytop() + room.height().max(1)).min(GRID_H as u32) {
                for x in room.x0()..(room.x0() + room.width()).min(GRID_W as u32) {
                    t.cell[y as usize][x as usize].room = *i;
                }
            }
        }
        t.nrooms = placed.iter().map(|(i, _)| *i + 1).max().unwrap_or(1).max(1);

        // the pieces behind
        let ground = GROUND as i64;
        let built = |x: i64, y: i64| grid.contains_key(&(x, y));
        for y in 0..GRID_H as i64 {
            for x in 0..GRID_W as i64 {
                let h = hash((x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b));
                let tile = grid.get(&(x, y)).copied();
                let back = match tile {
                    Some("floor") if y >= ground => a(if x % 3 == 0 { "building/basement-light" } else { "building/basement" }),
                    Some("floor") => a(if x % 3 == 0 { "building/floor-column" } else { "building/floor" }),
                    Some(_) => 0,
                    None if y >= ground => {
                        let floor = GRID_H as i64 - 1 - y; // 9 just under the street
                        let v = ["a", "b", "c", "d"][(h % 4) as usize];
                        match floor {
                            9 => a(&format!("world/soil-top-{}", ["a", "b"][(h % 2) as usize])),
                            5..=8 => a(&format!("world/soil-{v}")),
                            _ => a(&format!("world/rock-{v}")),
                        }
                    }
                    None if y == ground - 1 => a(if h % 8 == 0 { "world/street-lamp" } else { "world/street" }),
                    None if y < ground - 1 && built(x, y + 1) => {
                        let (l, r) = (built(x - 1, y + 1) && !built(x - 1, y), built(x + 1, y + 1) && !built(x + 1, y));
                        a(match (l, r) {
                            (false, _) => "building/roof-left",
                            (_, false) => "building/roof-right",
                            _ if x % 4 == 0 => "building/roof-light",
                            _ => "building/roof",
                        })
                    }
                    None => 0,
                };
                let c = &mut t.cell[y as usize][x as usize];
                c.back = back;
                c.flags = (tile.is_some() && y < ground) as u16;
            }
        }

        // wreckage: what a fire burnt out (on a burning floor) or a bomb
        // blew up (in its blast) leaves burnt cells or rubble, drawn where
        // it was until something is built there again
        {
            let es = &sim.world().event_state;
            let (fl, fr) = sim.fire_fronts();
            let burning = |y: i64| {
                let floor = (GRID_H as i64 - 1 - y) as usize;
                fl.get(floor).zip(fr.get(floor)).is_some_and(|(&l, &r)| l != 0xffff && r != 0xffff)
            };
            let blasted = |x: i64, y: i64| {
                let floor = GRID_H as i64 - 1 - y;
                es.game_state_flags & 0x40 != 0
                    && (es.bomb_floor - 2..=es.bomb_floor + 3).contains(&floor)
                    && (es.bomb_tile - 20..es.bomb_tile + 20).contains(&x)
            };
            let room = |x: i64, y: i64| grid.contains_key(&(x, y));
            let now: HashSet<(i64, i64)> = grid.keys().copied().collect();
            for &(x, y) in self.rooms_before.difference(&now) {
                if blasted(x, y) {
                    self.wreck.insert((x, y), if x % 2 == 0 { "building/rubble" } else { "building/rubble-b" });
                } else if burning(y) {
                    self.wreck.insert((x, y), "events/burnt");
                }
            }
            self.rooms_before = now;
            self.wreck.retain(|&(x, y), _| !room(x, y));
            for (&(x, y), &name) in &self.wreck {
                if (0..GRID_W as i64).contains(&x) && (0..GRID_H as i64).contains(&y) {
                    t.cell[y as usize][x as usize].back = a(name);
                }
            }
        }

        // fire in front: the fronts per floor, upstream's left and right
        let (left, right) = sim.fire_fronts();
        for (floor, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
            if l == 0xffff || r == 0xffff || l > r || floor >= GRID_H {
                continue;
            }
            let y = GRID_H - 1 - floor;
            for x in l.max(0)..=r.min(GRID_W as i64 - 1) {
                // (upstream keeps fronts on floors with nothing built; only
                // what is built burns on the screen)
                if !grid.contains_key(&(x, y as i64)) {
                    continue;
                }
                t.cell[y][x as usize].front = a(if x == l {
                    "events/fire-left"
                } else if x == r {
                    "events/fire-right"
                } else {
                    "events/fire"
                });
            }
        }

        // the floors' runs, and the fire escapes beside their ends
        let floors = floor_runs(&built);
        let escapes = fire_escapes(&floors, &built);
        for e in &escapes {
            for y in e.top..=e.bottom {
                let piece = if y == e.top { "top" } else { "flight" };
                for (x, part) in [(e.x, if e.side < 0 { "out" } else { "in" }), (e.x + 1, if e.side < 0 { "in" } else { "out" })] {
                    let side = if e.side < 0 { "left" } else { "right" };
                    let n = a(&format!("building/escape-{piece}-{side}-{part}"));
                    let c = &mut t.cell[y as usize][x as usize];
                    // over the sky behind; in front of a roof or the street
                    if c.back == 0 {
                        c.back = n;
                    } else if c.front == 0 {
                        c.front = n;
                    }
                }
            }
        }
        self.site.floors = floors;
        self.site.escapes = escapes;
        self.site.rooms = site_rooms;
        self.site.metro = patches.iter().filter(|p| p.tile_type == "metro" && p.is_anchor).map(|p| (p.x, p.y)).min_by_key(|m| m.1);
        self.site.links = overlays
            .iter()
            .filter(|(_, _, t)| *t == "stairs" || *t == "escalator")
            .map(|&(x, y, t)| (t == "escalator", x, y))
            .collect();

        // fixtures: a shaft for each carrier, a car elevator's, then the
        // stairs and escalators.  w2: the shaft's row of the stops table,
        // and the units its top piece stands above its highest floor
        let fix_word = |x0: u32, ytop: u32, w: u32, rows: u32| (x0 & 511) | (ytop & 127) << 9 | (w & 255) << 16 | (rows & 127) << 24;
        let top_over = |base: u16| {
            let h = art.head();
            let e = h.art[(base as usize + 1).min(ART - 1)];
            ((e[1] >> 16) / h.scale.max(1) as u32).min(255)
        };
        let mut nf = 0usize;
        let carriers = sim.live_carriers();
        for (k, c) in carriers.iter().enumerate().take(SHAFTS) {
            if nf == FIXTURES {
                break;
            }
            let (top, bottom) = (c.top_served_floor, c.bottom_served_floor);
            let ytop = (GRID_H as i64 - 1 - top).clamp(0, GRID_H as i64 - 1) as u32;
            let rows = (top - bottom + 2).clamp(2, 127) as u32; // the floors served and the pit
            let (kind, base, w) = match c.carrier_mode {
                0 => (FIX_EXPRESS, a("transport/elevator-express"), 6),
                2 => (FIX_SERVICE, a("transport/elevator-service"), 4),
                _ => (FIX_STANDARD, a("transport/elevator"), 4),
            };
            let x0 = c.column.clamp(0, GRID_W as i64 - 1) as u32;
            t.fix[nf].w = [fix_word(x0, ytop, w, rows), base as u32 | kind << 16, k as u32 & 63 | top_over(base) << 8, seed(x0, ytop)];
            nf += 1;
        }
        // car elevators: a shaft each from the street down, its row of the
        // stops table after the carriers', every floor of it served
        let mut lifts: BTreeMap<i64, (i64, i64)> = BTreeMap::new(); // column -> (top row, bottom row)
        for p in &patches {
            if p.tile_type == "carElevator" && p.is_anchor {
                let e = lifts.entry(p.x).or_insert((p.y, p.y));
                e.0 = e.0.min(p.y);
                e.1 = e.1.max(p.y);
            }
        }
        let lift_base = a("transport/car-lift");
        let mut lift_stops = Box::new([[0u8; ROWS]; SHAFTS]);
        for (j, (&x, &(y0, y1))) in lifts.iter().enumerate() {
            let k = carriers.len().min(SHAFTS) + j;
            if nf == FIXTURES || k >= SHAFTS {
                break;
            }
            let (x0, ytop) = (x.clamp(0, GRID_W as i64 - 1) as u32, y0.clamp(0, GRID_H as i64 - 1) as u32);
            let rows = (y1 - y0 + 2).clamp(2, 127) as u32;
            t.fix[nf].w = [fix_word(x0, ytop, 8, rows), lift_base as u32 | FIX_CARLIFT << 16, k as u32 & 63 | top_over(lift_base) << 8, seed(x0, ytop)];
            nf += 1;
            for row in y0..=y1 {
                if (0..ROWS as i64).contains(&row) {
                    lift_stops[k][row as usize] |= STOP_SERVED;
                }
            }
        }
        for &(x, y, tile) in &overlays {
            let (kind, name) = match tile {
                "stairs" => (FIX_STAIRS, "transport/stairs"),
                "escalator" => (FIX_ESCALATOR, "transport/escalator"),
                _ => continue,
            };
            if nf == FIXTURES {
                break;
            }
            let (x0, ytop) = (x.clamp(0, GRID_W as i64 - 1) as u32, y.clamp(0, GRID_H as i64 - 1) as u32);
            t.fix[nf].w = [fix_word(x0, ytop, width_of(tile), 1), a(name) as u32 | kind << 16, 0, seed(x0, ytop)];
            nf += 1;
        }
        t.nfixtures = nf as u16;
        self.lift_stops = lift_stops;
        // the lobby's ends on the ground floor, the way in from the street
        let g = GROUND as i64 - 1;
        let lob: Vec<i64> = (0..GRID_W as i64).filter(|&x| grid.get(&(x, g)).copied() == Some("lobby")).collect();
        self.site.lobby = lob.first().zip(lob.last()).map(|(a, b)| (*a, *b));

        // the palette: upstream's evaluation colours, premultiplied
        let pm = |r: u32, g: u32, b: u32, al: u32| {
            let c = |v: u32| (v * al + 127) / 255;
            c(r) | c(g) << 8 | c(b) << 16 | al << 24
        };
        t.pal[1] = pm(221, 51, 51, 128);
        t.pal[2] = pm(221, 204, 0, 128);
        t.pal[3] = pm(68, 136, 255, 128);

        // the header
        let old = &self.block;
        t.magic = TOWER_MAGIC;
        t.version = TOWER_VERSION;
        t.frame = old.frame;
        t.art = art.gen();
        t.grid_w = GRID_W as u16;
        t.grid_h = GRID_H as u16;
        t.ground_row = GROUND as u16;
        t.scale = art.head().scale;
        t.quality = look.quality;
        t.rate = 20;
        t.view = look.view;
        t.flags = if clock.weekend() { F_WEEKEND } else { 0 };
        t.hour = clock.hour64();
        t.day = clock.t.day_counter as u16;
        t.outline = look.outline;
        t.outline_rgba = look.outline_rgba;
        t.back_rgba = 0xff20_2020;

        // what moved, section by section
        let mut ch = Changed::default();
        let gen = old.gen;
        let room_gen = old.room_gen;
        t.gen = gen;
        t.room_gen = room_gen;
        let same = |a: &[u8], b: &[u8]| a == b;
        if !same(bytes(&t.cell[..]), bytes(&old.cell[..])) {
            ch.sections |= 1 << TW_CELLS;
        }
        for k in 0..ROOMS / ROOM_CHUNK {
            let r = k * ROOM_CHUNK..(k + 1) * ROOM_CHUNK;
            if !same(bytes(&t.room[r.clone()]), bytes(&old.room[r])) {
                ch.chunks |= 1 << k;
                ch.sections |= 1 << TW_ROOMS;
            }
        }
        if !same(bytes(&t.fix[..]), bytes(&old.fix[..])) {
            ch.sections |= 1 << TW_FIX;
        }
        if !same(bytes(&t.stops[..]), bytes(&old.stops[..])) {
            ch.sections |= 1 << TW_STOPS;
        }
        if !same(bytes(&t.pal[..]), bytes(&old.pal[..])) {
            ch.sections |= 1 << TW_PAL;
        }
        if !same(bytes(&t.mover[..]), bytes(&old.mover[..])) {
            ch.sections |= 1 << TW_MOVERS;
        }
        for s in 0..TW_NSECT {
            if ch.sections & (1 << s) != 0 {
                t.gen[s] = t.gen[s].wrapping_add(1);
            }
        }
        for k in 0..16 {
            if ch.chunks & (1 << k) != 0 {
                t.room_gen[k] = t.room_gen[k].wrapping_add(1);
            }
        }
        self.next = std::mem::replace(&mut self.block, t);
        ch
    }

    /// What moves (movers.rs: the stops table and the movers) into the
    /// block, the two sections' gens moved if their bytes did
    pub fn publish_moving(&mut self, stops: &[[u8; ROWS]; SHAFTS], movers: &[Mover]) -> Changed {
        let t = &mut self.block;
        let mut ch = Changed::default();
        // the car elevators' rows of the stops table are the static publish's
        let mut st = *stops;
        for (k, row) in st.iter_mut().enumerate() {
            for (r, v) in row.iter_mut().enumerate() {
                *v |= self.lift_stops[k][r];
            }
        }
        if bytes(&st[..]) != bytes(&t.stops[..]) {
            t.stops = st;
            ch.sections |= 1 << TW_STOPS;
        }
        let n = movers.len().min(MOVERS);
        let mut m = [Mover::default(); MOVERS];
        m[..n].copy_from_slice(&movers[..n]);
        if n as u16 != t.nmovers || bytes(&m[..]) != bytes(&t.mover[..]) {
            t.mover = m;
            t.nmovers = n as u16;
            ch.sections |= 1 << TW_MOVERS;
        }
        for s in 0..TW_NSECT {
            if ch.sections & (1 << s) != 0 {
                t.gen[s] = t.gen[s].wrapping_add(1);
            }
        }
        ch
    }

    /// the rooms by picture, for a census
    #[allow(dead_code)]
    pub fn census(&self, art: &Art) -> BTreeMap<String, usize> {
        let names: HashMap<u32, &String> = art.art.iter().map(|(n, &i)| (i, n)).collect();
        let mut m = BTreeMap::new();
        for i in 1..self.block.nrooms as usize {
            let w1 = self.block.room[i].w[1];
            if self.block.room[i].w[0] == 0 && w1 == 0 {
                continue;
            }
            let a = w1 & 0xffff;
            *m.entry(names.get(&a).map(|s| s.to_string()).unwrap_or_else(|| format!("#{a}"))).or_insert(0) += 1;
        }
        m
    }
}

/// a family's place in TILE_WIDTHS, for a room's identity
fn family_number(tile: &str) -> u16 {
    TILE_WIDTHS.iter().position(|(n, _)| *n == tile).unwrap_or(99) as u16
}

/// where each section and room chunk lies in the block, in bytes
pub fn section_range(s: usize) -> std::ops::Range<usize> {
    let at = |f: usize, n: usize| f..f + n;
    use std::mem::size_of;
    let cells = 192;
    let rooms = cells + size_of::<[[Cell; STRIDE]; GRID_H]>();
    let fix = rooms + size_of::<[Room; ROOMS]>();
    let stops = fix + size_of::<[Fixture; FIXTURES]>();
    let pal = stops + SHAFTS * ROWS;
    let movers = pal + 64;
    match s {
        TW_CELLS => at(cells, rooms - cells),
        TW_ROOMS => at(rooms, fix - rooms),
        TW_FIX => at(fix, stops - fix),
        TW_STOPS => at(stops, pal - stops),
        TW_PAL => at(pal, 64),
        _ => at(movers, size_of::<[Mover; MOVERS]>()),
    }
}

pub fn chunk_range(k: usize) -> std::ops::Range<usize> {
    let r = section_range(TW_ROOMS);
    let n = ROOM_CHUNK * std::mem::size_of::<Room>();
    r.start + k * n..r.start + (k + 1) * n
}

// ---- the publisher's proofs (graphics design 11, unit 4) ---------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tower_sim::commands::SimCommand;
    use tower_sim::snapshot::SimSnapshot;

    /// the art, packed from the art tree as towerpack packs it
    fn art() -> Option<Art> {
        let a = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tower/art");
        if !a.join("out/4/rooms").is_dir() {
            eprintln!("no rendered art: run python3 ports/tower/art/tools/render.py");
            return None;
        }
        let p = tower_surface::pack::pack(&tower_surface::pack::Options::new(a, 4, "en")).unwrap();
        Some(Art::new(p.block.clone(), &p.manifest()).unwrap())
    }

    /// one of upstream's fixture towers, as harness/dump-snapshots.mjs wrote it
    fn fixture(name: &str) -> Option<(TowerSim, Clock)> {
        let dir = std::env::var_os("TOWER_SNAPSHOTS")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/Volumes/xc/BOX/caches/tower/snapshots"));
        let text = std::fs::read(dir.join(name).join("15-live.json")).ok()?;
        let snap: SimSnapshot = serde_json::from_slice(&text).unwrap();
        Some((TowerSim::from_snapshot(&snap), Clock::new(snap.time.clone())))
    }

    #[test]
    fn the_mapping_table() {
        // design 6.3: vacant, sale, stuck and infested, by family
        let (st, sign) = room_state("office", 0x10, Some(1), 9);
        assert_eq!(st, S_VACANT);
        assert_eq!(sign, signs::banner("for-rent", 9).unwrap());
        assert_eq!(room_state("office", 0x0f, Some(1), 9), (0, 0));
        assert_eq!(room_state("office", 0x00, Some(0), 9).0, S_STUCK);
        let (st, sign) = room_state("condo", 0x18, Some(1), 16);
        assert_eq!(st, S_VACANT | S_SALE);
        assert_eq!(sign, signs::banner("for-sale", 16).unwrap());
        assert_eq!(room_state("condo", 0x17, Some(1), 16), (0, 0));
        assert_eq!(room_state("hotelSingle", 0x38, Some(1), 4).0, S_STUCK | S_INFESTED);
        assert_eq!(room_state("hotelSingle", 0x30, Some(1), 4).0, 0); // dirty is a picture, not a state
        assert_eq!(room_state("restaurant", 0, Some(0), 24).0, S_VACANT);
        assert_eq!(room_state("restaurant", 0, Some(1), 24).0, 0);
        // the families' heights and pictures
        assert_eq!(family("cinema").unwrap().1, 2);
        assert_eq!(family("metro").unwrap().1, 3);
        assert_eq!(family("cathedral").unwrap().1, 5);
        assert_eq!(family("office").unwrap().0.len(), 5);
        assert!(family("floor").is_none() && family("lobby").is_none());
        // upstream's variant hash, at a few places worked out in JavaScript
        assert!((0..20).all(|x| variant(x, 100, 5) < 5));
        assert_eq!(variant(3, 4, 1), 0);
    }

    /// every non-empty cell's room covers it, and every room's cells name
    /// it (but where a shaft is drawn over a lobby)
    fn check_cover(t: &Tower) {
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                let i = t.cell[y][x].room as usize;
                if i == 0 {
                    continue;
                }
                assert!(i < t.nrooms as usize, "cell {x},{y} names room {i} of {}", t.nrooms);
                let r = t.room[i];
                assert!(
                    (r.x0()..r.x0() + r.width()).contains(&(x as u32)) && (r.ytop()..r.ytop() + r.height()).contains(&(y as u32)),
                    "cell {x},{y} names room {i}, which does not cover it ({},{} {}x{})",
                    r.x0(),
                    r.ytop(),
                    r.width(),
                    r.height()
                );
            }
        }
        for i in 1..t.nrooms as usize {
            let r = t.room[i];
            if r.w == [0; 4] {
                continue; // a free index
            }
            for y in r.ytop()..r.ytop() + r.height() {
                for x in r.x0()..r.x0() + r.width() {
                    let k = t.cell[y as usize][x as usize].room as usize;
                    let over = t.room[k].w[1] & 0xffff;
                    assert!(k == i || (k != 0 && t.room[k].height() == 1 && over != (r.w[1] & 0xffff)), "room {i}'s cell {x},{y} names {k}");
                }
            }
        }
    }

    fn sections_equal(a: &Tower, b: &Tower, s: usize) -> bool {
        let r = section_range(s);
        a.as_bytes()[r.clone()] == b.as_bytes()[r]
    }

    #[test]
    fn fixture_towers_cover_and_gens_move_with_bytes() {
        let Some(art) = art() else { return };
        for name in ["mixed", "sky_office", "dense_hotel", "hotel", "condo", "fire"] {
            let Some((mut sim, mut clock)) = fixture(name) else {
                eprintln!("no snapshot of {name} (TOWER_SNAPSHOTS)");
                continue;
            };
            let mut p = Publisher::new(4);
            let look = Look::default();
            let ch = p.publish(&sim, &clock, &art, &look);
            assert!(ch.sections & (1 << TW_CELLS) != 0 && ch.sections & (1 << TW_ROOMS) != 0, "{name}: the first publish");
            check_cover(&p.block);
            let census = p.census(&art);
            assert!(census.contains_key("rooms/lobby"), "{name}: no lobby: {census:?}");
            assert!(!census.keys().any(|k| k.starts_with('#')), "{name}: a room without a picture: {census:?}");
            // nothing changed: nothing moves
            let before = Tower::from_bytes(p.block.as_bytes());
            let ch = p.publish(&sim, &clock, &art, &look);
            assert_eq!(ch.sections, 0, "{name}: a publish of the same tower moved {ch:?}");
            assert_eq!(p.block.gen, before.gen);
            // the simulation run on: each section's gen moves exactly when its bytes do
            for _round in 0..6 {
                for _ in 0..200 {
                    sim.step();
                }
                clock.sync(&sim);
                let before = Tower::from_bytes(p.block.as_bytes());
                let ch = p.publish(&sim, &clock, &art, &look);
                for s in 0..TW_NSECT {
                    let moved = p.block.gen[s] != before.gen[s];
                    assert_eq!(moved, !sections_equal(&p.block, &before, s), "{name}: section {s}");
                    assert_eq!(moved, ch.sections & (1 << s) != 0);
                }
                for k in 0..16 {
                    let r = chunk_range(k);
                    let same = p.block.as_bytes()[r.clone()] == before.as_bytes()[r];
                    assert_eq!(p.block.room_gen[k] != before.room_gen[k], !same, "{name}: room chunk {k}");
                }
                check_cover(&p.block);
            }
            eprintln!("{name}: {} rooms, {} fixtures; {census:?}", p.block.nrooms - 1, p.block.nfixtures);
        }
    }

    #[test]
    fn a_room_keeps_its_index_while_it_stands() {
        let Some(art) = art() else { return };
        let mut sim = TowerSim::create_box("t", "t", None);
        sim.free_build = true;
        let mut clock = Clock::of(&sim);
        let place = |sim: &mut TowerSim, x: i64, y: i64, t: &str| {
            let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: t.into() });
            assert!(r.accepted, "{t} at {x},{y}: {:?}", r.reason);
        };
        for x in 100..140 {
            place(&mut sim, x, 109, "lobby");
        }
        for x in 100..140 {
            place(&mut sim, x, 108, "floor");
        }
        place(&mut sim, 100, 108, "office");
        place(&mut sim, 120, 108, "office");
        let mut p = Publisher::new(4);
        clock.sync(&sim);
        p.publish(&sim, &clock, &art, &Look::default());
        check_cover(&p.block);
        let idx = |p: &Publisher, x: usize| p.block.cell[108][x].room;
        let (a, b) = (idx(&p, 100), idx(&p, 120));
        assert!(a != 0 && b != 0 && a != b);
        // a third office: the first two keep theirs
        place(&mut sim, 110, 108, "office");
        let ch = p.publish(&sim, &clock, &art, &Look::default());
        assert_eq!((idx(&p, 100), idx(&p, 120)), (a, b));
        assert_eq!(ch.chunks, 1, "one room chunk moved");
        // the first removed: its index is free, the others stay
        let r = sim.submit_command(&SimCommand::RemoveTile { x: 101, y: 108 });
        assert!(r.accepted, "{:?}", r.reason);
        p.publish(&sim, &clock, &art, &Look::default());
        assert_eq!(idx(&p, 120), b);
        assert_ne!(idx(&p, 100), a);
        check_cover(&p.block);
    }

    /// the cars where the simulation has them, the people waiting at the
    /// landings in the stops' call bits, in `waiting` and in the queues
    /// drawn, and a shaft's fixture from its machine room to its pit
    #[test]
    fn cars_people_and_shafts() {
        let Some(art) = art() else { return };
        let Some((mut sim, mut clock)) = fixture("sky_office") else { return };
        let mut p = Publisher::new(4);
        let mut scene = crate::movers::Scene::new();
        let (mut most_waiting, mut moved, mut queued_drawn) = (0u32, false, 0usize);
        let mut last: Vec<Mover> = Vec::new();
        let is = |m: &Mover, end: &str| art.sprites.iter().any(|(n, &i)| i as u16 == m.spr & M_SPRITE && n.ends_with(end));
        for round in 0..40 {
            for _ in 0..25 {
                sim.step();
            }
            clock.sync(&sim);
            p.publish(&sim, &clock, &art, &Look::default());
            let (stops, movers) = scene.frame(&sim, &art, &clock, 0.05, 20.0, &p.site);
            let before = Tower::from_bytes(p.block.as_bytes());
            let ch = p.publish_moving(&stops, &movers);
            let t = &p.block;
            assert_eq!(ch.sections & (1 << TW_MOVERS) != 0, t.mover[..] != before.mover[..] || t.nmovers != before.nmovers);
            let cars: usize = sim.live_carriers().iter().map(|c| c.cars.iter().filter(|c| c.active).count()).sum();
            let car_movers: Vec<Mover> = t.mover[..t.nmovers as usize].iter().copied().filter(|m| is(m, "-car")).collect();
            assert_eq!(car_movers.len(), cars, "a mover for each car");
            let waiting_total: u32 = scene.waiting.iter().flat_map(|w| w.iter()).map(|(u, d)| (*u + *d) as u32).sum();
            let standing = t.mover[..t.nmovers as usize].iter().filter(|m| is(m, "-stand-right")).count();
            queued_drawn = queued_drawn.max(standing);
            if round > 0 {
                assert!(standing as u32 <= waiting_total + 40, "no more people queue than wait");
            }
            for (k, c) in sim.live_carriers().iter().enumerate() {
                for row in 0..ROWS {
                    let (up, down) = scene.waiting[k][row];
                    assert_eq!(up > 0, t.stops[k][row] & STOP_UP != 0);
                    assert_eq!(down > 0, t.stops[k][row] & STOP_DOWN != 0);
                    most_waiting = most_waiting.max(up as u32 + down as u32);
                }
                // the shaft's fixture: its top row the highest floor's, a row
                // more than the floors for the pit, its stops row k
                let f = t.fix[k].w;
                assert_eq!((f[0] >> 9) & 127, (GRID_H as i64 - 1 - c.top_served_floor) as u32);
                assert_eq!((f[0] >> 24) & 127, (c.top_served_floor - c.bottom_served_floor + 2) as u32);
                assert_eq!(f[2] & 63, k as u32);
                assert!((f[2] >> 8) & 255 > 0, "the machine room stands above it");
            }
            moved |= !last.is_empty() && last != car_movers;
            last = car_movers;
        }
        assert!(moved, "the cars moved over 1,000 ticks");
        assert!(queued_drawn > 0, "people were drawn queueing");
        eprintln!("sky_office: at most {most_waiting} waiting at one landing, {queued_drawn} drawn queueing at once");
    }

    #[test]
    fn the_clock_keeps_step_with_the_simulation() {
        let Some((mut sim, mut clock)) = fixture("mixed").or_else(|| {
            let s = TowerSim::create_box("t", "t", None);
            let c = Clock::of(&s);
            Some((s, c))
        }) else {
            return;
        };
        for n in [1, 7, 400, 2600, 3000] {
            for _ in 0..n {
                sim.step();
            }
            clock.sync(&sim);
            let t = sim.save_state().time;
            assert_eq!((clock.t.day_tick, clock.t.day_counter, clock.t.weekend_flag), (t.day_tick, t.day_counter, t.weekend_flag));
        }
        assert_eq!(day_tick_to_hour(400), 12.0);
        assert_eq!(day_tick_to_hour(2300), 0.0);
    }
}

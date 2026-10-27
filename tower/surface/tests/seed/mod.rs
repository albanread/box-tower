//! THE SEED OF UNIT 4 (the engine's publisher), here only so that the twin
//! has towers to draw.  It maps a simulation's state to the live block's
//! cells and rooms, for the classic drawing, with no section generations,
//! fixtures, stops or movers.  It reads the simulation only through
//! `TowerSim`'s public calls (cells_to_array, fire_fronts, save_state for
//! the clock), as upstream's client does.  The publisher proper will live
//! in the engine and take over from this; tower-sim is a dev-dependency of
//! tower-surface for these tests only.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use tower_sim::index::TowerSim;
use tower_sim::resources::TILE_WIDTHS;
use tower_surface::block::*;
use tower_surface::pack::Packed;
use tower_surface::signs;

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

/// a family's pictures, as upstream's ROOM_TEXTURES lists them, by our names;
/// its height in floors; its lamp profile
fn family(tile: &str) -> Option<(Vec<String>, u32, u32)> {
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
        "security" => (one("security"), 1, P_SERVICE),
        "housekeeping" => (one("housekeeping"), 1, P_SERVICE),
        "medical" => (one("medical"), 1, P_SERVICE),
        "metro" => (v("metro", 2), 3, P_ALWAYS),
        "cathedral" => (one("cathedral"), 5, P_ENTERTAINMENT),
        _ => return None,
    })
}

fn width_of(tile: &str) -> u32 {
    TILE_WIDTHS.iter().find(|(n, _)| *n == tile).map(|(_, w)| *w as u32).unwrap_or(1)
}

/// The live block for a tower, classic: rooms, lobby runs, the pieces
/// behind (floor, basement, street, soil, rock, roof) and fire in front.
pub fn publish(sim: &TowerSim, art: &Packed) -> Box<Tower> {
    let mut t = Tower::new(art.head().scale);
    let snap = sim.save_state();
    let hour = day_tick_to_hour(snap.time.day_tick);
    t.hour = ((hour * 64.0).round() as u32 % 1536) as u16;
    t.day = snap.time.day_counter as u16;
    if snap.time.weekend_flag != 0 {
        t.flags |= F_WEEKEND;
    }
    t.quality = 0;
    t.view = 0;
    t.back_rgba = 0xff20_2020;
    let a = |n: &str| art.art_of(n) as u16;

    // what is in each cell
    let mut grid: HashMap<(i64, i64), String> = HashMap::new();
    let patches = sim.cells_to_array();
    for p in &patches {
        if !p.is_overlay.unwrap_or(false) {
            grid.insert((p.x, p.y), p.tile_type.clone());
        }
    }

    // the rooms, from the anchors, left to right, top to bottom
    let mut anchors: Vec<_> = patches.iter().filter(|p| p.is_anchor && !p.is_overlay.unwrap_or(false)).collect();
    anchors.sort_by_key(|p| (p.y, p.x));
    let mut n = 1usize;
    let set_room = |t: &mut Tower, x0: u32, y0: u32, wc: u32, hf: u32, room: Room, n: &mut usize| {
        t.room[*n] = room;
        for y in y0..(y0 + hf).min(GRID_H as u32) {
            for x in x0..(x0 + wc).min(GRID_W as u32) {
                t.cell[y as usize][x as usize].room = *n as u16;
            }
        }
        *n += 1;
    };
    for p in &anchors {
        let tile = p.tile_type.as_str();
        if tile == "recyclingCenterLower" && grid.get(&(p.x, p.y - 1)).map(String::as_str) == Some("recyclingCenterUpper") {
            continue; // the upper slice's picture covers both floors
        }
        let Some((pics, hf, profile)) = family(tile) else { continue };
        let (x0, y0, wc) = (p.x as u32, p.y as u32, width_of(tile));
        let us = p.unit_status.unwrap_or(0);
        let active = p.eval_active_flag;
        let hotel = HOTELS.contains(&tile);
        let mut name = pics[variant(p.x, p.y, pics.len())].clone();
        if hotel && us >= 0x28 {
            name.push_str(".dirty");
        }
        if tile == "parking" && p.coverage_flag.unwrap_or(0) == 0 {
            name = "rooms/parking.no-access".into();
        }
        let mut st = 0;
        if (hotel && us >= 0x38) || ((tile == "office" || tile == "condo") && active == Some(0)) {
            st |= S_STUCK;
        }
        if hotel && us >= 0x38 {
            st |= S_INFESTED;
        }
        let vacant = match tile {
            "office" => us > 0x0f,
            "condo" => us > 0x17,
            _ if hotel => false,
            _ => active == Some(0),
        };
        let mut room = Room::place(x0, y0, wc, hf);
        if vacant {
            st |= S_VACANT;
            let kind = if tile == "condo" {
                st |= S_SALE;
                "for-sale"
            } else {
                "for-rent"
            };
            room.set_sign(signs::banner(kind, wc).unwrap_or(0));
        }
        room.set_art(art.art.get(&name).copied().unwrap_or(0));
        room.set_profile(profile);
        room.set_state(st);
        if let Some(e) = p.eval_level.filter(|e| (0..=2).contains(e)) {
            room.set_eval(e as u32 + 1);
        }
        set_room(&mut t, x0, y0, wc, hf, room, &mut n);
    }
    // lobby runs, one room each
    for y in 0..GRID_H as i64 {
        let mut x = 0i64;
        while x < GRID_W as i64 {
            if grid.get(&(x, y)).map(String::as_str) == Some("lobby") {
                let x0 = x;
                while grid.get(&(x, y)).map(String::as_str) == Some("lobby") {
                    x += 1;
                }
                let wc = (x - x0) as u32;
                let mut room = Room::place(x0 as u32, y as u32, wc, 1);
                room.set_art(a("rooms/lobby") as u32);
                room.set_profile(P_LOBBY);
                set_room(&mut t, x0 as u32, y as u32, wc, 1, room, &mut n);
            } else {
                x += 1;
            }
        }
    }
    t.nrooms = n as u16;

    // the pieces behind
    let ground = GROUND as i64; // the first row below the ground
    let built = |x: i64, y: i64| grid.contains_key(&(x, y));
    for y in 0..GRID_H as i64 {
        for x in 0..GRID_W as i64 {
            let c = &mut t.cell[y as usize][x as usize];
            let h = hash((x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b));
            let tile = grid.get(&(x, y)).map(String::as_str);
            c.back = match tile {
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
        }
    }

    // fire in front: the fronts per floor, upstream's left and right
    let (left, right) = sim.fire_fronts();
    for (floor, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
        if l == 0xffff || r == 0xffff || l > r {
            continue;
        }
        let y = GRID_H - 1 - floor;
        for x in l.max(0)..=r.min(GRID_W as i64 - 1) {
            t.cell[y][x as usize].front = a(if x == l {
                "events/fire-left"
            } else if x == r {
                "events/fire-right"
            } else {
                "events/fire"
            });
        }
    }
    t
}

/// the cells (x0, y0, x1, y1) of everything built, and the roof row
pub fn bounds(t: &Tower) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (GRID_W, GRID_H, 0, 0);
    for y in 0..GROUND + 1 {
        for x in 0..GRID_W {
            let c = t.cell[y][x];
            if c.room != 0 || (c.back != 0 && y < GROUND - 1) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    for y in GROUND..GRID_H {
        for x in 0..GRID_W {
            if t.cell[y][x].room != 0 {
                y1 = y1.max(y + 1);
            }
        }
    }
    (x0 as u32, y0 as u32, x1 as u32, y1 as u32)
}

/// how many rooms of each picture, for the test's report
pub fn census(t: &Tower, art: &Packed) -> BTreeMap<String, usize> {
    let names: HashMap<u32, &String> = art.art.iter().map(|(n, &i)| (i, n)).collect();
    let mut m = BTreeMap::new();
    for i in 1..t.nrooms as usize {
        let a = t.room[i].w[1] & 0xffff;
        *m.entry(names.get(&a).map(|s| s.to_string()).unwrap_or_else(|| format!("#{a}"))).or_insert(0) += 1;
    }
    m
}

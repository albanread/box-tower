// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Alban Read
//
// movers.rs -- what moves, a frame at a time (graphics design 5.7, 5.8):
// the stops table (served floors, calls, the landing doors) and the movers
// (elevator cars, the car lift's platform, people, vehicles), made from the
// simulation's state at every frame the engine publishes.
//
// The simulation moves in ticks; the picture moves between them, as
// upstream's client does it:
//
// - Cars are where the simulation has them (`current_floor`), drawn at a
//   fractional floor that follows it smoothly; their doors open while they
//   dwell; their load is how many ride against the shaft's capacity.
// - People waiting for a car stand in a queue at its landing, laid out as
//   upstream's getQueuedSimLayout lays them (left of the shaft to go up,
//   right of it to go down), coloured by the stress of their trip (the
//   tint layer, art/src/people/NOTES.md).  People riding are the cars'
//   load.
// - The simulation has no one walking: a person is in a room, in a queue
//   or in a car.  So the walks between are made here: someone who joins a
//   queue at the lobby from outside walks in from the street; someone who
//   leaves a car walks from the shaft to their room, or out of the lobby
//   to the street, or a few steps along the floor they got out at.  Walks
//   take a moment and are drawn only; nothing of the simulation waits for
//   them.  No walk leaves the floor it is on: it keeps to that floor's run
//   of built cells, and only the ground floor's lobby opens on the street.
// - Fire (BOX): while a fire burns, people come out of the floors above the
//   lobby, walk to the nearer fire escape (publish.rs), go down it flight
//   by flight, across a roof to the next where the floors step back, and
//   away along the street.  Otherwise everyone leaves by the lobby.
// - BOX RULES' medics, guards and patients (box_ext::emergency) where the
//   simulation has them: walking, waiting at a lift, kneeling by the
//   patient, the patient lying where they fell, then carried on the
//   stretcher between two medics.  Riders are the cars' load.
// - Vehicles: BOX RULES' car-elevator traffic and street services as the
//   simulation has them, and passing traffic on the street, drawn behind
//   the building.

use std::collections::{BTreeMap, HashMap, HashSet};

use tower_sim::index::TowerSim;
use tower_sim::resources::OP_SCORE_THRESHOLDS;
use tower_sim::sims::population::sim_key;
use tower_sim::world::RouteState;
use tower_surface::block::*;

use tower_sim::box_ext::emergency::{PersonPhase, Role};

use crate::publish::{Art, Clock, Site};

/// the tint's param bits (design 5.8)
const P_TINTED: u16 = 0x8000;
const UPSTREAM_FAMILY_WIDTHS: [(i64, i64, i64); 9] = [
    // family, width in cells, population
    (3, 4, 1),
    (4, 6, 2),
    (5, 10, 3),
    (7, 9, 6),
    (9, 16, 3),
    (13, 26, 2),
    (14, 16, 6),
    (15, 15, 6),
    (6, 24, 1),
];
const GROUND_FLOOR: i64 = 10;

/// A walk: along `path` (cells across, rows down: a row's floor surface)
/// at `speed` cells a second from `t0`; a storey of stairs counts as three
/// cells walked
#[derive(Clone, Debug)]
struct Walk {
    key: String,
    family: i64,
    top: u8,
    path: Vec<(f64, f64)>,
    t0: f64,
    speed: f64,
    /// a figure other than the person's own (a child's, a dog's)
    fig: Option<&'static str>,
    /// standing still as it goes (on an escalator)
    ride: bool,
}

const STOREY: f64 = 3.0;

impl Walk {
    fn straight(key: &str, family: i64, top: u8, row: i64, x0: f64, x1: f64, t0: f64, dur: f64) -> Walk {
        let speed = ((x1 - x0).abs() / dur.max(0.05)).max(0.1);
        Walk { key: key.to_string(), family, top, path: vec![(x0, row as f64), (x1, row as f64)], t0, speed, fig: None, ride: false }
    }
    fn len(&self) -> f64 {
        self.path.windows(2).map(|w| (w[1].0 - w[0].0).abs().max((w[1].1 - w[0].1).abs() * STOREY)).sum()
    }
    fn done(&self, now: f64) -> bool {
        (now - self.t0) * self.speed >= self.len()
    }
    /// where, which way it faces (-1, 1) and how far it has come
    fn at(&self, now: f64) -> (f64, f64, f64, f64) {
        let mut d = ((now - self.t0) * self.speed).max(0.0);
        let gone = d;
        for w in self.path.windows(2) {
            let ((x0, r0), (x1, r1)) = (w[0], w[1]);
            let l = (x1 - x0).abs().max((r1 - r0).abs() * STOREY);
            if d <= l && l > 0.0 {
                let k = d / l;
                return (x0 + (x1 - x0) * k, r0 + (r1 - r0) * k, if x1 < x0 { -1.0 } else { 1.0 }, gone);
            }
            d -= l;
        }
        let (x, r) = *self.path.last().unwrap();
        (x, r, 1.0, gone)
    }
}

/// what a person was doing at the last frame, and the floor they were
/// going to
#[derive(Clone, Copy, Debug, PartialEq)]
enum Was {
    /// waiting at floor `at`
    Queued { carrier: i64, to: i64, at: i64 },
    Riding { carrier: i64, to: i64 },
}

pub struct Scene {
    t: f64,
    cars: HashMap<(i64, usize), f64>,
    was: HashMap<String, Was>,
    walks: Vec<Walk>,
    /// evacuees due (fractional), how many have come out of this fire, and
    /// of every fire (their names)
    evac_due: f64,
    evac_out: i64,
    evac_n: u64,
    /// BOX RULES: the people out of this fire already drawn going
    evac_seen: HashSet<String>,
    /// where each emergency vehicle and responder is drawn, following the
    /// simulation's steps smoothly
    shown_x: HashMap<String, f64>,
    /// each car's last (in its bay, its driver going), to see it arrive and
    /// to see its people come back
    parked: HashMap<i64, (bool, bool)>,
    /// each person's floor at the last frame, to see a flight of stairs or
    /// an escalator taken
    floor_was: HashMap<String, i64>,
    /// the metro's last train whose passengers were drawn
    metro_cycle: i64,
    /// the clock's hour at the last frame (shifts and the school run start
    /// as it passes their times), and the day
    hour_was: f64,
    /// the game's events drawn: a new star's moment (when it began; a
    /// celebration asked for and not yet begun), the bomb's blast
    celebrate_at: Option<f64>,
    celebrate_asked: bool,
    blast_at: Option<(f64, i64, i64)>,
    last_flags: i64,
    /// people waiting at each shaft's landings, by row: (up, down)
    pub waiting: Vec<[(u16, u16); ROWS]>,
}

fn hash(x: u64) -> u64 {
    let mut x = x.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^ (x >> 29)
}

fn hstr(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// A person's figure: their role's, as a man or a woman, full height or a
/// little shorter (people/gen.py's variants), by a hash of who they are
fn figure(role: &str, key: &str) -> String {
    let h = hstr(key);
    let woman = (h >> 20) & 1 == 1;
    let short = (h >> 21) % 10 < 3;
    format!("people/{role}{}", match (woman, short) {
        (false, false) => "",
        (true, false) => "-f",
        (false, true) => "-s",
        (true, true) => "-fs",
    })
}

/// Who goes with a walker: children behind a resident or a customer now and
/// then, and a dog ahead of a resident
fn companions(w: &Walk) -> Vec<Walk> {
    let h = hash(hstr(&w.key) ^ 0x5eed);
    let (x0, x1) = (w.path[0].0, w.path[w.path.len() - 1].0);
    let dir = if x1 >= x0 { 1.0 } else { -1.0 };
    let shifted = |off: f64, k: &str, fig: &'static str| Walk {
        key: format!("{}{k}", w.key),
        family: w.family,
        top: top_colour(w.family, &format!("{}{k}", w.key), 0),
        path: w.path.iter().map(|&(x, r)| (x + off * dir, r)).collect(),
        t0: w.t0,
        speed: w.speed,
        fig: Some(fig),
        ride: false,
    };
    let mut out = Vec::new();
    let resident = w.family == 9;
    let customer = matches!(w.family, 6 | 10 | 12);
    if (resident || customer) && h % 4 == 0 {
        let n = 1 + (h >> 4) % 2;
        for k in 0..n {
            let fig = if (h >> (8 + k)) & 1 == 0 { "people/child" } else { "people/child-f" };
            out.push(shifted(-0.8 - 0.6 * k as f64, &format!("/c{k}"), fig));
        }
    }
    if resident && (h >> 12) % 5 == 0 {
        out.push(shifted(1.1, "/dog", "people/dog"));
    }
    out
}

/// a person's role, by the family of the room they belong to
fn role(family: i64) -> &'static str {
    match family {
        7 => "worker",
        3 | 4 | 5 => "guest",
        15 => "housekeeping",
        14 => "security",
        13 => "medical",
        _ => "civilian",
    }
}

/// The cathedral's guests (families 0x24..0x28) have their own figures
/// (art/src/events: guest-a, guest-b, walking)
fn cathedral_guest(family: i64, key: &str) -> Option<&'static str> {
    (0x24..=0x28).contains(&family).then(|| if hstr(key) & 1 == 0 { "events/guest-a" } else { "events/guest-b" })
}

/// the colour of a person's top: by stress while waiting, else their own
fn top_colour(family: i64, key: &str, stress: u8) -> u8 {
    match stress {
        2 => 9,
        1 => 8,
        _ => match family {
            15 => 5,
            14 => 6,
            13 => 7,
            _ => (hstr(key) % 5) as u8,
        },
    }
}

fn person_param(frame: u16, key: &str, top: u8, stress: u8) -> u16 {
    let h = hstr(key);
    let legs = if stress == 2 { 3 } else if stress == 1 { 2 } else { (h >> 8) as u16 & 1 };
    let skin = (h >> 12) as u16 & 3;
    (frame & 15) | (top as u16 & 15) << 4 | legs << 8 | skin << 10 | P_TINTED
}

/// quarter units, from cells across and a row's floor (feet on its surface)
fn feet(x_cells: f64, row: f64) -> (u16, u16) {
    let x = (x_cells * UNIT_W as f64 * 4.0).round().clamp(0.0, 65535.0) as u16;
    let y = (((row + 1.0) * UNIT_H as f64 - 1.0) * 4.0).round().clamp(0.0, 65535.0) as u16;
    (x, y)
}

/// The street's row: the ground floor's
const STREET_ROW: i64 = GROUND as i64 - 1;

/// Evacuee `n` of a fire: somewhere on a floor above the lobby, out by the
/// nearer fire escape beside their floor, down it, across a roof to the
/// next where the floors step back, and away along the street
fn evacuee(site: &Site, n: u64, now: f64) -> Option<Walk> {
    let h = hash(n.wrapping_mul(7919) + 13);
    let runs: Vec<(i64, (i64, i64))> = (0..STREET_ROW)
        .flat_map(|y| site.floors.get(y as usize).into_iter().flatten().map(move |&r| (y, r)))
        .collect();
    let total: i64 = runs.iter().map(|(_, r)| r.1 - r.0 + 1).sum();
    if total == 0 {
        return None;
    }
    let mut pick = (h % total as u64) as i64;
    let &(row, run) = runs.iter().find(|(_, r)| {
        let w = r.1 - r.0 + 1;
        if pick < w {
            true
        } else {
            pick -= w;
            false
        }
    })?;
    let family = [7, 3, 9, 7][(h >> 20) as usize % 4];
    escape_walk(site, row, run.0 + pick, family, format!("evac{n}"), now)
}

/// Someone going out in a fire from cell `x` of row `row`: along the floor
/// to the nearer fire escape beside it, down it, across a roof to the next
/// where the floors step back, and away along the street, at a walk
fn escape_walk(site: &Site, row: i64, x: i64, family: i64, key: String, now: f64) -> Option<Walk> {
    let mut row = row;
    let run = *site.floors.get(row as usize)?.iter().find(|r| r.0 <= x && x <= r.1)?;
    let h = hstr(&key);
    let mut x = x as f64 + 0.5;
    let mut path = vec![(x, row as f64)];
    // first the escapes beside this run's ends, then (on a roof) the nearest
    let beside = |e: &crate::publish::Escape| (e.side < 0 && e.x + 2 == run.0) || (e.side > 0 && e.x == run.1 + 1);
    let mut used: Vec<usize> = Vec::new();
    for leg in 0..8 {
        let e = site
            .escapes
            .iter()
            .enumerate()
            .filter(|(i, e)| e.top <= row && row <= e.bottom && !used.contains(i) && (leg > 0 || beside(e)))
            .min_by(|a, b| (a.1.landing() - x).abs().total_cmp(&(b.1.landing() - x).abs()))?;
        used.push(e.0);
        let e = *e.1;
        path.push((e.inner(), row as f64));
        path.push((e.outer(), row as f64));
        for r in row..e.bottom {
            path.push((e.inner(), r as f64 + 0.5));
            path.push((e.outer(), (r + 1) as f64));
        }
        row = e.bottom;
        x = e.outer();
        if row == STREET_ROW {
            path.push((x + e.side as f64 * (10.0 + (h % 9) as f64), row as f64));
            let top = top_colour(family, &key, 0);
            return Some(Walk { key, family, top, path, t0: now, speed: 2.2, fig: None, ride: false });
        }
    }
    None
}

/// Where on a fire escape a climb `progress` of the way along is (cells,
/// row): from the landing beside the floor left, out to the stair's outer
/// end, flight by flight up or down, and in at the landing beside the floor
/// reached
fn climb_at(c: &tower_sim::box_ext::emergency::Climb, progress: f64) -> (f64, f64) {
    let e = &c.escape;
    let (a, b) = (c.from_row, c.to_row);
    let mut path = vec![(e.inner(), a as f64), (e.outer(), a as f64)];
    if b < a {
        for r in (b + 1..=a).rev() {
            path.push((e.inner(), r as f64 - 0.5));
            path.push((e.outer(), (r - 1) as f64));
        }
    } else {
        for r in a..b {
            path.push((e.inner(), r as f64 + 0.5));
            path.push((e.outer(), (r + 1) as f64));
        }
    }
    path.push((e.inner(), b as f64));
    let mut w = Walk { key: String::new(), family: 0, top: 0, path, t0: 0.0, speed: 1.0, fig: None, ride: false };
    w.speed = w.len();
    let (x, row, _, _) = w.at(progress);
    (x, row)
}

/// The nearest way up from floor `floor` at cell `x`: a lift that serves
/// the floor (its middle), or a flight of stairs or an escalator on it
fn way_up(sim: &TowerSim, column_of: &HashMap<i64, (i64, i64)>, floor: i64, x: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut consider = |c: f64| {
        if best.is_none_or(|b| (c - x).abs() < (b - x).abs()) {
            best = Some(c);
        }
    };
    for c in sim.live_carriers() {
        if (c.bottom_served_floor..=c.top_served_floor).contains(&floor) {
            if let Some(&(col, w)) = column_of.get(&c.carrier_id) {
                consider(col as f64 + w as f64 / 2.0);
            }
        }
    }
    let row = GRID_H as i64 - 1 - floor;
    for p in sim.cells_to_array() {
        if p.is_overlay.unwrap_or(false) && (p.y == row || p.y == row + 1) && (p.tile_type == "stairs" || p.tile_type == "escalator") {
            consider(p.x as f64 + 4.0);
        }
    }
    best
}

/// A workplace's shifts: when its staff come in, and when they go home
/// (hours of the day; the tower's own staff and the drawn crowd)
fn shifts(tile: &str) -> (&'static [f64], &'static [f64]) {
    match tile {
        "retail" => (&[9.0], &[18.5]),
        "restaurant" => (&[10.5], &[23.0]),
        "fastFood" => (&[7.0], &[20.5]),
        "cinema" => (&[11.5], &[23.5]),
        "partyHall" => (&[15.5], &[23.0]),
        "security" => (&[7.0, 19.0], &[7.25, 19.25]),
        "housekeeping" | "laundry" => (&[7.5], &[16.5]),
        "medical" => (&[7.5, 19.5], &[7.75, 19.75]),
        "recyclingCenterUpper" | "recyclingCenterLower" => (&[6.0], &[14.0]),
        "metro" => (&[5.0], &[23.5]),
        "kindergarten" => (&[7.25], &[18.0]),
        "school" => (&[7.75], &[16.5]),
        "premierLounge" => (&[10.5], &[21.5]),
        "pool" => (&[6.25], &[22.25]),
        "gym" => (&[5.75], &[21.5]),
        "guestServices" => (&[6.5, 14.5, 22.5], &[6.75, 14.75, 22.75]),
        "convention" => (&[7.5], &[18.5]),
        "lobby" => (&[6.5, 14.5, 22.5], &[6.75, 14.75, 22.75]),
        _ => (&[], &[]),
    }
}

/// The hour `t` passed between `a` and `b` (the clock going round midnight)
fn passed(a: f64, b: f64, t: f64) -> bool {
    if a < 0.0 || a == b {
        false
    } else if a < b {
        a < t && t <= b
    } else {
        t > a || t <= b
    }
}

/// The figure of someone who works in a room, by its family of work
fn staff_family(tile: &str) -> i64 {
    match tile {
        "security" => 14,
        "medical" => 13,
        "housekeeping" | "laundry" => 15,
        _ => 7,
    }
}

impl Scene {
    /// A trip from `a` to `b` (cells, rows) for `key`: along the floor, or
    /// to a lift that serves both floors, unseen while it rides, and on from
    /// the lift on the other floor.  None if no lift serves both.
    #[allow(clippy::too_many_arguments)]
    fn trip(
        &mut self,
        sim: &TowerSim,
        column_of: &HashMap<i64, (i64, i64)>,
        key: &str,
        family: i64,
        fig: Option<&'static str>,
        a: (f64, i64),
        b: (f64, i64),
        t0: f64,
    ) -> bool {
        let top = top_colour(family, key, 0);
        let walk = |path: Vec<(f64, f64)>, t0: f64| Walk { key: key.to_string(), family, top, path, t0, speed: 3.0, fig, ride: false };
        if a.1 == b.1 {
            self.walks.push(walk(vec![(a.0, a.1 as f64), (b.0, b.1 as f64)], t0));
            return true;
        }
        let (fa, fb) = (GRID_H as i64 - 1 - a.1, GRID_H as i64 - 1 - b.1);
        let lift = sim
            .live_carriers()
            .iter()
            .filter(|c| c.carrier_mode != 0)
            .filter(|c| (c.bottom_served_floor..=c.top_served_floor).contains(&fa) && (c.bottom_served_floor..=c.top_served_floor).contains(&fb))
            .filter_map(|c| column_of.get(&c.carrier_id).map(|&(col, w)| col as f64 + w as f64 / 2.0))
            .min_by(|x, y| (x - a.0).abs().total_cmp(&(y - a.0).abs()));
        let Some(lx) = lift else { return false };
        let w1 = walk(vec![(a.0, a.1 as f64), (lx, a.1 as f64)], t0);
        let ride = 1.0 + (fa - fb).abs() as f64 * 0.2;
        let t1 = t0 + w1.len() / w1.speed + ride;
        self.walks.push(w1);
        self.walks.push(walk(vec![(lx, b.1 as f64), (b.0, b.1 as f64)], t1));
        true
    }

    /// Staff coming in to the rooms whose shift starts as the clock passes
    /// it, or going home as one ends; and on a weekday morning and
    /// afternoon the school run (BOX RULES' register: box_ext::people)
    fn shifts_and_school(&mut self, sim: &TowerSim, site: &Site, column_of: &HashMap<i64, (i64, i64)>, hour: f64, weekend: bool, now: f64) {
        let was = self.hour_was;
        self.hour_was = hour;
        if was < 0.0 || site.rooms.is_empty() {
            return;
        }
        let street_row = STREET_ROW;
        let door = |x: f64| match site.lobby {
            Some((l0, l1)) => if (x - l0 as f64) < ((l1 + 1) as f64 - x) { l0 as f64 - 3.0 } else { (l1 + 1) as f64 + 3.0 },
            None => x - 8.0,
        };
        let homes: Vec<&(String, i64, i64, i64)> = site.rooms.iter().filter(|r| r.0 == "condo").collect();
        let day = (now / 60.0) as u64; // (only to vary who comes)
        // the staff
        let mut drawn = 0;
        for (i, (tile, x, w, row)) in site.rooms.iter().enumerate() {
            let (ins, outs) = shifts(tile);
            let coming = ins.iter().any(|&t| passed(was, hour, t));
            let going = outs.iter().any(|&t| passed(was, hour, t));
            if !(coming || going) || drawn > 60 {
                continue;
            }
            let n = match tile.as_str() {
                "lobby" => 1,
                "school" | "convention" | "restaurant" => 3,
                _ => 2,
            };
            for k in 0..n {
                let key = format!("staff{i}:{k}:{day}");
                let h = hash(hstr(&key));
                let at = (*x as f64 + 1.0 + (h % (*w as u64).max(1)) as f64, *row);
                // one in five lives in the tower (when anyone does)
                let from = if !homes.is_empty() && h % 5 == 0 {
                    let home = homes[(h >> 8) as usize % homes.len()];
                    (home.1 as f64 + home.2 as f64 / 2.0, home.3)
                } else {
                    (door(at.0), street_row)
                };
                let (a, b) = if coming { (from, at) } else { (at, from) };
                let family = staff_family(tile);
                if self.trip(sim, column_of, &key, family, None, a, b, now + k as f64 * 0.7 + (h >> 16) as f64 % 1.5) {
                    drawn += 1;
                }
            }
        }
        // the school run: a parent and the children, from home to the
        // tower's kindergarten or school, or out to school outside, and back
        let to_school = passed(was, hour, 7.6);
        let from_school = passed(was, hour, 15.6);
        if weekend || !(to_school || from_school) || homes.is_empty() {
            return;
        }
        let schools: Vec<&(String, i64, i64, i64)> = site.rooms.iter().filter(|r| r.0 == "kindergarten" || r.0 == "school").collect();
        let mut pairs = 0;
        for home in &homes {
            let key = format!("{},{}", home.1, home.3);
            let (_, kids) = tower_sim::box_ext::people::household(&key);
            if kids == 0 || pairs >= 16 {
                continue;
            }
            pairs += 1;
            let h = hash(hstr(&key) ^ 0x5c4001);
            let at_home = (home.1 as f64 + home.2 as f64 / 2.0, home.3);
            let school = if schools.is_empty() { None } else { Some(schools[(h as usize) % schools.len()]) };
            let there = match school {
                Some(s) => (s.1 as f64 + 1.0 + (h >> 8) as f64 % (s.2 as f64 - 2.0).max(1.0), s.3),
                None => (door(at_home.0), street_row),
            };
            let (a, b) = if to_school { (at_home, there) } else { (there, at_home) };
            let t0 = now + (h >> 20) as f64 % 4.0;
            let pk = format!("parent{key}");
            let pf = 9;
            if self.trip(sim, column_of, &pk, pf, None, a, b, t0) {
                for c in 0..kids {
                    let fig = if (h >> (4 + c)) & 1 == 0 { "people/child" } else { "people/child-f" };
                    let off = -0.7 - 0.6 * c as f64;
                    let dir = if b.0 >= a.0 { 1.0 } else { -1.0 };
                    let _ = self.trip(sim, column_of, &format!("{pk}/c{c}"), pf, Some(fig), (a.0 + off * dir, a.1), (b.0 + off * dir, b.1), t0);
                }
            }
        }
    }
}

impl Default for Scene {
    fn default() -> Self {
        Scene::new()
    }
}

impl Scene {
    pub fn new() -> Scene {
        Scene {
            t: 0.0,
            cars: HashMap::new(),
            was: HashMap::new(),
            walks: Vec::new(),
            evac_due: 0.0,
            evac_out: 0,
            evac_n: 0,
            evac_seen: HashSet::new(),
            shown_x: HashMap::new(),
            parked: HashMap::new(),
            floor_was: HashMap::new(),
            metro_cycle: -1,
            hour_was: -1.0,
            celebrate_at: None,
            celebrate_asked: false,
            blast_at: None,
            last_flags: 0,
            waiting: Vec::new(),
        }
    }

    /// A new star: the rating star and fireworks over the roof, now
    pub fn celebrate(&mut self) {
        self.celebrate_asked = true;
    }

    /// A new tower: nothing carried over
    pub fn reset(&mut self) {
        *self = Scene::new();
    }

    /// The scene `dt` seconds on, the simulation running at `ticks` ticks a
    /// second: the stops table and the movers.  `site` is the building as
    /// the publisher last saw it: the lobby (the way in from the street),
    /// the floors' runs and the fire escapes.
    pub fn frame(
        &mut self,
        sim: &TowerSim,
        art: &Art,
        clock: &Clock,
        dt: f64,
        ticks: f64,
        site: &Site,
    ) -> (Box<[[u8; ROWS]; SHAFTS]>, Vec<Mover>) {
        let lobby = site.lobby;
        // the scene's clock runs with the simulation's: walks and passing
        // traffic stop when the game is paused, and keep pace when it is fast
        self.t += dt * ticks / 20.0;
        let now = self.t;
        let spr = |n: &str| art.sprites.get(n).copied().unwrap_or(0) as u16 & M_SPRITE;
        let mut stops = Box::new([[0u8; ROWS]; SHAFTS]);
        let mut cars: Vec<Mover> = Vec::new();
        let mut people: Vec<Mover> = Vec::new();
        let mut vehicles: Vec<Mover> = Vec::new();
        self.waiting.clear();
        let carriers = sim.live_carriers();
        let pace = 1.0; // the scene's clock already keeps the simulation's pace

        // ---- the shafts: calls, cars, doors
        let mut column_of: HashMap<i64, (i64, i64)> = HashMap::new(); // carrier id -> (column, width)
        let mut seen_cars: HashSet<(i64, usize)> = HashSet::new();
        for (k, c) in carriers.iter().enumerate().take(SHAFTS) {
            let w = if c.carrier_mode == 0 { 6 } else { 4 };
            column_of.insert(c.carrier_id, (c.column, w));
            let mut wait = [(0u16, 0u16); ROWS];
            for floor in c.bottom_served_floor..=c.top_served_floor {
                let row = GRID_H as i64 - 1 - floor;
                if !(0..ROWS as i64).contains(&row) {
                    continue;
                }
                let slot = tower_sim::carriers::slot::floor_to_slot(c, floor);
                if slot < 0 {
                    continue;
                }
                let slot = slot as usize;
                let row = row as usize;
                if c.stop_floor_enabled.get(slot).copied().unwrap_or(1) != 0 {
                    stops[k][row] |= STOP_SERVED;
                }
                if let Some(q) = c.floor_queues.get(slot) {
                    let (up, down) = (q.up.count.clamp(0, 0xffff) as u16, q.down.count.clamp(0, 0xffff) as u16);
                    wait[row] = (up, down);
                    if up > 0 {
                        stops[k][row] |= STOP_UP;
                    }
                    if down > 0 {
                        stops[k][row] |= STOP_DOWN;
                    }
                }
            }
            self.waiting.push(wait);
            let (car, doors, load) = match c.carrier_mode {
                0 => ("transport/elevator-express-car", "transport/elevator-express-car-doors", "transport/elevator-express-car-load"),
                2 => ("transport/elevator-service-car", "transport/elevator-service-car-doors", "transport/elevator-service-car-load"),
                _ => ("transport/elevator-car", "transport/elevator-car-doors", "transport/elevator-car-load"),
            };
            let cap = c.assignment_capacity.max(1);
            for (ci, cc) in c.cars.iter().enumerate().filter(|(_, cc)| cc.active) {
                // the car's floor, smoothly after the simulation's: a floor a
                // tick at most, as fast as the clock runs
                let want = cc.current_floor as f64;
                let y = self.cars.entry((c.carrier_id, ci)).or_insert(want);
                let step = (ticks * dt * 1.5).max(dt * 2.0);
                *y = if (want - *y).abs() <= step { want } else { *y + step * (want - *y).signum() };
                if (want - *y).abs() > 6.0 {
                    *y = want;
                }
                seen_cars.insert((c.carrier_id, ci));
                let row = (GRID_H as i64 - 1) as f64 - *y;
                let dwelling = cc.dwell_counter > 0 && (want - *y).abs() < 1e-6;
                let r = row.round() as i64;
                if dwelling && (0..ROWS as i64).contains(&r) {
                    stops[k][r as usize] |= STOP_DOORS;
                }
                let (fx, fy) = feet(c.column as f64 + w as f64 / 2.0, row);
                cars.push(Mover { x: fx, y: fy, spr: spr(car) | M_LIT, param: 0 });
                let lvl = ((cc.assigned_count.max(0) * 4 + cap - 1) / cap).min(4);
                if lvl > 0 {
                    cars.push(Mover { x: fx, y: fy, spr: spr(&format!("{load}.{}", lvl - 1)) | M_LIT, param: 0 });
                }
                let frame = if dwelling { 3 } else { 0 };
                cars.push(Mover { x: fx, y: fy, spr: spr(&format!("{doors}.{frame}")) | M_LIT, param: 0 });
            }
        }
        self.cars.retain(|k, _| seen_cars.contains(k));

        // ---- people: who waits where, who rides, who has just arrived
        let mut routes: HashMap<String, (bool, i64)> = HashMap::new(); // key -> boarded, to
        for c in carriers {
            for r in &c.pending_routes {
                routes.insert(r.sim_id.clone(), (r.boarded, r.destination_floor));
            }
        }
        let (medium, high) = OP_SCORE_THRESHOLDS
            .iter()
            .find(|(s, _)| *s == sim.star_count().min(5))
            .map(|(_, t)| (t[0], t[1]))
            .unwrap_or((80, 200));
        // queues: (carrier, floor, up) -> people, by key
        let mut queues: BTreeMap<(i64, i64, bool), Vec<(String, i64, u8)>> = BTreeMap::new();
        let mut now_was: HashMap<String, Was> = HashMap::new();
        // key -> family, home x, home floor, the floor they are on now
        let mut homes: HashMap<String, (i64, f64, i64, i64)> = HashMap::new();
        for s in sim.live_sims() {
            let RouteState::Carrier { carrier_id, .. } = s.route else { continue };
            let key = sim_key(s);
            let route = routes.get(&key).copied();
            let to = route.map(|r| r.1).unwrap_or(s.destination_floor);
            if route.is_some_and(|r| r.0) {
                now_was.insert(key, Was::Riding { carrier: carrier_id, to });
                continue;
            }
            let up = if s.destination_floor >= 0 { s.destination_floor > s.selected_floor } else { s.floor_anchor >= s.selected_floor };
            let elapsed = if s.last_demand_tick >= 0 { (clock.t.day_tick - s.last_demand_tick + 2600) % 2600 } else { 0 };
            let ticks_waited = (s.elapsed_ticks + elapsed).min(300);
            let stress = if ticks_waited >= high { 2 } else if ticks_waited >= medium { 1 } else { 0 };
            queues.entry((carrier_id, s.selected_floor, up)).or_default().push((key.clone(), s.family_code, stress));
            now_was.insert(key, Was::Queued { carrier: carrier_id, to, at: s.selected_floor });
        }
        for s in sim.live_sims() {
            let key = sim_key(s);
            if self.was.contains_key(&key) && !now_was.contains_key(&key) {
                let (w, pop) = UPSTREAM_FAMILY_WIDTHS
                    .iter()
                    .find(|f| f.0 == s.family_code)
                    .map(|f| (f.1, f.2))
                    .unwrap_or((4, 1));
                let hx = s.home_column as f64 + (s.base_offset as f64 + 0.5) / pop as f64 * w as f64;
                homes.insert(key, (s.family_code, hx, s.floor_anchor, s.selected_floor));
            }
        }
        // the queues drawn, and those who have just joined one at the lobby
        // from outside walking in first
        let mut walking_to: HashMap<String, (f64, i64)> = HashMap::new();
        // BOX RULES: office workers who live in the tower, and where their
        // front door is (cells, row)
        let local_home: HashMap<String, (f64, i64)> = sim
            .box_ext()
            .locals
            .iter()
            .filter_map(|l| {
                let s = sim.live_sims().iter().find(|s| l.who.is(s))?;
                let (x, y) = l.home.split_once(',')?;
                let (x, y): (i64, i64) = (x.parse().ok()?, y.parse().ok()?);
                Some((sim_key(s), (x as f64 + 2.0 + (hstr(&l.home) % 12) as f64, y)))
            })
            .collect();
        for ((carrier, floor, up), mut list) in queues {
            let Some(&(col, w)) = column_of.get(&carrier) else { continue };
            list.sort();
            let row = GRID_H as i64 - 1 - floor;
            for (i, (key, family, stress)) in list.iter().enumerate().take(40) {
                // upstream's layout: QUEUE_START_OFFSET 0.6, the gap 0.2, 0.8 a person
                let x = if up { col as f64 - 0.8 - i as f64 * 0.8 } else { (col + w) as f64 - 0.25 + 0.8 + i as f64 * 0.8 };
                let fresh = !self.was.contains_key(key);
                if fresh && floor == GROUND_FLOOR {
                    if let Some((l0, l1)) = lobby {
                        // in from the street, at the nearer end of the lobby
                        let from = if (x - l0 as f64).abs() < (x - (l1 + 1) as f64).abs() { l0 as f64 - 3.0 } else { (l1 + 1) as f64 + 3.0 };
                        let dur = ((x - from).abs() / (4.0 * pace)).clamp(0.3, 6.0);
                        let top = top_colour(*family, key, 0);
                        let w = Walk::straight(key, *family, top, row, from, x, now, dur);
                        self.walks.extend(companions(&w));
                        self.walks.push(w);
                    }
                } else if let Some(&(hx, _)) = local_home.get(key).filter(|h| fresh && h.1 == row) {
                    // BOX RULES: out of their front door to the lift
                    let dur = ((x - hx).abs() / (4.0 * pace)).clamp(0.3, 6.0);
                    let w = Walk::straight(key, *family, top_colour(*family, key, 0), row, hx, x, now, dur);
                    self.walks.push(w);
                } else if let Some((x0, _)) = site.metro.filter(|m| fresh && m.1 == row) {
                    // BOX RULES: off a train, up from the platform to the
                    // ticket hall and along to the lift
                    let (x0f, r) = (x0 as f64, row as f64);
                    let path = vec![(x0f + 4.0 + (hstr(key) % 18) as f64, r + 1.375), (x0f + 28.5, r + 1.375), (x0f + 23.25, r), (x, r)];
                    let w = Walk { key: key.clone(), family: *family, top: top_colour(*family, key, 0), path, t0: now, speed: 3.0, fig: None, ride: false };
                    self.walks.push(w);
                }
                if self.walks.iter().any(|w| &w.key == key) {
                    walking_to.insert(key.clone(), (x, row));
                    continue;
                }
                let (fx, fy) = feet(x, row as f64);
                let top = top_colour(*family, key, *stress);
                let flip = if up { 0 } else { M_FLIP };
                if let Some(g) = cathedral_guest(*family, key) {
                    people.push(Mover { x: fx, y: fy, spr: spr(g) | flip, param: 0 });
                    continue;
                }
                people.push(Mover {
                    x: fx,
                    y: fy,
                    spr: spr(&format!("{}-stand-right", figure(role(*family), key))) | flip,
                    param: person_param(0, key, top, *stress),
                });
            }
        }
        // out of a car: to their room, or out to the street
        for (key, was) in &self.was {
            if now_was.contains_key(key) {
                continue;
            }
            let Some(&(family, hx, home_floor, now_floor)) = homes.get(key) else { continue };
            let (carrier, to) = match was {
                Was::Queued { carrier, to, at } => {
                    if now_floor == *at {
                        continue; // left the queue without riding: nowhere to walk from
                    }
                    (*carrier, *to)
                }
                Was::Riding { carrier, to } => (*carrier, *to),
            };
            let Some(&(col, w)) = column_of.get(&carrier) else { continue };
            // where they got out: the floor they are on now (the car's
            // destination as it was seen, if the simulation has no floor)
            let floor = if now_floor >= 0 { now_floor } else if to >= 0 { to } else { home_floor };
            let row = GRID_H as i64 - 1 - floor;
            let from = col as f64 + w as f64 / 2.0;
            if let Some((x0, _)) = site.metro.filter(|m| m.1 == row && floor != home_floor) {
                // BOX RULES: home by train, down to the platform
                let (x0f, r) = (x0 as f64, row as f64);
                let path = vec![(from, r), (x0f + 23.25, r), (x0f + 28.5, r + 1.375), (x0f + 4.0 + (hstr(key) % 18) as f64, r + 1.375)];
                let w = Walk { key: key.clone(), family, top: top_colour(family, key, 0), path, t0: now, speed: 3.0, fig: None, ride: false };
                self.walks.push(w);
                continue;
            }
            let to = if let Some(&(lx, _)) = local_home.get(key).filter(|h| h.1 == row && floor != home_floor) {
                lx // BOX RULES: home from work, to their front door
            } else if floor == home_floor {
                site.on_floor(row, hx)
            } else if floor == GROUND_FLOOR {
                // out through the lobby's nearer end to the street
                match lobby {
                    Some((l0, l1)) if (from - l0 as f64) < ((l1 + 1) as f64 - from) => l0 as f64 - 3.0,
                    Some((_, l1)) => (l1 + 1) as f64 + 3.0,
                    None => site.on_floor(row, from + 8.0),
                }
            } else {
                // where they are going on this floor: the venue the
                // simulation chose them (entertainment), else a shop, a
                // restaurant, an amenity ... on it, else any room, else (a
                // floor of nothing) the next lift: they are changing lifts;
                // a few steps along it only where there is nothing at all
                let h = hash(hstr(key));
                let s = sim.live_sims().iter().find(|s| &sim_key(s) == key);
                let venue = s.filter(|s| s.commercial_venue_slot >= 0).and_then(|s| {
                    sim.world().placed_objects.iter().find(|(_, o)| o.linked_record_index == s.commercial_venue_slot).map(|(_, o)| (o.left_tile_index + o.right_tile_index + 1) as f64 / 2.0)
                });
                let on_floor: Vec<&(String, i64, i64, i64)> = site.rooms.iter().filter(|r| r.3 == row && r.0 != "lobby").collect();
                let visiting: Vec<&&(String, i64, i64, i64)> = on_floor
                    .iter()
                    .filter(|r| matches!(r.0.as_str(), "restaurant" | "fastFood" | "retail" | "cinema" | "partyHall") || tower_sim::box_ext::amenities::kind(&r.0).is_some())
                    .collect();
                let room_x = |r: &(String, i64, i64, i64)| r.1 as f64 + 1.0 + (h >> 12) as f64 % (r.2 as f64 - 2.0).max(1.0);
                if let Some(vx) = venue {
                    vx
                } else if !visiting.is_empty() {
                    room_x(visiting[(h as usize) % visiting.len()])
                } else if !on_floor.is_empty() {
                    let r = on_floor.iter().min_by(|a, b| (room_x(a) - from).abs().total_cmp(&(room_x(b) - from).abs())).unwrap();
                    room_x(r)
                } else if let Some(lx) = sim
                    .live_carriers()
                    .iter()
                    .filter(|c| c.carrier_id != carrier && (c.bottom_served_floor..=c.top_served_floor).contains(&floor))
                    .filter_map(|c| column_of.get(&c.carrier_id).map(|&(col, w)| col as f64 + w as f64 / 2.0))
                    .min_by(|a, b| (a - from).abs().total_cmp(&(b - from).abs()))
                {
                    lx
                } else {
                    let d = 2.0 + (h >> 8 & 7) as f64;
                    site.on_floor(row, from + if h & 1 == 0 { d } else { -d })
                }
            };
            let dur = ((to - from).abs() / (4.0 * pace)).clamp(0.3, 6.0);
            let w = Walk::straight(key, family, top_colour(family, key, 0), row, from, to, now, dur);
            self.walks.extend(companions(&w));
            self.walks.push(w);
        }
        self.was = now_was;
        // a flight of stairs or an escalator taken: a person routed by a
        // segment (stairs or escalator) whose floor moved by one since the
        // last frame goes up or down the nearest one between the two floors
        let mut floor_now: HashMap<String, i64> = HashMap::with_capacity(self.floor_was.len());
        for s in sim.live_sims() {
            let key = sim_key(s);
            let f = s.selected_floor;
            if let (RouteState::Segment { .. }, Some(&prev)) = (&s.route, self.floor_was.get(&key)) {
                if (f - prev).abs() == 1 {
                    let lower_row = GRID_H as i64 - 1 - f.min(prev);
                    let x = s.home_column as f64;
                    let link = site
                        .links
                        .iter()
                        .filter(|l| l.2 == lower_row)
                        .min_by(|a, b| ((a.1 as f64 + 4.0) - x).abs().total_cmp(&((b.1 as f64 + 4.0) - x).abs()));
                    if let Some(&(escalator, lx, _)) = link {
                        let (lx, r) = (lx as f64, lower_row as f64);
                        let mut path = vec![(lx + 0.5, r), (lx + 1.0, r), (lx + 7.0, r - 1.0), (lx + 7.5, r - 1.0)];
                        if f < prev {
                            path.reverse();
                        }
                        let top = top_colour(s.family_code, &key, 0);
                        let mut w = Walk { key: key.clone(), family: s.family_code, top, path, t0: now, speed: 1.0, fig: None, ride: escalator };
                        // a flight in about a second and a half, an escalator a little quicker
                        w.speed = w.len() / if escalator { 1.2 } else { 1.6 };
                        self.walks.push(w);
                    }
                }
            }
            floor_now.insert(key, f);
        }
        self.floor_was = floor_now;
        // a fire: people out of the floors above the lobby by the escapes
        let (fl, fr) = sim.fire_fronts();
        let burning = fl.iter().zip(fr.iter()).any(|(&l, &r)| l != 0xffff && r != 0xffff && l <= r);
        // (the tower's people, out in about half a minute of the game's
        // time, and no more of them than live and work above the lobby)
        let population = sim.current_population().max(0);
        if sim.box_rules() {
            // BOX RULES: the people the alarm sent out, each from their own
            // place, a few at a time so that the stairs are not a crush
            let fire = sim.box_views().fire;
            let mut started = 0;
            for &(floor, col, since) in &fire.out {
                let key = format!("evac{floor}.{col}.{since}");
                if started >= 6 || self.evac_seen.contains(&key) {
                    continue;
                }
                self.evac_seen.insert(key.clone());
                started += 1;
                let row = tower_sim::world::GRID_HEIGHT - 1 - floor;
                let family = [7, 3, 9, 7][(hstr(&key) >> 20) as usize % 4];
                if let Some(w) = escape_walk(site, row, col + (hstr(&key) % 4) as i64, family, key, now) {
                    self.walks.push(w);
                }
            }
            if fire.out.is_empty() {
                self.evac_seen.clear();
            }
        } else if burning && !site.escapes.is_empty() && self.evac_out < population {
            let rate = (population as f64 / 30.0).clamp(1.0, 12.0);
            self.evac_due += dt * ticks / 20.0 * rate;
            let mut tries = 0;
            while self.evac_due >= 1.0 && tries < 64 && self.evac_out < population {
                tries += 1;
                self.evac_due -= 1.0;
                self.evac_n += 1;
                if let Some(w) = evacuee(site, self.evac_n, now) {
                    self.walks.push(w);
                    self.evac_out += 1;
                }
            }
        } else if !burning {
            self.evac_due = 0.0;
            self.evac_out = 0;
        }
        // the walks
        self.walks.retain(|w| !w.done(now));
        if self.walks.len() > 2000 {
            let n = self.walks.len() - 2000;
            self.walks.drain(..n);
        }
        for w in &self.walks {
            if now < w.t0 {
                continue; // (not yet: on the way, in a lift)
            }
            let (x, row, dir, gone) = w.at(now);
            let frame = ((gone * 4.0).floor() as u16) & 3; // four frames a cell
            let (fx, fy) = feet(x, row);
            let flip = if dir < 0.0 { M_FLIP } else { 0 };
            if let Some(g) = cathedral_guest(w.family, &w.key) {
                people.push(Mover { x: fx, y: fy, spr: spr(g) | flip, param: 0 });
                continue;
            }
            let fig = match w.fig {
                Some(f) => f.to_string(),
                None => figure(role(w.family), &w.key),
            };
            let (pose, frame) = if w.ride { ("stand", 0) } else { ("walk", frame) };
            let param = if w.fig == Some("people/dog") { frame } else { person_param(frame, &w.key, w.top, 0) };
            people.push(Mover { x: fx, y: fy, spr: spr(&format!("{fig}-{pose}-right")) | flip, param });
        }
        let _ = walking_to;

        // ---- the game's events (art/src/events/NOTES.md): smoke over a
        // fire, the rescue helicopter and its water, the firefighters, the
        // bomb and its blast, a new star's rating star and fireworks
        {
            let es = &sim.world().event_state;
            let (fl, fr) = sim.fire_fronts();
            let mut top_burning: Option<i64> = None;
            for (floor, (&l, &r)) in fl.iter().zip(fr.iter()).enumerate() {
                if l == 0xffff || r == 0xffff || l > r {
                    continue;
                }
                let row = GRID_H as i64 - 1 - floor as i64;
                top_burning = Some(top_burning.map_or(row, |t: i64| t.min(row)));
                let built = |x: i64| site.floors.get(row as usize).is_some_and(|runs| runs.iter().any(|&(a, b)| a <= x && x <= b));
                for x in (l..=r).step_by(3).filter(|&x| built(x)) {
                    let (fx, fy) = feet(x as f64 + 0.5, row as f64);
                    people.push(Mover { x: fx, y: fy, spr: spr("events/smoke"), param: 0 });
                }
                let p = es.helicopter_extinguish_pos;
                if p > 0 && (l..=r).contains(&p) {
                    let (fx, fy) = feet(p as f64 + 0.5, row as f64);
                    people.push(Mover { x: fx, y: fy, spr: spr("events/water"), param: 0 });
                }
            }
            if let (Some(top), true) = (top_burning, es.helicopter_extinguish_pos > 0) {
                // nose left, the tail over the place being put out
                let (fx, fy) = feet(es.helicopter_extinguish_pos as f64 - 3.5, (top - 1) as f64);
                people.push(Mover { x: fx, y: fy, spr: spr("events/helicopter") | M_LIT, param: 0 });
            }
            for h in &es.fire_rescue_helpers {
                if h.status == 3 {
                    continue;
                }
                let (fx, fy) = feet(h.column as f64 + 0.5, (GRID_H as i64 - 1 - h.floor) as f64);
                people.push(Mover { x: fx, y: fy, spr: spr("events/firefighter") | M_FLIP, param: 0 });
            }
            let bomb_row = GRID_H as i64 - 1 - es.bomb_floor;
            if es.game_state_flags & 1 != 0 && es.bomb_floor >= 0 {
                let (fx, fy) = feet(es.bomb_tile as f64 + 0.5, bomb_row as f64);
                people.push(Mover { x: fx, y: fy, spr: spr("events/bomb"), param: 0 });
            }
            if es.game_state_flags & 0x40 != 0 && self.last_flags & 0x40 == 0 {
                self.blast_at = Some((now, es.bomb_tile, bomb_row));
            }
            self.last_flags = es.game_state_flags;
            if let Some((t0, x, row)) = self.blast_at {
                if now - t0 < 0.6 {
                    let (fx, fy) = feet(x as f64 + 0.5, row as f64);
                    people.push(Mover { x: fx, y: fy, spr: spr("events/explosion") | M_LIT, param: 0 });
                } else {
                    self.blast_at = None;
                }
            }
            if std::mem::take(&mut self.celebrate_asked) {
                self.celebrate_at = Some(now);
            }
            if let Some(t0) = self.celebrate_at {
                let age = now - t0;
                // over the highest roof's middle
                let top = site.floors.iter().position(|runs| !runs.is_empty());
                match top {
                    Some(row) if age < 4.0 => {
                        let (a, b) = site.floors[row].iter().fold((i64::MAX, i64::MIN), |(a, b), r| (a.min(r.0), b.max(r.1)));
                        let mid = (a + b + 1) as f64 / 2.0;
                        let row = row as f64;
                        if age < 2.5 {
                            let (fx, fy) = feet(mid, row - 3.0);
                            people.push(Mover { x: fx, y: fy, spr: spr("events/rating-star") | M_LIT, param: 0 });
                        }
                        for (k, (dx, dy, start)) in [(-10.0, 2.0, 0.0), (9.0, 3.0, 0.6), (-2.0, 4.5, 1.2), (14.0, 2.5, 1.9)].iter().enumerate() {
                            let _ = k;
                            if age >= *start && age < start + 1.5 {
                                let (fx, fy) = feet(mid + dx, row - dy);
                                people.push(Mover { x: fx, y: fy, spr: spr("events/fireworks") | M_LIT, param: 0 });
                            }
                        }
                    }
                    _ => self.celebrate_at = None,
                }
            }
        }

        // ---- the metro: a train every METRO_PERIOD seconds of the game's
        // time, in from the left tunnel, stopping at the platform with its
        // doors open while passengers get off (along the platform and up the
        // stairs to the ticket hall) and on (down them), and away to the right
        if let Some((x0, top)) = site.metro {
            const METRO_PERIOD: f64 = 36.0;
            let cycle = (now / METRO_PERIOD).floor() as i64;
            let t = now - cycle as f64 * METRO_PERIOD;
            let (x0f, rail) = (x0 as f64, (top + 2) as f64);
            let stop = x0f + 0.75;
            let ease_in = |k: f64| k * k;
            let ease_out = |k: f64| 1.0 - (1.0 - k) * (1.0 - k);
            let (pos, open) = match t {
                t if t < 5.0 => (Some(stop - 28.0 + 28.0 * ease_out(t / 5.0)), false),
                t if t < 7.0 => (Some(stop), false),
                t if t < 17.0 => (Some(stop), true),
                t if t < 19.0 => (Some(stop), false),
                t if t < 24.0 => (Some(stop + 30.0 * ease_in((t - 19.0) / 5.0)), false),
                _ => (None, false),
            };
            if let Some(x) = pos {
                for (car, left) in [("rear", x), ("front", x + 14.0)] {
                    // out of the tunnel's mouths only: a car is seen once it is
                    // wholly inside the station
                    if left >= x0f && left + 14.0 <= x0f + 30.0 {
                        let (fx, fy) = feet(left + 7.0, rail);
                        let name = format!("vehicles/metro-car-{car}{}", if open { "-open" } else { "" });
                        vehicles.push(Mover { x: fx, y: fy, spr: spr(&name) | M_LIT, param: 0 });
                    }
                }
            }
            if t >= 7.5 && self.metro_cycle != cycle {
                self.metro_cycle = cycle;
                let platform = top as f64 + 1.375;
                let hall = top as f64 - 0.04;
                let doors = [3.45, 9.95, 17.45, 23.95];
                let h = hash(cycle as u64 + 991);
                let off = 3 + (h % 5) as usize;
                let on = 2 + ((h >> 8) % 3) as usize;
                for k in 0..off + on {
                    let hk = hash(h ^ k as u64);
                    let door = stop + doors[(hk % 4) as usize];
                    let mut path = vec![
                        (door, platform),
                        (x0f + 28.5, platform),
                        (x0f + 23.25, hall),
                        (x0f + 14.0 - (hk >> 8 & 7) as f64, hall),
                    ];
                    let boarding = k >= off;
                    if boarding {
                        path.reverse();
                    }
                    let family = [7, 9, 3, 9, 7][(hk >> 16) as usize % 5];
                    let key = format!("metro{cycle}:{k}");
                    let top_c = top_colour(family, &key, 0);
                    let w = Walk {
                        key,
                        family,
                        top: top_c,
                        path,
                        t0: now + if boarding { 0.0 } else { 0.4 * k as f64 },
                        speed: 3.0,
                        fig: None,
                        ride: false,
                    };
                    self.walks.extend(companions(&w));
                    self.walks.push(w);
                }
            }
        }

        // ---- staff coming and going, and the school run (BOX RULES)
        if sim.box_rules() {
            self.shifts_and_school(sim, site, &column_of, clock.hour(), clock.weekend(), now);
        }

        // ---- BOX RULES' medics, guards and patients
        let views = sim.box_views();
        let step = (ticks * dt * 0.75).max(dt); // a little faster than they walk
        let follow = |shown: &mut HashMap<String, f64>, key: String, want: f64, step: f64| -> f64 {
            let x = shown.entry(key).or_insert(want);
            *x = if (want - *x).abs() <= step || (want - *x).abs() > 6.0 { want } else { *x + step * (want - *x).signum() };
            *x
        };
        let mut seen: HashSet<String> = HashSet::new();
        // the patients on stretchers: incident -> where, which way
        let mut stretchers: HashMap<i64, (f64, f64, i64)> = HashMap::new();
        for p in &views.people {
            if p.role == Role::Patient && !p.in_lift && !matches!(p.phase, PersonPhase::Lying | PersonPhase::Admitted) {
                stretchers.insert(p.incident, (p.x, p.row, p.facing));
            }
        }
        // the bearers: each incident's medics (or staff) in order, the first
        // at the stretcher's head, the next at its foot
        let mut bearers: HashMap<i64, Vec<i64>> = HashMap::new();
        for p in &views.people {
            if matches!(p.role, Role::Medic | Role::Staff) {
                bearers.entry(p.incident).or_default().push(p.id);
            }
        }
        for v in bearers.values_mut() {
            v.sort_unstable();
        }
        for p in &views.people {
            if p.in_lift || matches!(p.phase, PersonPhase::InAmbulance | PersonPhase::Admitted) {
                continue;
            }
            let key = format!("er{}", p.id);
            seen.insert(key.clone());
            let walking = matches!(p.phase, PersonPhase::Walking | PersonPhase::Carried | PersonPhase::Stairs);
            let mut x = if walking { follow(&mut self.shown_x, key.clone(), p.x, step) } else { follow(&mut self.shown_x, key.clone(), p.x, 1e9) };
            // on a fire escape: up or down its flights, as far as the climb
            // has come (the bearers a little behind and ahead of the stretcher)
            let mut row = p.row;
            let climbing = p.climb.is_some();
            if let Some(c) = &p.climb {
                let lead = match p.role {
                    Role::Patient => 0.0,
                    _ if p.id % 2 == 0 => 0.04,
                    _ => -0.04,
                };
                let (cx, crow) = climb_at(c, (c.progress + lead).clamp(0.0, 1.0));
                x = cx;
                row = crow;
            }
            let flip = if p.facing < 0 { M_FLIP } else { 0 };
            let (fam, top) = if p.role == Role::Guard { (14, 6u8) } else { (13, 7u8) };
            if p.role == Role::Patient {
                let name = if p.phase == PersonPhase::Lying { "people/patient-lying" } else { "people/stretcher-right" };
                let (fx, fy) = feet(x, row);
                people.push(Mover { x: fx, y: fy, spr: spr(name) | flip, param: 0 });
                continue;
            }
            // a medic with the stretcher: one at each end of it
            let moving = walking || climbing;
            if let Some(&(sx, srow, facing)) = stretchers.get(&p.incident) {
                if !climbing && p.role != Role::Guard && (p.x - sx).abs() < 1.0 && (p.row - srow).abs() < 0.01 {
                    let rank = bearers.get(&p.incident).and_then(|v| v.iter().position(|&i| i == p.id)).unwrap_or(0);
                    let end = if rank % 2 == 0 { 1.0 } else { -1.0 };
                    let k = format!("er{}s", p.incident);
                    seen.insert(k.clone());
                    x = follow(&mut self.shown_x, k, sx, step) + end * 1.6 * facing as f64;
                }
            }
            let k = format!("{fam}:{}", p.id);
            let name = match p.phase {
                PersonPhase::Treating => format!("{}-kneel-right", figure(p.family, &k)),
                _ if moving => format!("{}-walk-right", figure(p.family, &k)),
                _ => format!("{}-stand-right", figure(p.family, &k)),
            };
            let frame = if moving { ((x * 4.0).abs().floor() as u16) & 3 } else { 0 };
            let (fx, fy) = feet(x, row);
            people.push(Mover { x: fx, y: fy, spr: spr(&name) | flip, param: person_param(frame, &k, top, 0) });
        }

        // ---- vehicles: BOX RULES' traffic and street services, and the
        // street's own passing traffic, behind the building
        let street_row = (GROUND - 1) as f64;
        for p in &views.platforms {
            let (fx, fy) = feet(p.column as f64 + p.width as f64 / 2.0, p.row);
            people.push(Mover { x: fx, y: fy, spr: spr("transport/car-lift-platform"), param: 0 });
        }
        // BOX RULES' car parks: a car in its bay is drawn nose out in the
        // space (one car a space, the last to come; the space stands for
        // several); one to four people get out of each car that parks and
        // walk to the nearest lift, stairs or escalator on that floor, and
        // walk back to it when its driver is going
        let mut in_bays: BTreeMap<(i64, i64), (i64, &str)> = BTreeMap::new();
        let mut seen_cars: HashMap<i64, (bool, bool)> = HashMap::new();
        for v in &views.vehicles {
            let now_state = (v.in_bay, v.going);
            let was = self.parked.get(&v.id).copied().unwrap_or((false, false));
            seen_cars.insert(v.id, now_state);
            let row = v.bay_row as i64;
            let bay_mid = v.bay_x + 2.0;
            if (v.in_bay && !was.0) || (v.in_bay && v.going && !was.1) {
                let out = !v.going;
                if let Some(to) = way_up(sim, &column_of, GRID_H as i64 - 1 - row, bay_mid) {
                    let h = hash(v.id as u64 * 31 + 7);
                    let n = 1 + (h % 4) as i64;
                    for k in 0..n {
                        let family = [7, 9, 7, 3][(h >> (8 + k * 2)) as usize % 4];
                        let key = format!("car{}:{k}", v.id);
                        let off = (k as f64 - (n - 1) as f64 / 2.0) * 0.7;
                        let (a, b) = if out { (bay_mid + off, to) } else { (to, bay_mid + off) };
                        let dur = ((a - b).abs() / 4.0).clamp(0.6, 3.0);
                        let top = top_colour(family, &key, 0);
                        self.walks.push(Walk::straight(&key, family, top, row, a, b, now + k as f64 * 0.3, dur));
                    }
                }
            }
            if v.in_bay {
                let e = in_bays.entry((v.bay_x as i64, row)).or_insert((v.id, v.style.as_str()));
                if v.id > e.0 {
                    *e = (v.id, v.style.as_str());
                }
                continue;
            }
            let side = if v.facing < 0 { "left" } else { "right" };
            let (fx, fy) = feet(v.x + v.length / 2.0, v.row);
            let moving = matches!(v.phase, tower_sim::box_ext::car_elevator::VehiclePhase::Approaching);
            let frame = if moving { ((now * 6.0) as u16) & 1 } else { 0 };
            vehicles.push(Mover { x: fx, y: fy, spr: spr(&format!("vehicles/{}-{side}", v.style)), param: frame });
        }
        self.parked = seen_cars;
        for ((bx, row), (_, style)) in in_bays {
            let (fx, fy) = feet(bx as f64 + 2.0, row as f64);
            vehicles.push(Mover { x: fx, y: fy, spr: spr(&format!("vehicles/{style}-front")), param: 0 });
        }
        // the service vehicles: the body (rolling, or the ambulance parked
        // with its back open), the beacons over it; along the street
        // smoothly between the simulation's steps
        let vstep = (ticks * dt * 2.5).max(dt);
        for (i, v) in views.service_vehicles.iter().enumerate() {
            let side = if v.facing < 0 { "left" } else { "right" };
            let key = format!("sv{i}:{}", v.style);
            seen.insert(key.clone());
            let x = follow(&mut self.shown_x, key, v.x, vstep);
            let (fx, fy) = feet(x + v.length / 2.0, v.row);
            let body = if v.style == "ambulance" && !v.moving {
                format!("vehicles/ambulance-open-{side}")
            } else {
                format!("vehicles/{}-{side}", v.style)
            };
            let roll = if v.moving { ((now * 6.0) as u16) & 1 } else { 0 };
            vehicles.push(Mover { x: fx, y: fy, spr: spr(&body), param: roll });
            if v.beacon {
                let beacon = format!("vehicles/{}-{side}-beacon", v.style);
                vehicles.push(Mover { x: fx, y: fy, spr: spr(&beacon), param: ((now * 6.0) as u16) & 3 });
            }
        }
        self.shown_x.retain(|k, _| seen.contains(k));
        const STYLES: [&str; 6] = ["saloon", "hatchback", "estate", "van", "taxi", "pickup"];
        let world = GRID_W as f64 + 20.0;
        for i in 0..10u64 {
            let h = hash(i + 77);
            let right = h & 1 == 0;
            let speed = 5.0 + (h >> 8 & 7) as f64; // cells a second
            let gap = 60.0 + (h >> 16 & 63) as f64;
            let span = world + gap;
            let pos = (now * speed + (h >> 24 & 1023) as f64).rem_euclid(span) - 10.0;
            let x = if right { pos } else { world - 20.0 - pos };
            let side = if right { "right" } else { "left" };
            let style = STYLES[(h >> 40) as usize % STYLES.len()];
            let (fx, fy) = feet(x, street_row);
            vehicles.push(Mover { x: fx, y: fy, spr: spr(&format!("vehicles/{style}-{side}")) | M_BEHIND, param: ((now * speed) as u16) & 1 });
        }

        // the list, in the design's order: cars, then people, then vehicles
        let mut movers = cars;
        movers.extend(people);
        movers.extend(vehicles);
        movers.truncate(MOVERS);
        (stops, movers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::{Look, Publisher};
    use std::path::PathBuf;
    use tower_sim::snapshot::SimSnapshot;

    fn art() -> Option<Art> {
        let a = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tower/art");
        if !a.join("out/4/rooms").is_dir() {
            return None;
        }
        let p = tower_surface::pack::pack(&tower_surface::pack::Options::new(a, 4, "en")).unwrap();
        Some(Art::new(p.block.clone(), &p.manifest()).unwrap())
    }

    /// The amenities play-test tower (caches/tower/Amenities.json, written
    /// (the amenities play-test tower): nobody is drawn getting out of a lift
    /// on a floor they are not on -- the family and their dog walking home
    /// from nowhere
    #[test]
    fn people_get_out_where_they_are() {
        let Some(art) = art() else { return };
        let Ok(text) = std::fs::read("/Volumes/xc/BOX/caches/tower/Amenities.json") else { return };
        let snap: SimSnapshot = serde_json::from_slice(&text).unwrap();
        let mut sim = TowerSim::from_snapshot(&snap);
        let mut clock = Clock::new(snap.time.clone());
        let mut p = Publisher::new(4);
        p.publish(&sim, &clock, &art, &Look::default());
        let mut scene = Scene::new();
        let mut seen: HashSet<String> = HashSet::new();
        let (mut walks, mut wrong) = (0, 0);
        for tick in 0..2 * 2600 {
            sim.step();
            clock.sync(&sim);
            if tick % 200 == 0 {
                p.publish(&sim, &clock, &art, &Look::default());
            }
            scene.frame(&sim, &art, &clock, 0.05, 20.0, &p.site);
            for w in &scene.walks {
                if !seen.insert(format!("{}@{}", w.key, w.t0)) {
                    continue;
                }
                // a sim's own walk (keys of other walks have a prefix)
                let Some(s) = sim.live_sims().iter().find(|s| sim_key(s) == w.key) else { continue };
                let row = w.path[0].1.round() as i64;
                if row == STREET_ROW || w.path.iter().any(|p| p.1.round() as i64 != row) {
                    continue; // from the street, or by stairs or the metro
                }
                walks += 1;
                let floor = GRID_H as i64 - 1 - row;
                if floor != s.selected_floor && floor != s.floor_anchor {
                    wrong += 1;
                } else if floor == s.floor_anchor && s.selected_floor != s.floor_anchor && s.selected_floor >= 0 {
                    wrong += 1; // drawn home while the sim is elsewhere
                }
            }
        }
        eprintln!("{walks} walks out of lifts, {wrong} on a floor the person was not on");
        assert!(walks > 0);
        assert_eq!(wrong, 0, "{wrong} of {walks}");
    }

    /// by the sim's amenities test): through a weekday morning, staff come
    /// in to their rooms and the children are taken to school
    #[test]
    fn staff_come_in_and_the_children_go_to_school() {
        let Some(art) = art() else { return };
        let Ok(text) = std::fs::read("/Volumes/xc/BOX/caches/tower/Amenities.json") else { return };
        let snap: SimSnapshot = serde_json::from_slice(&text).unwrap();
        let mut sim = TowerSim::from_snapshot(&snap);
        let mut clock = Clock::new(snap.time.clone());
        let mut p = Publisher::new(4);
        p.publish(&sim, &clock, &art, &Look::default());
        let mut scene = Scene::new();
        let (mut staff, mut school, mut weekday_morning) = (0, 0, false);
        for tick in 0..2 * 2600 {
            sim.step();
            clock.sync(&sim);
            if tick % 200 == 0 {
                p.publish(&sim, &clock, &art, &Look::default());
            }
            scene.frame(&sim, &art, &clock, 0.05, 20.0, &p.site);
            weekday_morning |= !clock.weekend() && (7.0..8.0).contains(&clock.hour());
            staff = staff.max(scene.walks.iter().filter(|w| w.key.starts_with("staff")).count());
            school = school.max(scene.walks.iter().filter(|w| w.key.starts_with("parent")).count());
        }
        eprintln!("staff walks {staff}, school run walks {school}");
        assert!(weekday_morning, "the run never reached a weekday morning");
        assert!(staff > 0, "no staff came in");
        assert!(school > 0, "no child was taken to school");
    }
}

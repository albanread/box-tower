//! Not tests: a play-through of a mixed tower, BOX RULES and classic side by
//! side, day by day, to see whether the game makes sense
//! (`cargo test --release --test playthrough -- --ignored --nocapture`).

use std::collections::BTreeMap;

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{y_to_floor, RouteState, GROUND_Y};

const G: i64 = GROUND_Y;

fn place(sim: &mut TowerSim, log: &mut Vec<String>, x: i64, y: i64, tile: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile.to_string() });
    if !r.accepted {
        log.push(format!("{tile}@{x},{}: {:?}", y_to_floor(y) - y_to_floor(G), r.reason));
    }
}

/// A mixed tower as a player would have it at four stars
pub fn mixed(box_rules: bool) -> (TowerSim, Vec<String>) {
    let mut sim = TowerSim::create_box("play", "Play", None);
    sim.set_box_rules(box_rules);
    sim.set_star_count(4);
    sim.free_build = true;
    let mut log = Vec::new();
    let l = &mut log;
    for x in 100..=219 {
        place(&mut sim, l, x, G, "lobby");
        for b in 1..=3 {
            place(&mut sim, l, x, G + b, "floor");
        }
        for f in 1..=15 {
            place(&mut sim, l, x, G - f, "floor");
        }
    }
    // the metro under the lobby's left end
    place(&mut sim, l, 100, G + 1, "metro");
    // lifts from B2 up, a service lift, stairs to the shops
    for f in -2..=15 {
        place(&mut sim, l, 150, G - f, "elevator");
        place(&mut sim, l, 162, G - f, "elevator");
    }
    for f in 0..=15 {
        place(&mut sim, l, 174, G - f, "elevator");
    }
    for f in 1..=15 {
        place(&mut sim, l, 186, G - f, "elevatorService");
    }
    for f in 0..=1 {
        place(&mut sim, l, 178, G - f, "stairs");
    }
    // shops and food on 1 and 2
    for x in [100, 112, 124, 136] {
        place(&mut sim, l, x, G - 1, "retail");
    }
    place(&mut sim, l, 192, G - 1, "fastFood");
    place(&mut sim, l, 100, G - 2, "restaurant");
    place(&mut sim, l, 124, G - 2, "restaurant");
    place(&mut sim, l, 192, G - 2, "fastFood");
    // offices on 3 to 8
    for f in 3..=8 {
        for x in [100, 109, 118, 127, 136, 192, 201, 210] {
            place(&mut sim, l, x, G - f, "office");
        }
    }
    // homes on 9 to 11
    for f in 9..=11 {
        for x in [100, 116, 132, 192] {
            place(&mut sim, l, x, G - f, "condo");
        }
    }
    // hotel on 12 to 14
    for f in 12..=14 {
        for i in 0..8 {
            place(&mut sim, l, 100 + 6 * i, G - f, "hotelTwin");
        }
        for i in 0..4 {
            place(&mut sim, l, 192 + 6 * i, G - f, "hotelTwin");
        }
    }
    // services on 15
    place(&mut sim, l, 100, G - 15, "housekeeping");
    place(&mut sim, l, 116, G - 15, "security");
    place(&mut sim, l, 192, G - 15, "medical");
    if box_rules {
        place(&mut sim, l, 132, G - 15, "laundry");
        place(&mut sim, l, 210, G - 2, "sprinklers");
        place(&mut sim, l, 120, G, "guestServices");
        place(&mut sim, l, 132, G + 1, "gym");
        place(&mut sim, l, 186, G + 2, "pool");
        // the car lift to B1, out at 230, and spaces on B1
        for x in 220..230 {
            place(&mut sim, l, x, G + 1, "floor");
        }
        place(&mut sim, l, 230, G + 1, "carElevator");
        for x in (192..=216).step_by(4) {
            place(&mut sim, l, x, G + 1, "parking");
        }
    }
    sim.free_build = false;
    sim.set_cash(5_000_000);
    (sim, log)
}

#[derive(Default, Debug)]
pub struct Day {
    pub cash: i64,
    pub population: i64,
    pub stars: i64,
    /// family -> [unrated, bad, fair, good]
    pub evals: BTreeMap<&'static str, [usize; 4]>,
    pub stress: BTreeMap<&'static str, i64>,
    pub idle_never: usize,
    pub long_waits: usize,
    pub notes: BTreeMap<String, usize>,
    pub hotel_nights: usize,
    pub condos_sold: usize,
    pub offices_let: usize,
    pub parked_cars: usize,
    /// the period's ledgers so far, at the day's end: (index, amount)
    pub income: Vec<(usize, i64)>,
    pub expense: Vec<(usize, i64)>,
    pub box_income: i64,
    /// sims with no trip by family code, BOX flows at the day's end
    pub never_by: BTreeMap<i64, usize>,
    pub held: String,
}

fn family(code: i64) -> Option<&'static str> {
    Some(match code {
        3 | 4 | 5 => "hotel",
        6 => "restaurant",
        7 => "office",
        9 => "condo",
        10 => "retail",
        12 => "fastFood",
        _ => return None,
    })
}

pub fn day(sim: &mut TowerSim) -> Day {
    let mut d = Day::default();
    let mut longest_wait = 0;
    for _ in 0..2600 {
        sim.step();
        for n in sim.drain_notifications() {
            *d.notes.entry(n.message).or_default() += 1;
        }
        for p in sim.drain_prompts() {
            *d.notes.entry(format!("prompt: {:?}", p.prompt_kind)).or_default() += 1;
        }
        let t = sim.time().day_tick;
        if t == 2530 {
            let l = sim.ledger();
            d.income = l.income_ledger.iter().enumerate().filter_map(|(i, v)| v.to_number().filter(|n| *n != 0).map(|n| (i, n))).collect();
            d.expense = l.expense_ledger.iter().enumerate().filter(|(_, v)| **v != 0).map(|(i, v)| (i, *v)).collect();
            let b = sim.box_ext();
            d.box_income = b.period_parking + b.period_amenity_income - b.period_amenity_upkeep;
        }
        if t == 2150 {
            d.hotel_nights =
                sim.world().placed_objects.values().filter(|o| [3, 4, 5].contains(&o.object_type_code) && o.unit_status < 0x18).count();
            d.parked_cars = sim.box_ext().vehicles.iter().filter(|v| v.in_bay).count();
        }
        if t % 50 == 0 {
            let now = t;
            let waits = sim
                .world()
                .sims
                .iter()
                .filter(|s| matches!(s.route, RouteState::Carrier { .. }) && now - s.queue_tick > 300 && now >= s.queue_tick)
                .count();
            longest_wait = longest_wait.max(waits);
        }
    }
    d.long_waits = longest_wait;
    let w = sim.world();
    d.cash = sim.cash();
    d.population = sim.current_population();
    d.stars = sim.star_count();
    for o in w.placed_objects.values() {
        let Some(f) = family(o.object_type_code) else { continue };
        let e = d.evals.entry(f).or_default();
        match o.eval_level {
            0 => e[1] += 1,
            1 => e[2] += 1,
            2 => e[3] += 1,
            _ => e[0] += 1,
        }
        if o.eval_score >= 0 {
            *d.stress.entry(f).or_default() += o.eval_score;
        }
        if o.object_type_code == 9 && o.unit_status < 0x18 {
            d.condos_sold += 1;
        }
        if o.object_type_code == 7 && o.unit_status <= 0x0f {
            d.offices_let += 1;
        }
    }
    for (f, total) in d.stress.iter_mut() {
        let n: usize = d.evals[f][1..].iter().sum::<usize>().max(1);
        *total /= n as i64;
    }
    d.idle_never = w.sims.iter().filter(|s| family(s.family_code).is_some() && s.trip_count == 0).count();
    for s in w.sims.iter().filter(|s| s.trip_count == 0) {
        *d.never_by.entry(s.family_code).or_default() += 1;
    }
    let b = sim.box_ext();
    let riders = (0..w.sims.len() as i64).filter(|&i| tower_sim::box_ext::metro::arrival_floor(w, i).is_some()).count();
    d.held = format!(
        "commuters held {} fire out {} locals {} riders {} incidents {} residents' cars {}",
        b.commuters.len(),
        b.fire.out.len(),
        b.locals.len(),
        riders,
        b.emergency.incidents.len(),
        b.vehicles.iter().filter(|v| v.resident).count()
    );
    d
}

#[test]
#[ignore = "a report, not a test"]
fn play_two_weeks() {
    let days: i64 = std::env::var("DAYS").ok().and_then(|s| s.parse().ok()).unwrap_or(14);
    let (mut b, blog) = mixed(true);
    let (mut c, clog) = mixed(false);
    eprintln!("placement refused, BOX: {blog:?}");
    eprintln!("placement refused, classic: {clog:?}");
    for n in 1..=days {
        if n == 6 {
            // a fire on day 6 in both
            b.box_test_event(true);
            c.box_test_event(true);
        }
        let db = day(&mut b);
        let dc = day(&mut c);
        for (name, d) in [("BOX", &db), ("classic", &dc)] {
            eprintln!(
                "day {n:2} {name:7} cash {:>10} pop {:5} stars {} let {:2} sold {:2} hotel {:3} cars {:2} waits {:3} never {:4}",
                d.cash, d.population, d.stars, d.offices_let, d.condos_sold, d.hotel_nights, d.parked_cars, d.long_waits, d.idle_never
            );
            eprintln!("        evals [unrated bad fair good] {:?}  stress {:?}", d.evals, d.stress);
            eprintln!("        income {:?} expense {:?} box {}", d.income, d.expense, d.box_income);
            eprintln!("        never by family {:?}; {}", d.never_by, d.held);
            if !d.notes.is_empty() {
                eprintln!("        notes {:?}", d.notes);
            }
        }
    }
}


/// Not a test: the mixed tower (BOX RULES) at 7:30 on its fourth day, for
/// the play tests (`PLAY_TOWER=path cargo test --release --test playthrough
/// write -- --ignored`).
#[test]
#[ignore = "writes a tower for the play tests"]
fn write_the_mixed_tower() {
    let Ok(out) = std::env::var("PLAY_TOWER") else { return };
    let (mut sim, _) = mixed(true);
    for _ in 0..3 * 2600 {
        sim.step();
    }
    while sim.time().day_tick != 40 {
        sim.step();
    }
    std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
}

/// Not a test: people who leave a lift's queue without riding, by family
#[test]
#[ignore = "a report"]
fn queue_leavers() {
    let text = std::fs::read("/Volumes/xc/BOX/caches/tower/Amenities.json").unwrap();
    let snap: tower_sim::snapshot::SimSnapshot = serde_json::from_slice(&text).unwrap();
    let mut sim = TowerSim::from_snapshot(&snap);
    let mut was: std::collections::HashMap<(i64, i64, i64, i64), (i64, i64)> = Default::default();
    let mut left: BTreeMap<(i64, i64, i64, i64), usize> = BTreeMap::new();
    for _ in 0..2 * 2600 {
        sim.step();
        let mut now = std::collections::HashMap::new();
        for s in &sim.world().sims {
            let k = (s.family_code, s.floor_anchor, s.home_column, s.base_offset);
            if let RouteState::Carrier { source, .. } = s.route {
                now.insert(k, (source, s.destination_floor));
            }
        }
        for (k, (source, _)) in &was {
            if !now.contains_key(k) {
                let s = sim.world().sims.iter().find(|s| (s.family_code, s.floor_anchor, s.home_column, s.base_offset) == *k).unwrap();
                if s.selected_floor == *source {
                    *left.entry(*k).or_default() += 1;
                }
            }
        }
        was = now;
    }
    let mut v: Vec<_> = left.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    eprintln!("left a queue without riding (family, floor, column, n): {:?}", &v[..v.len().min(15)]);
}

/// Not a test: the car park through a weekday in the mixed tower
#[test]
#[ignore = "a report"]
fn car_park_day() {
    let (mut sim, _) = mixed(true);
    for _ in 0..3 * 2600 {
        sim.step();
    }
    let mut notes: BTreeMap<String, usize> = BTreeMap::new();
    for _ in 0..3 * 2600 {
        sim.step();
        for n in sim.drain_notifications() {
            *notes.entry(n.message).or_default() += 1;
        }
        let t = sim.time().day_tick;
        if t == 300 {
            let mut st: BTreeMap<i64, usize> = BTreeMap::new();
            for o in sim.world().placed_objects.values().filter(|o| o.object_type_code == 7) {
                *st.entry(o.unit_status).or_default() += 1;
            }
            eprintln!("day {} office unit_status {:?}", sim.time().day_counter, st);
        }
        if t % 400 == 0 {
            let b = sim.box_ext();
            eprintln!(
                "tick {t:4} weekend {} vehicles {} in bays {} residents {} commuters held {} parked today {}",
                sim.time().weekend_flag,
                b.vehicles.len(),
                b.vehicles.iter().filter(|v| v.in_bay).count(),
                b.vehicles.iter().filter(|v| v.resident).count(),
                b.commuters.len(),
                b.parked_today
            );
        }
    }
    eprintln!("notes {notes:?}");
    let routes = tower_sim::box_ext::car_elevator::reachable_bays(sim.world(), &sim.box_ext().car_elevators[0]);
    eprintln!("bays the car lift reaches: {}", routes.len());
}

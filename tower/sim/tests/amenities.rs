//! BOX addition: amenities (src/box_ext/amenities.rs, docs/amenities.md).
//! Placement and removal, where each may stand, saving, and a scenario: an
//! office tower with a gym and a laundry, run for some game days.

use serde_json::Value;
use tower_sim::box_ext::amenities::{kind, MAX_RELIEF};
use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{GROUND_Y, UNDERGROUND_Y};

const B1: i64 = UNDERGROUND_Y;

fn submit(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) -> tower_sim::index::CommandResult {
    sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() })
}

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = submit(sim, x, y, tile_type);
    assert!(r.accepted, "place {tile_type}@{x},{y}: {}", r.reason.unwrap_or_default());
}

fn cell(sim: &TowerSim, x: i64, y: i64) -> Option<String> {
    sim.save_state().world.cells.get(&format!("{x},{y}")).cloned()
}

fn saved(sim: &TowerSim) -> Value {
    serde_json::to_value(sim.save_state()).unwrap()["world"]["boxExt"]["amenities"].clone()
}

/// A lobby 100..=170, B1 and three floors above laid with floor, offices
/// on the three floors and a lift: as the car elevator's office tower.
fn office_tower(box_rules: bool) -> TowerSim {
    let mut sim = TowerSim::create_box("amenities", "Amenities", None);
    sim.set_box_rules(box_rules);
    sim.free_build = true;
    for x in 100..=170 {
        place(&mut sim, x, GROUND_Y, "lobby");
        place(&mut sim, x, B1, "floor");
        for floor in 1..=4 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for (floor, first) in [(1, 102), (2, 101), (3, 100)] {
        for i in 0..5 {
            place(&mut sim, first + 12 * i, GROUND_Y - floor, "office");
        }
    }
    for floor in -1..=4 {
        place(&mut sim, 162, GROUND_Y - floor, "elevator");
    }
    sim.free_build = false;
    sim.set_star_count(4);
    sim
}

#[test]
fn classic_rules_know_no_amenity() {
    let mut sim = office_tower(false);
    let r = submit(&mut sim, 100, B1, "gym");
    assert!(!r.accepted);
    assert_eq!(r.reason.as_deref(), Some("Invalid tile type"));
    assert!(saved(&sim).is_null(), "a classic tower saves no amenities");
}

#[test]
fn builds_where_each_may_stand_charges_and_removes() {
    let mut sim = office_tower(true);
    let cash = sim.cash();
    // a gym is a basement room, a laundry is not
    assert!(!submit(&mut sim, 100, GROUND_Y - 4, "gym").accepted);
    assert!(!submit(&mut sim, 100, B1, "laundry").accepted);
    // guest services stand in a lobby, not on a floor above it
    assert!(!submit(&mut sim, 100, GROUND_Y - 4, "guestServices").accepted);
    place(&mut sim, 130, GROUND_Y, "guestServices");
    assert_eq!(cell(&sim, 139, GROUND_Y).as_deref(), Some("guestServices"));
    assert!(sim.submit_command(&SimCommand::RemoveTile { x: 135, y: GROUND_Y }).accepted);
    assert_eq!(cell(&sim, 139, GROUND_Y).as_deref(), Some("lobby"));
    // not over a room, and not where no floor is laid
    assert!(!submit(&mut sim, 101, GROUND_Y - 2, "laundry").accepted);
    assert!(!submit(&mut sim, 100, GROUND_Y - 6, "laundry").accepted);
    place(&mut sim, 100, B1, "gym");
    assert_eq!(sim.cash(), cash - kind("guestServices").unwrap().cost - kind("gym").unwrap().cost);
    assert_eq!(cell(&sim, 100, B1).as_deref(), Some("gym"));
    assert_eq!(cell(&sim, 115, B1).as_deref(), Some("gym"));
    // a convention centre: two floors, clicked on the lower
    let mut five = TowerSim::from_snapshot(&sim.save_state());
    five.free_build = true;
    for x in 100..=170 {
        let _ = submit(&mut five, x, GROUND_Y - 5, "floor");
    }
    place(&mut five, 120, GROUND_Y - 4, "convention");
    assert_eq!(cell(&five, 120, GROUND_Y - 5).as_deref(), Some("convention"));
    assert_eq!(cell(&five, 149, GROUND_Y - 4).as_deref(), Some("convention"));
    // a click anywhere on it removes the whole room, bare floor again
    assert!(five.submit_command(&SimCommand::RemoveTile { x: 140, y: GROUND_Y - 5 }).accepted);
    assert_eq!(cell(&five, 120, GROUND_Y - 4).as_deref(), Some("floor"));
    assert_eq!(cell(&five, 149, GROUND_Y - 5).as_deref(), Some("floor"));
    assert!(sim.submit_command(&SimCommand::RemoveTile { x: 110, y: B1 }).accepted);
    assert_eq!(cell(&sim, 100, B1).as_deref(), Some("floor"));
    // nothing left to save: no rooms, no tallies yet
    assert!(saved(&sim).is_null());
}

fn days(sim: &mut TowerSim, n: i64) {
    for _ in 0..n * 2600 {
        sim.step();
    }
}

#[test]
fn a_gym_and_a_laundry_serve_earn_and_cost_and_survive_saving() {
    let mut with = office_tower(true);
    let mut without = office_tower(true);
    with.free_build = true;
    place(&mut with, 100, B1, "gym");
    place(&mut with, 140, GROUND_Y - 4, "laundry");
    with.free_build = false;
    days(&mut with, 4);
    days(&mut without, 4);
    let a = &with.world().box_ext.amenities;
    // the gym serves the fifteen offices; the laundry has no hotel room
    assert_eq!(a.served.iter().map(|s| s.len()).collect::<Vec<_>>(), vec![15, 0]);
    assert!(a.relief.values().all(|&r| r == 20 && r <= MAX_RELIEF), "{:?}", a.relief);
    assert!(a.visits > 0 && a.income > 0, "visits {} income {}", a.visits, a.income);
    assert_eq!(a.upkeep, kind("gym").unwrap().upkeep + kind("laundry").unwrap().upkeep);
    // a served office scores no worse than the same office unserved
    let score = |sim: &TowerSim| -> Vec<i64> {
        let mut v: Vec<(String, i64)> = sim
            .world()
            .placed_objects
            .iter()
            .filter(|(_, o)| o.object_type_code == 7)
            .map(|(k, o)| (k.to_string(), o.eval_level))
            .collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    };
    for (w, wo) in score(&with).iter().zip(score(&without)) {
        assert!(*w == 0xff || wo == 0xff || *w >= wo, "with {w} without {wo}");
    }
    // saved and loaded: the rooms and tallies, and the same days after
    let snap = with.save_state();
    let mut again = TowerSim::from_snapshot(&snap);
    assert_eq!(saved(&again), saved(&with));
    days(&mut with, 1);
    days(&mut again, 1);
    assert_eq!(saved(&again), saved(&with));
    assert_eq!(again.cash(), with.cash());
}

/// Not a test: a tower with every amenity, for trying them in !Tower
/// (`AMENITY_TOWER=path cargo test --release --test amenities -- --ignored`).
#[test]
#[ignore = "writes a tower for the play tests"]
fn write_a_tower_with_every_amenity() {
    let Ok(out) = std::env::var("AMENITY_TOWER") else { return };
    let mut sim = TowerSim::create_box("amenities", "Amenities", None);
    sim.free_build = true;
    let g = GROUND_Y;
    for x in 100..=199 {
        place(&mut sim, x, g, "lobby");
        place(&mut sim, x, B1, "floor");
        place(&mut sim, x, B1 + 1, "floor");
        for floor in 1..=10 {
            place(&mut sim, x, g - floor, "floor");
        }
    }
    for floor in -2..=10 {
        place(&mut sim, 190, g - floor, "elevator");
        place(&mut sim, 170, g - floor, "elevator");
    }
    for floor in 1..=2 {
        for i in 0..13 {
            place(&mut sim, 100 + i * 6, g - floor, "hotelTwin");
        }
    }
    for floor in 3..=4 {
        for i in 0..9 {
            place(&mut sim, 100 + i * 9, g - floor, "office");
        }
    }
    for i in 0..5 {
        place(&mut sim, 100 + i * 16, g - 5, "condo");
    }
    sim.set_star_count(5);
    place(&mut sim, 100, g, "guestServices");
    place(&mut sim, 102, g - 6, "laundry");
    place(&mut sim, 120, g - 6, "premierLounge");
    place(&mut sim, 102, g - 7, "convention");
    place(&mut sim, 102, B1, "gym");
    place(&mut sim, 125, B1, "pool");
    place(&mut sim, 145, g - 6, "kindergarten");
    place(&mut sim, 102, g - 9, "school");
    sim.free_build = false;
    days(&mut sim, 2);
    // on to the late afternoon, when most of them are busy
    while sim.time().day_tick != 1500 {
        sim.step();
    }
    std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
}

#[test]
fn the_register_counts_households_jobs_and_pupils() {
    use tower_sim::box_ext::people::{register, TRADES};
    let mut sim = TowerSim::create_box("people", "People", None);
    sim.free_build = true;
    let g = GROUND_Y;
    for x in 100..=199 {
        place(&mut sim, x, g, "lobby");
        for floor in 1..=6 {
            place(&mut sim, x, g - floor, "floor");
        }
    }
    for floor in 1..=2 {
        for i in 0..5 {
            place(&mut sim, 100 + i * 16, g - floor, "condo");
        }
    }
    for i in 0..9 {
        place(&mut sim, 100 + i * 9, g - 3, "office");
    }
    for floor in 0..=6 {
        place(&mut sim, 190, g - floor, "elevator");
    }
    sim.set_star_count(4);
    place(&mut sim, 100, g - 4, "kindergarten");
    place(&mut sim, 120, g - 5, "school");
    sim.free_build = false;
    days(&mut sim, 3);
    let r = register(sim.world());
    eprintln!("{r:?}");
    assert_eq!(r.adults + r.children, r.households * 3, "households of three, as upstream counts a condominium");
    assert_eq!(r.jobs[TRADES.iter().position(|t| *t == "Education").unwrap()], 4 + 12, "teachers");
    assert!(r.jobs_total >= r.jobs[0] && r.held_by_residents + r.commuters == r.jobs_total);
    assert!(r.held_by_residents <= r.adults);
    assert_eq!(r.kindergarten_here + r.kindergarten_outside + r.school_here + r.school_outside, r.children);
    assert!(r.kindergarten_here <= 30 && r.school_here <= 150);
    // a classic tower has no register
    let mut classic = TowerSim::from_snapshot(&sim.save_state());
    classic.set_box_rules(false);
    assert_eq!(register(classic.world()).households, 0);
}

/// Not a test: a small tower over a metro station, for the play tests
/// (`METRO_TOWER=path cargo test --release --test amenities -- --ignored`).
#[test]
#[ignore = "writes a tower for the play tests"]
fn write_a_tower_with_a_metro() {
    let Ok(out) = std::env::var("METRO_TOWER") else { return };
    let mut sim = TowerSim::create_box("metro", "Metro", None);
    sim.free_build = true;
    let g = GROUND_Y;
    for x in 100..=150 {
        place(&mut sim, x, g, "lobby");
        for floor in 1..=3 {
            place(&mut sim, x, g - floor, "floor");
        }
    }
    let r = submit(&mut sim, 110, g + 1, "metro");
    assert!(r.accepted, "metro: {:?}", r.reason);
    for x in 140..=150 {
        place(&mut sim, x, g + 1, "floor");
    }
    for floor in 1..=3 {
        for i in 0..4 {
            place(&mut sim, 100 + i * 9, g - floor, "office");
        }
    }
    // a lift from the station's floor up (BOX RULES: some come by train)
    for floor in -1..=3 {
        place(&mut sim, 146, g - floor, "elevator");
    }
    sim.free_build = false;
    sim.set_star_count(4);
    days(&mut sim, 1);
    std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
}

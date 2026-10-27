//! BOX RULES' hotels (box_ext/hotels.rs): rooms are booked by their appeal,
//! so a hotel with amenities fills more nights.

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{GROUND_Y, UNDERGROUND_Y};

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
    assert!(r.accepted, "place {tile_type}@{x},{y}: {:?}", r.reason);
}

/// Twin rooms on four floors over a lobby 100..=199, two lifts; with
/// `amenities`, a laundry, a premier lounge, guest services and a pool, open
fn tower(amenities: bool) -> TowerSim {
    let mut sim = TowerSim::create_box("hotel", "Hotel", None);
    sim.free_build = true;
    sim.set_star_count(4);
    for x in 100..=199 {
        place(&mut sim, x, GROUND_Y, "lobby");
        place(&mut sim, x, UNDERGROUND_Y, "floor");
        for floor in 1..=5 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for floor in 1..=4 {
        for i in 0..11 {
            place(&mut sim, 100 + 6 * i, GROUND_Y - floor, "hotelTwin");
        }
    }
    place(&mut sim, 150, GROUND_Y - 5, "housekeeping");
    if amenities {
        place(&mut sim, 100, GROUND_Y - 5, "laundry");
        place(&mut sim, 120, GROUND_Y - 5, "premierLounge");
        place(&mut sim, 140, GROUND_Y, "guestServices");
        place(&mut sim, 100, UNDERGROUND_Y, "pool");
    }
    for floor in -1..=5 {
        place(&mut sim, 180, GROUND_Y - floor, "elevator");
        place(&mut sim, 190, GROUND_Y - floor, "elevator");
    }
    // housekeeping goes by the service lift
    for floor in 1..=5 {
        place(&mut sim, 172, GROUND_Y - floor, "elevatorService");
    }
    sim.free_build = false;
    let mut snap = sim.save_state();
    for r in snap.world.box_ext.amenities.rooms.iter_mut() {
        r.built_day = 0;
    }
    TowerSim::from_snapshot(&snap)
}

/// Rooms with guests, counted at 22:00 on each of `days` nights after two
/// days' settling in.
fn guest_nights(sim: &mut TowerSim, days: i64) -> usize {
    for _ in 0..2 * 2600 {
        sim.step();
    }
    let mut n = 0;
    for _ in 0..days {
        sim.step();
        while sim.save_state().time.day_tick != 2200 {
            sim.step();
        }
        n += sim.world().placed_objects.values().filter(|o| o.object_type_code == 4 && o.unit_status < 0x18).count();
    }
    n
}

#[test]
fn amenities_fill_the_hotel() {
    let mut classic = tower(false);
    classic.set_box_rules(false);
    let classic = guest_nights(&mut classic, 4);
    let plain = guest_nights(&mut tower(false), 4);
    let with = guest_nights(&mut tower(true), 4);
    eprintln!("guest nights over four nights of 44 rooms: classic {classic}, BOX {plain}, BOX with amenities {with}");
    assert!(plain < classic, "not every room is booked every night");
    assert!(with > plain, "amenities bring guests");
}



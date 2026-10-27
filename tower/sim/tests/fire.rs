//! BOX RULES' fire (box_ext/fire.rs, docs/fire.md): everyone on the fire
//! floor and above goes out, comes back at the all-clear (or the next
//! morning from a floor the fire reached), and sprinklers slow a fire.

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{y_to_floor, GROUND_Y};

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
    assert!(r.accepted, "place {tile_type}@{x},{y}: {:?}", r.reason);
}

const FLOORS: i64 = 5;

/// Offices on eight floors over a lobby 100..=200, a lift at 190; with a
/// sprinkler room on the sixth floor if asked; run to mid-morning.
fn tower(sprinklers: bool) -> TowerSim {
    let mut sim = TowerSim::create_box("fire", "Fire", None);
    sim.free_build = true;
    sim.set_star_count(3);
    for x in 100..=200 {
        place(&mut sim, x, GROUND_Y, "lobby");
        for floor in 1..=FLOORS {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for floor in 1..=FLOORS {
        for i in 0..8 {
            if sprinklers && floor == 4 && i == 0 {
                continue;
            }
            place(&mut sim, 110 + 9 * i, GROUND_Y - floor, "office");
        }
    }
    if sprinklers {
        place(&mut sim, 110, GROUND_Y - 4, "sprinklers");
    }
    for floor in 0..=FLOORS {
        place(&mut sim, 190, GROUND_Y - floor, "elevator");
        place(&mut sim, 100, GROUND_Y - floor, "elevator");
    }
    sim.free_build = false;
    let mut snap = sim.save_state();
    for r in snap.world.box_ext.amenities.rooms.iter_mut() {
        r.built_day = 0; // open now
    }
    let mut sim = TowerSim::from_snapshot(&snap);
    // to mid-morning (a game lights fires before 16:00 only)
    for _ in 0..2600 {
        sim.step();
    }
    while sim.save_state().time.day_tick != 1000 {
        sim.step();
    }
    sim
}

/// The tower with a fire lit `up` floors above the lobby, as the game
/// lights one.
fn light(sim: &TowerSim, up: i64) -> TowerSim {
    let mut snap = sim.save_state();
    let floor = y_to_floor(GROUND_Y) + up;
    let es = &mut snap.world.event_state;
    es.fire_floor = floor;
    es.fire_tile = 200 - 0x20;
    es.game_state_flags |= 8;
    es.fire_start_tick = snap.time.day_tick;
    es.fire_left_pos.fill(0xffff);
    es.fire_right_pos.fill(0xffff);
    es.fire_left_pos[floor as usize] = es.fire_tile;
    es.fire_right_pos[floor as usize] = es.fire_tile;
    es.rescue_countdown = 0;
    es.helicopter_extinguish_pos = 0;
    TowerSim::from_snapshot(&snap)
}

fn burning(sim: &TowerSim) -> bool {
    sim.save_state().world.event_state.game_state_flags & 8 != 0
}

fn offices(sim: &TowerSim) -> usize {
    sim.save_state().world.placed_objects.iter().filter(|(_, o)| o.object_type_code == 7).count()
}

#[test]
fn everyone_goes_out_and_comes_back() {
    let mut sim = light(&tower(false), 4);
    let fire_floor = y_to_floor(GROUND_Y) + 4;
    for _ in 0..40 {
        sim.step();
    }
    let w = sim.save_state().world;
    let out = &w.box_ext.fire.out;
    eprintln!("out after the alarm: {}", out.len());
    assert!(w.box_ext.fire.alarm);
    assert!(out.len() >= 40, "the offices above the fire emptied");
    assert!(out.iter().all(|e| e.floor >= fire_floor - 1), "only the fire floor, the one below and above");
    // nobody out is moving
    let before: Vec<_> = out.iter().map(|e| e.who.clone()).collect();
    let state = |w: &tower_sim::world::WorldState| -> Vec<i64> {
        before.iter().map(|d| w.sims.iter().find(|s| d.is(s)).map(|s| s.state_code).unwrap_or(-1)).collect()
    };
    let s0 = state(&w);
    for _ in 0..30 {
        sim.step();
    }
    assert_eq!(s0, state(&sim.save_state().world), "the people out wait outside");

    // the fire goes out, then the all-clear
    let mut t = 0;
    let mut peak = 0;
    while burning(&sim) {
        sim.step();
        peak = peak.max(sim.save_state().world.box_ext.fire.out.len());
        t += 1;
        assert!(t < 3000, "the fire never went out");
    }
    for _ in 0..tower_sim::box_ext::fire::ALL_CLEAR_TICKS + 2 {
        sim.step();
    }
    let f = sim.save_state().world.box_ext.fire;
    eprintln!("out for {t} ticks; burned floors {:?}, still out {} of {peak}", f.burned, f.out.len());
    assert!(!f.burned.is_empty());
    assert!(f.out.len() < peak, "people from floors the fire missed came back");
    assert!(f.out.iter().all(|e| f.burned.contains(&e.who.floor_anchor)), "only the closed floors' people are out");

    // the next morning everyone is home
    for _ in 0..2600 {
        sim.step();
    }
    assert!(sim.save_state().world.box_ext.fire.is_empty(), "the closed floors open again");
}

#[test]
fn going_out_shows_in_the_stress() {
    let calm = tower(false);
    let mut sim = light(&calm, 4);
    for _ in 0..40 {
        sim.step();
    }
    let w = sim.save_state().world;
    let e = &w.box_ext.fire.out[0];
    let then = calm.save_state().world.sims.iter().find(|s| e.who.is(s)).unwrap().accumulated_ticks;
    let now = w.sims.iter().find(|s| e.who.is(s)).unwrap().accumulated_ticks;
    assert_eq!(now - then, tower_sim::box_ext::fire::EVACUATION_STRESS);
}

#[test]
fn sprinklers_save_rooms() {
    let run = |sprinklers: bool| {
        let calm = tower(sprinklers);
        let before = offices(&calm);
        let mut sim = light(&calm, 4);
        let mut t = 0;
        while burning(&sim) {
            sim.step();
            t += 1;
            assert!(t < 3000);
        }
        (before - offices(&sim), sim.save_state().world.box_ext.fire.burned.len())
    };
    let (lost, floors) = run(false);
    let (lost_s, floors_s) = run(true);
    eprintln!("offices lost: {lost} over {floors} floors; with sprinklers {lost_s} over {floors_s}");
    assert!(lost_s < lost, "fewer offices burned");
    assert_eq!(floors_s, 1, "the fire did not climb");
}

#[test]
fn a_classic_fire_sends_nobody_out() {
    let mut calm = tower(false);
    calm.set_box_rules(false);
    let mut sim = light(&calm, 4);
    for _ in 0..200 {
        sim.step();
    }
    assert!(sim.save_state().world.box_ext.fire.is_empty());
}


#[test]
fn a_shop_on_a_floor_the_fire_reached_closes_for_the_day() {
    use tower_sim::world::{SidecarRecord, VENUE_CLOSED};
    let mut sim = TowerSim::create_box("shop", "Shop", None);
    sim.free_build = true;
    sim.set_star_count(3);
    for x in 100..=200 {
        place(&mut sim, x, GROUND_Y, "lobby");
        for floor in 1..=5 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for floor in [1, 2, 3, 5] {
        for i in 0..8 {
            place(&mut sim, 100 + 9 * i, GROUND_Y - floor, "office");
        }
    }
    place(&mut sim, 100, GROUND_Y - 4, "fastFood");
    for floor in 0..=5 {
        place(&mut sim, 190, GROUND_Y - floor, "elevator");
    }
    sim.free_build = false;
    for _ in 0..2600 {
        sim.step();
    }
    while sim.save_state().time.day_tick != 1000 {
        sim.step();
    }
    let mut sim = light(&sim, 4);
    let floor = y_to_floor(GROUND_Y) + 4;
    let mut t = 0;
    while !sim.save_state().world.box_ext.fire.burned.contains(&floor) {
        sim.step();
        t += 1;
        assert!(t < 500, "the fire never reached its floor");
    }
    let w = sim.save_state().world;
    let link = w.placed_objects.get(&format!("100,{}", GROUND_Y - 4)).map(|o| o.linked_record_index).unwrap();
    let Some(SidecarRecord::CommercialVenue(v)) = w.sidecars.get(link as usize) else { panic!("no venue record") };
    assert_eq!(v.availability_state, VENUE_CLOSED);
}

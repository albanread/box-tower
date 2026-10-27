//! BOX RULES: the tower's residents who work in its offices go to work from
//! home (box_ext/locals.rs, docs/people.md).

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{y_to_floor, RouteState, GROUND_Y};

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
    assert!(r.accepted, "place {tile_type}@{x},{y}: {:?}", r.reason);
}

/// Offices on floors 1 and 2, condominiums on 3 to 5, over a lobby
/// 100..=199, two lifts
fn tower() -> TowerSim {
    let mut sim = TowerSim::create_box("locals", "Locals", None);
    sim.free_build = true;
    for x in 100..=199 {
        place(&mut sim, x, GROUND_Y, "lobby");
        for floor in 1..=5 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for floor in 1..=2 {
        for i in 0..8 {
            place(&mut sim, 100 + 9 * i, GROUND_Y - floor, "office");
        }
    }
    for floor in 3..=5 {
        for i in 0..4 {
            place(&mut sim, 100 + 16 * i, GROUND_Y - floor, "condo");
        }
    }
    for floor in 0..=5 {
        place(&mut sim, 180, GROUND_Y - floor, "elevator");
        place(&mut sim, 190, GROUND_Y - floor, "elevator");
    }
    sim.free_build = false;
    sim.set_star_count(2);
    sim
}

#[test]
fn residents_go_to_work_from_home() {
    let mut sim = tower();
    for _ in 0..3 * 2600 {
        sim.step();
    }
    let locals = sim.box_ext().locals.len();
    let held = tower_sim::box_ext::people::register(sim.world()).held_by_residents;
    let homes: Vec<i64> = (3..=5).map(|f| y_to_floor(GROUND_Y - f)).collect();
    let mut from_home = 0;
    for _ in 0..2600 {
        sim.step();
        from_home = from_home.max(
            sim.world()
                .sims
                .iter()
                .filter(|s| s.family_code == 7 && matches!(s.route, RouteState::Carrier { source, .. } if homes.contains(&source)))
                .count(),
        );
    }
    eprintln!("{held} jobs held by residents; {locals} office workers live here; {from_home} at once down from home");
    assert!(locals > 0);
    assert!(from_home > 0, "office workers took the lift down from home");
}

#[test]
fn not_without_box_rules() {
    let mut sim = tower();
    sim.set_box_rules(false);
    for _ in 0..3 * 2600 {
        sim.step();
    }
    assert!(sim.box_ext().locals.is_empty());
}

/// Not a test: the tower above just before its fourth morning, for the play
/// tests (`LOCALS_TOWER=path cargo test --release --test locals -- --ignored`).
#[test]
#[ignore = "writes a tower for the play tests"]
fn write_a_tower_of_locals() {
    let Ok(out) = std::env::var("LOCALS_TOWER") else { return };
    let mut sim = tower();
    for _ in 0..3 * 2600 {
        sim.step();
    }
    while sim.save_state().time.day_tick != 2560 {
        sim.step();
    }
    std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
}

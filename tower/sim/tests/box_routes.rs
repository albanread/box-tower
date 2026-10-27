//! BOX RULES' routes: a crowded lift loses a short hop to the stairs, and
//! the walk to a lift or stairs is part of a trip's stress (docs/people.md
//! has the rest of the register; these are sims/scoring.rs and
//! route_scoring/select_candidate.rs).

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{RouteState, GROUND_Y};

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
    assert!(r.accepted, "place {tile_type}@{x},{y}: {:?}", r.reason);
}

/// Offices on floors 1 and 2 over a lobby 100..=200; a lift at 190 from the
/// lobby to floor 2; stairs at 100 from the lobby to floor 1 and from 1 to 2
fn tower(box_rules: bool) -> TowerSim {
    let mut sim = TowerSim::create_box("routes", "Routes", None);
    sim.set_box_rules(box_rules);
    sim.free_build = true;
    for x in 100..=200 {
        place(&mut sim, x, GROUND_Y, "lobby");
        for floor in 1..=2 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for floor in 1..=2 {
        for i in 0..8 {
            place(&mut sim, 110 + 9 * i, GROUND_Y - floor, "office");
        }
    }
    for floor in 0..=2 {
        place(&mut sim, 190, GROUND_Y - floor, "elevator");
    }
    place(&mut sim, 100, GROUND_Y, "stairs");
    place(&mut sim, 100, GROUND_Y - 1, "stairs");
    sim.free_build = false;
    sim.set_star_count(2);
    sim
}

/// sims on the stairs, counted a tick at a time, over `days`
fn on_stairs(sim: &mut TowerSim, days: i64) -> i64 {
    let mut n = 0;
    for _ in 0..days * 2600 {
        sim.step();
        n += sim.world().sims.iter().filter(|s| matches!(s.route, RouteState::Segment { .. })).count() as i64;
    }
    n
}

#[test]
fn a_crowded_lift_loses_short_hops_to_the_stairs() {
    let classic = on_stairs(&mut tower(false), 3);
    let boxed = on_stairs(&mut tower(true), 3);
    eprintln!("on the stairs: classic {classic}, BOX {boxed}");
    assert!(boxed > classic, "the stairs took more of the short hops under BOX RULES");
}

#[test]
fn the_walk_to_the_lift_is_part_of_the_trip() {
    // offices far from the lift (110..) and near it (173..)
    let mut sim = tower(true);
    for _ in 0..4 * 2600 {
        sim.step();
    }
    let score = |x: i64| sim.world().placed_objects.get(&format!("{x},{}", GROUND_Y - 2)).map(|o| o.eval_score).unwrap();
    let (far, near) = (score(110), score(173));
    eprintln!("trip stress: far {far}, near {near}");
    assert!(far > near, "the office far from the lift is the more stressed");
    let far = tower_sim::box_ext::rating::of(sim.world(), &format!("110,{}", GROUND_Y - 2)).unwrap();
    let near = tower_sim::box_ext::rating::of(sim.world(), &format!("173,{}", GROUND_Y - 2)).unwrap();
    eprintln!("rating: far {far:?}\nnear {near:?}");
    assert_eq!(far.walk_cells, Some(80));
    assert_eq!(near.walk_cells, Some(17));
    assert!(far.walk_cost > near.walk_cost);
}

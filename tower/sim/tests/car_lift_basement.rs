//! BOX addition: the car elevator joined to the tower's basement by laying
//! basement floor out to it and placing spaces there (docs/car-elevator.md).

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{SidecarRecord, GROUND_Y, UNDERGROUND_Y};

const B1: i64 = UNDERGROUND_Y;
const B2: i64 = UNDERGROUND_Y + 1;

fn submit(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) -> tower_sim::index::CommandResult {
    sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() })
}

fn coverage(sim: &TowerSim, x: i64, y: i64) -> Option<i64> {
    let snap = sim.save_state();
    let obj = snap.world.placed_objects.get(&format!("{x},{y}"))?;
    match snap.world.sidecars.get(obj.linked_record_index as usize)? {
        SidecarRecord::ServiceRequest(r) => r.coverage_flag,
        _ => None,
    }
}

/// A lobby 100..=170 with B1 and B2 under it, and a car elevator to B2
/// out under the street at 180 (`box_rules` on or off).
fn tower(box_rules: bool) -> TowerSim {
    let mut sim = TowerSim::create_box("lift", "Lift", None);
    sim.set_box_rules(box_rules);
    sim.free_build = true;
    for x in 100..=170 {
        assert!(submit(&mut sim, x, GROUND_Y, "lobby").accepted);
        assert!(submit(&mut sim, x, B1, "floor").accepted);
        assert!(submit(&mut sim, x, B2, "floor").accepted);
    }
    if box_rules {
        assert!(submit(&mut sim, 180, B2, "carElevator").accepted);
    }
    sim
}

#[test]
fn the_basement_is_dug_out_to_the_car_elevator_and_its_spaces_are_reached() {
    let mut sim = tower(true);
    // floor laid outwards from the building, cell by cell, to the shaft
    for x in 171..180 {
        for y in [B1, B2] {
            let r = submit(&mut sim, x, y, "floor");
            assert!(r.accepted, "floor {x},{y}: {:?}", r.reason);
        }
    }
    // and beyond it, from the shaft's far side
    for x in 188..=195 {
        assert!(submit(&mut sim, x, B2, "floor").accepted);
    }
    // spaces in the new basement and in the old, bare floor between
    for x in [140, 160, 172, 176, 190] {
        let r = submit(&mut sim, x, B2, "parking");
        assert!(r.accepted, "space {x}: {:?}", r.reason);
    }
    for x in [140, 160, 172, 176, 190] {
        assert_eq!(coverage(&sim, x, B2), Some(1), "space {x} is reached from the car elevator");
    }
    assert_eq!(sim.query_cell(180, B2).car_elevator_info.unwrap().bays_reached, 5, "the car elevator's own bays");
    // a room on the row still stops the cars
    assert!(submit(&mut sim, 144, B2, "security").accepted);
    assert_eq!(coverage(&sim, 160, B2), Some(1));
    assert_eq!(coverage(&sim, 140, B2), Some(0), "the security office is in the way");
}

#[test]
fn classic_rules_dig_nothing_sideways_and_floor_stops_coverage() {
    let mut sim = tower(false);
    let r = submit(&mut sim, 171, B1, "floor");
    assert_eq!(r.reason.as_deref(), Some("No support"));
    // a ramp with bare floor between it and a space: upstream's coverage
    assert!(submit(&mut sim, 150, B1, "parkingRamp").accepted);
    assert!(submit(&mut sim, 120, B1, "parking").accepted);
    assert_eq!(coverage(&sim, 120, B1), Some(0));
}

fn day_ticks(sim: &mut TowerSim, ticks: i64) {
    for _ in 0..ticks {
        sim.step();
    }
}

/// Not a test: an office tower whose basement is dug out under the street
/// to its car elevator, spaces along it, for trying in !Tower
/// (`CAR_LIFT_TOWER=path cargo test --release --test car_lift_basement -- --ignored`).
#[test]
#[ignore = "writes a tower for the play tests"]
fn write_a_tower_with_a_dug_out_basement() {
    let Ok(out) = std::env::var("CAR_LIFT_TOWER") else { return };
    let mut sim = tower(true);
    for x in 171..180 {
        for y in [B1, B2] {
            assert!(submit(&mut sim, x, y, "floor").accepted);
        }
    }
    for x in 188..=207 {
        assert!(submit(&mut sim, x, B2, "floor").accepted);
    }
    for x in [104, 112, 120, 128, 136, 144, 152, 160, 172, 176, 188, 192, 196, 200] {
        assert!(submit(&mut sim, x, B2, "parking").accepted, "space {x}");
    }
    for floor in 1..=3 {
        for x in 100..=170 {
            assert!(submit(&mut sim, x, GROUND_Y - floor, "floor").accepted);
        }
    }
    for (floor, first) in [(1, 102), (2, 101), (3, 100)] {
        for i in 0..5 {
            assert!(submit(&mut sim, first + 12 * i, GROUND_Y - floor, "office").accepted);
        }
    }
    for floor in -2..=3 {
        assert!(submit(&mut sim, 162, GROUND_Y - floor, "elevator").accepted);
    }
    sim.free_build = false;
    sim.set_star_count(3);
    day_ticks(&mut sim, 2 * 2600);
    // to the morning, as the cars come in
    while sim.time().day_tick != 300 {
        sim.step();
    }
    std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
}

const B3: i64 = UNDERGROUND_Y + 2;

/// Offices over a lobby 100..=170, B1..B3 under it, a ramp stack at 140
/// from B1 to B3, spaces on B3; with `lift`, a car elevator to B1 only out
/// at 180, B1 dug out to it.
fn ramp_tower(lift: bool) -> TowerSim {
    let mut sim = TowerSim::create_box("ramps", "Ramps", None);
    sim.free_build = true;
    for x in 100..=170 {
        assert!(submit(&mut sim, x, GROUND_Y, "lobby").accepted);
        for y in [B1, B2, B3] {
            assert!(submit(&mut sim, x, y, "floor").accepted);
        }
        for floor in 1..=3 {
            assert!(submit(&mut sim, x, GROUND_Y - floor, "floor").accepted);
        }
    }
    for y in [B1, B2, B3] {
        let r = submit(&mut sim, 140, y, "parkingRamp");
        assert!(r.accepted, "ramp {y}: {:?}", r.reason);
    }
    for x in [104, 108, 112, 116, 120] {
        assert!(submit(&mut sim, x, B3, "parking").accepted);
    }
    if lift {
        assert!(submit(&mut sim, 180, B1, "carElevator").accepted);
        for x in 171..180 {
            assert!(submit(&mut sim, x, B1, "floor").accepted);
        }
    }
    for (floor, first) in [(1, 102), (2, 101), (3, 100)] {
        for i in 0..5 {
            assert!(submit(&mut sim, first + 12 * i, GROUND_Y - floor, "office").accepted);
        }
    }
    for floor in 0..=3 {
        assert!(submit(&mut sim, 162, GROUND_Y - floor, "elevator").accepted);
    }
    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

#[derive(Default, Debug)]
struct Watched {
    on_a_ramp: bool,
    parked_b3: usize,
    most_in_a_bay: usize,
    via_lift: bool,
    from_street: bool,
    left_by_ramps: bool,
}

fn watch(sim: &mut TowerSim, days: i64) -> Watched {
    use tower_sim::box_ext::car_elevator::{VehiclePhase, RAMP};
    let mut w = Watched::default();
    for _ in 0..days * 2600 {
        sim.step();
        let st = sim.box_ext();
        for v in &st.vehicles {
            let below = v.floor < 10 * 16;
            if below && v.floor % 16 != 0 && v.phase != VehiclePhase::Riding {
                w.on_a_ramp = true;
            }
            if v.in_bay && v.bay_floor == 7 {
                w.parked_b3 += 1;
                w.via_lift |= v.elevator != RAMP && v.lift_floor.is_some();
                w.from_street |= v.elevator == RAMP;
            }
            if v.elevator == RAMP && v.phase == VehiclePhase::Leaving {
                w.left_by_ramps = true;
            }
            w.most_in_a_bay = w.most_in_a_bay.max(st.vehicles.iter().filter(|o| o.bay == v.bay).count());
        }
    }
    w
}

#[test]
fn cars_take_the_lift_then_the_ramps_down_to_their_spaces() {
    let mut sim = ramp_tower(true);
    let w = watch(&mut sim, 4);
    eprintln!("{w:?}");
    assert!(w.on_a_ramp, "no car drove on a ramp");
    assert!(w.parked_b3 > 0 && w.via_lift, "no car came down to B3 by the car elevator and the ramps");
    assert!(w.most_in_a_bay <= 5);
    assert!(sim.box_ext().parking_income > 0, "no parking fees");
}

#[test]
fn a_car_park_of_ramps_alone_is_driven_from_the_street() {
    let mut sim = ramp_tower(false);
    let w = watch(&mut sim, 4);
    eprintln!("{w:?}");
    assert!(w.on_a_ramp && w.from_street && w.parked_b3 > 0, "no car came down the ramps from the street");
    assert!(w.left_by_ramps, "no car drove back up and away");
    assert!(w.most_in_a_bay > 1, "a space holds more than one car");
}

/// Not a test: the ramp tower with its car elevator, spaces on B1..B3, in
/// the morning as the cars come (`CAR_PARK_TOWER=path cargo test --release
/// --test car_lift_basement -- --ignored`).
#[test]
#[ignore = "writes a tower for the play tests"]
fn write_a_car_park_tower() {
    let Ok(out) = std::env::var("CAR_PARK_TOWER") else { return };
    let mut sim = ramp_tower(true);
    sim.free_build = true;
    for x in [104, 108, 112, 116, 120] {
        assert!(submit(&mut sim, x, B1, "parking").accepted);
        assert!(submit(&mut sim, x, B2, "parking").accepted);
    }
    for x in [172, 176] {
        assert!(submit(&mut sim, x, B1, "parking").accepted);
    }
    sim.free_build = false;
    day_ticks(&mut sim, 2 * 2600);
    while sim.box_ext().vehicles.iter().filter(|v| v.in_bay).count() < 6
        || !sim.box_ext().vehicles.iter().any(|v| !v.in_bay)
    {
        sim.step();
    }
    std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
}

#[test]
fn commuters_who_drive_set_off_from_the_car_park() {
    use tower_sim::world::RouteState;
    // the ramp tower, its passenger lift also down to B1 where the car
    // elevator lands
    let mut sim = ramp_tower(true);
    sim.free_build = true;
    assert!(submit(&mut sim, 162, B1, "elevator").accepted);
    for x in [104, 108, 112, 116, 120] {
        assert!(submit(&mut sim, x, B1, "parking").accepted);
    }
    sim.free_build = false;
    let (mut held, mut from_b1) = (false, false);
    for _ in 0..3 * 2600 {
        sim.step();
        held |= !sim.box_ext().commuters.is_empty();
        from_b1 |= sim.world().sims.iter().any(|s| {
            s.family_code == 7 && matches!(s.route, RouteState::Carrier { .. }) && s.selected_floor == 9
        });
        if held && from_b1 {
            break;
        }
    }
    assert!(held, "no commuter waited for their car to park");
    assert!(from_b1, "no commuter took the lift up from the car park (B1)");
}

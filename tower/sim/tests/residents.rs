//! BOX RULES: residents' cars (box_ext/car_elevator.rs, docs/car-elevator.md):
//! half the households park overnight, coming home in the evening and going
//! out in the morning, and pay for it.

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{GROUND_Y, UNDERGROUND_Y};

const B1: i64 = UNDERGROUND_Y;

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
    assert!(r.accepted, "place {tile_type}@{x},{y}: {:?}", r.reason);
}

/// Condominiums on two floors over a lobby 100..=170, a lift; B1 under it
/// dug out to a car elevator at 180, five spaces on B1
fn tower() -> TowerSim {
    let mut sim = TowerSim::create_box("homes", "Homes", None);
    sim.free_build = true;
    for x in 100..=170 {
        place(&mut sim, x, GROUND_Y, "lobby");
        place(&mut sim, x, B1, "floor");
        for floor in 1..=2 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    place(&mut sim, 180, B1, "carElevator");
    for x in 171..180 {
        place(&mut sim, x, B1, "floor");
    }
    for x in [104, 108, 112, 116, 120] {
        place(&mut sim, x, B1, "parking");
    }
    for floor in 1..=2 {
        for i in 0..4 {
            place(&mut sim, 100 + 16 * i, GROUND_Y - floor, "condo");
        }
    }
    for floor in -1..=2 {
        place(&mut sim, 166, GROUND_Y - floor, "elevator");
    }
    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

fn residents(sim: &TowerSim) -> usize {
    sim.box_ext().vehicles.iter().filter(|v| v.resident).count()
}

fn to_tick(sim: &mut TowerSim, t: i64) {
    sim.step();
    while sim.save_state().time.day_tick != t {
        sim.step();
    }
}

#[test]
fn residents_park_overnight_and_go_out_in_the_morning() {
    let mut sim = tower();
    for _ in 0..3 * 2600 {
        sim.step();
    }
    let homes = sim.world().placed_objects.values().filter(|o| o.object_type_code == 9 && o.unit_status <= 0x17).count();
    to_tick(&mut sim, 2000);
    let night = residents(&sim);
    let parked = sim.box_ext().vehicles.iter().filter(|v| v.resident && v.in_bay).count();
    to_tick(&mut sim, 400);
    // (on their way out by now: a car lift takes one at a time)
    let morning = sim.box_ext().vehicles.iter().filter(|v| v.resident && v.going_since == 0).count();
    eprintln!("{homes} households; residents' cars at 20:00: {night} ({parked} in their spaces); at 10:00: {morning}");
    assert!(homes > 0);
    assert!(night > 0, "cars came home");
    assert!(parked > 0, "and parked");
    assert_eq!(morning, 0, "and set off in the morning");
}

#[test]
fn residents_pay_to_park() {
    let mut sim = tower();
    for _ in 0..3 * 2600 {
        sim.step();
    }
    let before = sim.box_ext().parking_income;
    to_tick(&mut sim, 2000);
    let came = residents(&sim) as i64;
    for _ in 0..2600 {
        sim.step();
    }
    let paid = sim.box_ext().parking_income - before;
    assert!(paid >= came * tower_sim::box_ext::car_elevator::PARKING_FEE, "paid {paid} for {came}");
}

/// Not a test: the tower above at 16:30 on its fourth day, for the play
/// tests (`HOMES_TOWER=path cargo test --release --test residents -- --ignored`).
#[test]
#[ignore = "writes a tower for the play tests"]
fn write_a_tower_of_homes() {
    let Ok(out) = std::env::var("HOMES_TOWER") else { return };
    let mut sim = tower();
    for _ in 0..3 * 2600 {
        sim.step();
    }
    to_tick(&mut sim, 1560);
    std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
}

#[test]
fn parking_spaces_cost_what_their_fees_can_bear() {
    use tower_sim::box_ext::car_elevator::{CAR_ELEVATOR_UPKEEP, SPACE_UPKEEP};
    let ledgers = |box_rules: bool| {
        let mut sim = tower();
        sim.set_box_rules(box_rules);
        // to the expense sweep at the end of a three-day period
        while sim.ledger().expense_ledger[24] == 0 {
            sim.step();
        }
        (sim.ledger().expense_ledger[24], sim.ledger().expense_ledger[1])
    };
    let (box_parking, box_lifts) = ledgers(true);
    let (classic_parking, classic_lifts) = ledgers(false);
    eprintln!("parking family: BOX {box_parking}, classic {classic_parking}; lifts: BOX {box_lifts}, classic {classic_lifts}");
    // five spaces at SPACE_UPKEEP instead of the lobby's rate a cell
    let classic_space = (classic_parking - box_parking + 5 * SPACE_UPKEEP) / 5;
    assert!(classic_space > 100 * SPACE_UPKEEP, "upstream's rate for a space: {classic_space}");
    assert_eq!(box_lifts - classic_lifts, CAR_ELEVATOR_UPKEEP);
}

#[test]
fn residents_cars_stay_home_at_the_weekend() {
    let mut sim = tower();
    for _ in 0..2 * 2600 {
        sim.step();
    }
    // to a weekend morning, the cars home from the evening before
    while sim.save_state().time.weekend_flag == 0 || sim.save_state().time.day_tick != 400 {
        sim.step();
    }
    let home = sim.box_ext().vehicles.iter().filter(|v| v.resident && v.in_bay && v.going_since == 0).count();
    eprintln!("weekend morning: {home} residents' cars at home");
    assert!(home > 0, "they did not go out on a weekend");
}

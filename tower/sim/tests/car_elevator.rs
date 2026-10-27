//! BOX addition: the car elevator (src/box_ext/car_elevator.rs,
//! docs/car-elevator.md).  Placement, removal and coverage, and a scenario:
//! a small office tower with a car elevator to B2 and parking spaces there,
//! run for some game days.

use serde_json::Value;
use tower_sim::box_ext::car_elevator::{
    platform_views, vehicle_views, VehiclePhase, BAY_CARS, CAR_ELEVATOR_COST, CAR_ELEVATOR_FLOOR_COST, CAR_ELEVATOR_WIDTH,
};
use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{SidecarRecord, GROUND_Y, UNDERGROUND_Y};

const B1: i64 = UNDERGROUND_Y;
const B2: i64 = UNDERGROUND_Y + 1;
const B3: i64 = UNDERGROUND_Y + 2;
/// The car elevator's column in these towers.
const CE: i64 = 171;

fn submit(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) -> tower_sim::index::CommandResult {
    sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() })
}

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = submit(sim, x, y, tile_type);
    assert!(r.accepted, "place {tile_type}@{x},{y}: {}", r.reason.unwrap_or_default());
}

fn remove(sim: &mut TowerSim, x: i64, y: i64) -> tower_sim::index::CommandResult {
    sim.submit_command(&SimCommand::RemoveTile { x, y })
}

/// Ground lobby 100..=170 (the street beyond it free for the shaft), B1
/// floor under it, and parking spaces on B2 from `CE - 4` leftwards.
fn basement_tower(spaces: i64) -> TowerSim {
    let mut sim = TowerSim::create_box("car-elevator", "Car Elevator", None);
    sim.free_build = true;
    for x in 100..=170 {
        place(&mut sim, x, GROUND_Y, "lobby");
        place(&mut sim, x, B1, "floor");
    }
    for i in 0..spaces {
        place(&mut sim, CE - 4 - 4 * i, B2, "parking");
    }
    sim
}

fn snapshot_json(sim: &TowerSim) -> Value {
    serde_json::to_value(sim.save_state()).unwrap()
}

/// coverageFlag of the parking space anchored at (x, y).
fn coverage(sim: &TowerSim, x: i64, y: i64) -> Option<i64> {
    let snap = sim.save_state();
    let obj = snap.world.placed_objects.get(&format!("{x},{y}"))?;
    match snap.world.sidecars.get(obj.linked_record_index as usize)? {
        SidecarRecord::ServiceRequest(r) => r.coverage_flag,
        _ => None,
    }
}

fn cell(sim: &TowerSim, x: i64, y: i64) -> Option<String> {
    sim.save_state().world.cells.get(&format!("{x},{y}")).cloned()
}

// ─── Placement, removal, coverage ────────────────────────────────────────────

/// The saved car-elevator state (an empty list when none is built).
fn saved_elevators(sim: &TowerSim) -> Value {
    snapshot_json(sim)["world"]["boxExt"]["carElevators"].clone()
}

#[test]
fn a_classic_tower_has_no_car_elevator_and_saves_no_box_state() {
    let mut sim = TowerSim::create("classic", "Classic", None);
    sim.free_build = true;
    for x in 100..=170 {
        place(&mut sim, x, GROUND_Y, "lobby");
    }
    let r = submit(&mut sim, CE, B1, "carElevator");
    assert_eq!(r.reason.as_deref(), Some("Invalid tile type"), "a BOX RULES feature");
    assert!(snapshot_json(&sim)["world"].get("boxExt").is_none());
    let sim = basement_tower(2);
    assert_eq!(saved_elevators(&sim), serde_json::json!([]));
}

#[test]
fn places_a_span_from_the_street_to_the_chosen_basement_and_charges_for_it() {
    let mut sim = basement_tower(0);
    sim.free_build = false;
    let cash = sim.cash();
    let r = submit(&mut sim, CE, B2, "carElevator");
    assert!(r.accepted, "{:?}", r.reason);
    assert_eq!(r.economy_changed, Some(true));
    assert_eq!(sim.cash(), cash - CAR_ELEVATOR_COST - CAR_ELEVATOR_FLOOR_COST);
    for y in [GROUND_Y, B1, B2] {
        for dx in 0..CAR_ELEVATOR_WIDTH {
            assert_eq!(cell(&sim, CE + dx, y).as_deref(), Some("carElevator"), "{},{y}", CE + dx);
        }
    }
    assert_eq!(cell(&sim, CE, B3), None);
    let patch = r.patch.unwrap();
    assert_eq!(patch.iter().filter(|p| p.tile_type == "carElevator").count() as i64, 3 * CAR_ELEVATOR_WIDTH);

    let q = sim.query_cell(CE + 3, B1);
    assert_eq!(q.tile_type, "carElevator");
    let info = q.car_elevator_info.expect("car elevator info");
    assert_eq!(info.column, CE);
    assert_eq!(info.served_floors, vec![10, 9, 8]);
    assert_eq!(info.bottom_floor, 8);
    assert!(sim.query_cell(CE - 1, B1).car_elevator_info.is_none());

    // Extending down a floor costs one floor.
    let cash = sim.cash();
    assert!(submit(&mut sim, CE, B3, "carElevator").accepted);
    assert_eq!(sim.cash(), cash - CAR_ELEVATOR_FLOOR_COST);
    assert_eq!(cell(&sim, CE + 7, B3).as_deref(), Some("carElevator"));
    assert!(!submit(&mut sim, CE, B2, "carElevator").accepted, "already reaches B2");
}

#[test]
fn refuses_bad_placements() {
    let mut sim = basement_tower(0);
    // Only to a basement floor.
    assert!(!submit(&mut sim, CE, GROUND_Y, "carElevator").accepted);
    assert!(!submit(&mut sim, CE, GROUND_Y - 3, "carElevator").accepted);
    // Not through the lobby.
    assert!(!submit(&mut sim, 160, B1, "carElevator").accepted);
    // Not off the map.
    assert!(!submit(&mut sim, 370, B1, "carElevator").accepted);
    // Not without the money.
    sim.free_build = false;
    let snap = {
        let mut s = sim.save_state();
        s.ledger.cash_balance = CAR_ELEVATOR_COST - 1;
        s
    };
    let mut poor = TowerSim::from_snapshot(&snap);
    let r = submit(&mut poor, CE, B1, "carElevator");
    assert_eq!(r.reason.as_deref(), Some("Insufficient funds"));
    // Nothing was built by any of them.
    assert_eq!(saved_elevators(&sim), serde_json::json!([]));

    // Not over another, and nothing laid over it.
    sim.free_build = true;
    place(&mut sim, CE, B2, "carElevator");
    assert!(!submit(&mut sim, CE + 4, B1, "carElevator").accepted);
    assert!(!submit(&mut sim, CE + 2, GROUND_Y, "stairs").accepted);
    assert!(!submit(&mut sim, CE - 2, GROUND_Y, "elevator").accepted);
    assert!(!submit(&mut sim, CE + 2, B1, "floor").accepted);
}

#[test]
fn its_landings_cover_parking_as_a_ramp_does_and_removal_undoes_it() {
    let mut sim = basement_tower(3);
    // A fourth space further along, bare floor between: under BOX RULES the
    // floor is a driveway, so it is reached as well (classic coverage would
    // stop at the floor).
    let far = CE - 4 - 4 * 3 - 4 - 4;
    place(&mut sim, far, B2, "parking");
    for i in 0..3 {
        assert_eq!(coverage(&sim, CE - 4 - 4 * i, B2), Some(0));
    }
    let log_before = sim.save_state().world.parking_demand_log.len();
    assert_eq!(log_before, 4);

    let r = submit(&mut sim, CE, B2, "carElevator");
    assert!(r.accepted);
    for i in 0..3 {
        assert_eq!(coverage(&sim, CE - 4 - 4 * i, B2), Some(1), "space {i}");
    }
    assert_eq!(coverage(&sim, far, B2), Some(1));
    assert_eq!(sim.save_state().world.parking_demand_log.len(), 0);
    // The command's patch carries the spaces' new coverage, as a ramp's does.
    assert!(r.patch.unwrap().iter().any(|p| p.tile_type == "parking" && p.coverage_flag == Some(1)));
    let info = sim.query_cell(CE, B2).car_elevator_info.unwrap();
    assert_eq!(info.bays_reached, 4);

    // Click anywhere on it: all of it goes.
    let r = remove(&mut sim, CE + 5, B1);
    assert!(r.accepted);
    for i in 0..3 {
        assert_eq!(coverage(&sim, CE - 4 - 4 * i, B2), Some(0));
    }
    assert_eq!(sim.save_state().world.parking_demand_log.len(), 4);
    for y in [GROUND_Y, B1, B2] {
        assert_ne!(cell(&sim, CE + 3, y).as_deref(), Some("carElevator"));
    }
    assert_eq!(saved_elevators(&sim), serde_json::json!([]));
    assert_eq!(sim.box_ext().next_vehicle_id, 0);
}

#[test]
fn a_landing_covers_only_its_own_floors() {
    let mut sim = basement_tower(1);
    // A space on B3 under the B2 space (support from the B2 space above).
    place(&mut sim, CE - 4, B3, "parking");
    place(&mut sim, CE, B2, "carElevator");
    assert_eq!(coverage(&sim, CE - 4, B2), Some(1));
    assert_eq!(coverage(&sim, CE - 4, B3), Some(0), "B3 is below the platform's reach");
    place(&mut sim, CE, B3, "carElevator");
    assert_eq!(coverage(&sim, CE - 4, B3), Some(1));
}

// ─── The scenario ────────────────────────────────────────────────────────────

/// Offices on floors 1..3 whose workers include drivers (office workers
/// ask for parking when (floor + column) % 4 == 1), a passenger elevator,
/// a car elevator to B2 and eight spaces there; three stars.
fn office_tower() -> TowerSim {
    let mut sim = basement_tower(8);
    for floor in 1..=3 {
        for x in 100..=170 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    // Floor anchors 11, 12, 13: columns = 2, 1, 0 (mod 4).
    for (floor, first) in [(1, 102), (2, 101), (3, 100)] {
        for i in 0..5 {
            place(&mut sim, first + 12 * i, GROUND_Y - floor, "office");
        }
    }
    for floor in 0..=3 {
        place(&mut sim, 162, GROUND_Y - floor, "elevator");
    }
    place(&mut sim, CE, B2, "carElevator");
    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

#[derive(Default, Debug)]
struct Seen {
    vehicles: usize,
    max_queue: usize,
    rode_down: bool,
    rode_up: bool,
    parked: bool,
    left: usize,
    phases: Vec<VehiclePhase>,
}

fn watch(sim: &mut TowerSim, ticks: i64, seen: &mut Seen) {
    let mut ids: Vec<i64> = sim.box_ext().vehicles.iter().map(|v| v.id).collect();
    for _ in 0..ticks {
        sim.step();
        let st = sim.box_ext();
        for v in &st.vehicles {
            if !seen.phases.contains(&v.phase) {
                seen.phases.push(v.phase);
            }
            if v.phase == VehiclePhase::Riding && !v.outbound && v.floor < 10 * 16 {
                seen.rode_down = true;
            }
            if v.phase == VehiclePhase::Riding && v.outbound && v.floor > 8 * 16 {
                seen.rode_up = true;
            }
            if v.phase == VehiclePhase::Parked {
                seen.parked = true;
            }
        }
        for e in &st.car_elevators {
            seen.max_queue = seen.max_queue.max(e.queue.len());
            assert!(e.platform.floor >= e.bottom_floor * 16 && e.platform.floor <= 10 * 16);
        }
        let now: Vec<i64> = st.vehicles.iter().map(|v| v.id).collect();
        seen.left += ids.iter().filter(|id| !now.contains(id)).count();
        seen.vehicles += now.iter().filter(|id| !ids.contains(id)).count();
        ids = now;
        // At most one vehicle aboard each platform, and a bay holds at most
        // BAY_CARS cars (a space drawn stands for several).
        for v in &st.vehicles {
            assert!(st.vehicles.iter().filter(|w| w.bay == v.bay).count() <= BAY_CARS);
        }
        for e in &st.car_elevators {
            let aboard = st.vehicles.iter().filter(|v| v.elevator == e.column && v.phase == VehiclePhase::Riding).count();
            assert!(aboard <= 1);
        }
    }
}

const DAY: i64 = 2600;

#[test]
fn cars_arrive_ride_down_park_and_leave() {
    let mut sim = office_tower();
    let mut seen = Seen::default();
    watch(&mut sim, 6 * DAY, &mut seen);
    eprintln!("{seen:?}");
    assert!(seen.vehicles > 0, "no car came");
    assert!(seen.rode_down, "no car rode the platform down");
    assert!(seen.parked, "no car parked");
    assert!(seen.rode_up, "no car rode the platform up");
    assert!(seen.left > 0, "no car left");
    // The presenter's views are there.
    let views = vehicle_views(sim.box_ext());
    assert_eq!(views.len(), sim.box_ext().vehicles.len());
    assert_eq!(platform_views(sim.box_ext()).len(), 1);
}

#[test]
fn the_traffic_is_deterministic_and_survives_save_and_load() {
    let mut a = office_tower();
    let mut b = office_tower();
    // Run to the middle of a morning's arrivals.
    let mut seen = Seen::default();
    watch(&mut a, 2 * DAY, &mut seen);
    for _ in 0..2 * DAY {
        b.step();
    }
    assert_eq!(snapshot_json(&a), snapshot_json(&b), "same run twice");
    // Find a tick with a car on the move, then save there.
    let mut guard = 0;
    while !a.box_ext().vehicles.iter().any(|v| v.phase == VehiclePhase::Riding || v.phase == VehiclePhase::Approaching) {
        a.step();
        b.step();
        guard += 1;
        assert!(guard < 3 * DAY, "no car on the move");
    }
    let saved = a.save_state();
    assert!(serde_json::to_value(&saved).unwrap()["world"].get("boxExt").is_some());
    let mut c = TowerSim::from_snapshot(&saved);
    assert_eq!(serde_json::to_value(c.save_state()).unwrap(), serde_json::to_value(&saved).unwrap());
    for _ in 0..DAY {
        a.step();
        c.step();
    }
    assert_eq!(snapshot_json(&a), snapshot_json(&c), "loaded mid-trip, continued the same");
}

#[test]
fn a_demolished_bay_or_car_elevator_takes_its_cars_with_it() {
    let mut sim = office_tower();
    let mut guard = 0;
    while !sim.box_ext().vehicles.iter().any(|v| v.phase == VehiclePhase::Parked) {
        sim.step();
        guard += 1;
        assert!(guard < 3 * DAY, "no car parked");
    }
    let parked = sim.box_ext().vehicles.iter().find(|v| v.phase == VehiclePhase::Parked).unwrap().clone();
    let (bx, by) = {
        let mut it = parked.bay.split(',').map(|p| p.parse::<i64>().unwrap());
        (it.next().unwrap(), it.next().unwrap())
    };
    sim.free_build = true;
    assert!(remove(&mut sim, bx, by).accepted);
    sim.step();
    assert!(!sim.box_ext().vehicles.iter().any(|v| v.id == parked.id));
    // The rest keep moving, and the platform is not left waiting for it.
    let mut seen = Seen::default();
    watch(&mut sim, DAY, &mut seen);

    // Removing the car elevator removes every car using it, and the state.
    assert!(remove(&mut sim, CE, GROUND_Y).accepted);
    assert!(sim.box_ext().vehicles.is_empty());
    assert_eq!(saved_elevators(&sim), serde_json::json!([]));
    for _ in 0..DAY {
        sim.step();
    }
    assert!(sim.box_ext().vehicles.is_empty(), "no cars without a car elevator");
}

// ─── BOX RULES: parking that makes sense, and busyness ───────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Access {
    Nothing,
    Ramp,
    CarElevator,
}

/// Offices whose workers drive.  `Nothing`: eight spaces on B2 that nothing
/// reaches; `CarElevator`: the same with a car elevator to B2; `Ramp`: a
/// ramp on B1 at 150 with eight spaces beside it.
fn office_tower_with(box_rules: bool, access: Access) -> TowerSim {
    let mut sim = basement_tower(if access == Access::Ramp { 0 } else { 8 });
    sim.free_build = true;
    if access == Access::Ramp {
        place(&mut sim, 150, B1, "parkingRamp");
        for i in 1..=8 {
            place(&mut sim, 150 - 4 * i, B1, "parking");
        }
    }
    for floor in 1..=3 {
        for x in 100..=170 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for (floor, first) in [(1, 102), (2, 101), (3, 100)] {
        for i in 0..5 {
            place(&mut sim, first + 12 * i, GROUND_Y - floor, "office");
        }
    }
    for floor in 0..=3 {
        place(&mut sim, 162, GROUND_Y - floor, "elevator");
    }
    if access == Access::CarElevator {
        place(&mut sim, CE, B2, "carElevator");
    }
    sim.set_box_rules(box_rules);
    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

/// "... demand Parking" messages over `ticks`, and the cars that came.
fn complaints(sim: &mut TowerSim, ticks: i64) -> (usize, i64) {
    let mut n = 0;
    for _ in 0..ticks {
        let r = sim.step();
        n += r.notifications.iter().filter(|m| m.message.contains("demand Parking")).count();
    }
    (n, sim.box_ext().next_vehicle_id)
}

#[test]
fn under_box_rules_reachable_bays_serve_the_drivers() {
    // Classic: the ramp covers every space, which upstream takes to mean
    // no demand: the drivers complain.
    let (classic, _) = complaints(&mut office_tower_with(false, Access::Ramp), DAY);
    assert!(classic > 0, "upstream's inverted parking");
    // BOX RULES: the reachable spaces serve them, by ramp or car elevator,
    // as many as the spaces hold (all ninety of this tower's office workers
    // drive, every working day; eight spaces hold forty cars).  With the
    // spaces unreachable every driver complains.
    let (unreached, none) = complaints(&mut office_tower_with(true, Access::Nothing), DAY);
    assert!(unreached > 0);
    assert_eq!(none, 0);
    for access in [Access::Ramp, Access::CarElevator] {
        let (boxed, cars) = complaints(&mut office_tower_with(true, access), DAY);
        eprintln!("{} complaints, {cars} cars; unreachable: {unreached}", boxed);
        assert!(cars >= 20, "the spaces filled: {cars}");
        assert!(boxed < unreached, "fewer complained: {boxed} of {unreached}");
    }
}

/// A rush-hour tower: lobby 60..=230 with a gap for each car elevator,
/// parking spaces on B1..B3 either side, offices on floors 1..4 whose
/// workers all drive, two passenger lifts.
fn rush_tower(second: bool, office_floors: i64) -> TowerSim {
    const CE2: i64 = CE + CAR_ELEVATOR_WIDTH + 16; // four spaces between
    let mut sim = TowerSim::create_box("rush", "Rush", None);
    sim.free_build = true;
    let gap = |x: i64| (CE..CE + CAR_ELEVATOR_WIDTH).contains(&x) || (second && (CE2..CE2 + CAR_ELEVATOR_WIDTH).contains(&x));
    for x in 60..=231 {
        if !gap(x) {
            place(&mut sim, x, GROUND_Y, "lobby");
        }
    }
    place(&mut sim, CE, B3, "carElevator");
    if second {
        place(&mut sim, CE2, B3, "carElevator");
    }
    for floor in 1..=4 {
        for x in 60..=231 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for y in [B1, B2, B3] {
        let mut x = CE - 4;
        while x >= 60 {
            place(&mut sim, x, y, "parking");
            x -= 4;
        }
        let mut x = CE + CAR_ELEVATOR_WIDTH;
        while x + 3 <= 231 {
            if !gap(x) && !gap(x + 3) {
                place(&mut sim, x, y, "parking");
            }
            x += 4;
        }
    }
    // Offices whose workers drive: (floor anchor + column) % 4 == 1.
    for floor in 1..=office_floors {
        let anchor = 10 + floor;
        let mut x = 60;
        while (anchor + x) % 4 != 1 {
            x += 1;
        }
        while x + 8 <= 231 {
            if !(x..x + 9).any(|c| c == 120 || c == 121 || c == 122 || c == 123 || c == 140 || c == 141 || c == 142 || c == 143) {
                place(&mut sim, x, GROUND_Y - floor, "office");
            }
            x += 12;
        }
    }
    for floor in 0..=4 {
        place(&mut sim, 120, GROUND_Y - floor, "elevator");
        place(&mut sim, 140, GROUND_Y - floor, "elevator");
    }
    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

/// (average wait in ticks per car, longest street line, cars that rode a
/// platform, cars turned away)
fn rush(sim: &mut TowerSim, ticks: i64) -> (i64, i64, i64, usize) {
    let mut longest = 0;
    let mut away = 0;
    for _ in 0..ticks {
        let r = sim.step();
        away += r.notifications.iter().filter(|m| m.message.contains("turned away")).count();
        for p in platform_views(sim.box_ext()) {
            longest = longest.max(p.line);
        }
    }
    let st = sim.box_ext();
    let (w, r) = st.car_elevators.iter().fold((0, 0), |(w, r), e| (w + e.wait_total, r + e.rides));
    (if r > 0 { w / r } else { 0 }, longest, r, away)
}

#[test]
fn rush_hour_queues_at_one_car_elevator_and_a_second_relieves_it() {
    // Offices on two floors: about 150 drivers for one platform to B3.
    let one = rush(&mut rush_tower(false, 2), 2 * DAY);
    let two = rush(&mut rush_tower(true, 2), 2 * DAY);
    eprintln!("one: {one:?}  two: {two:?}");
    assert!(one.1 > tower_sim::box_ext::car_elevator::STREET_QUEUE_LIMIT, "no queue built up");
    assert!(two.0 < one.0, "the second car elevator did not shorten the wait");
    // (not the office workers' stress: those who drive now ride up from
    // the car park, so more cars in means more lift rides from below)
    assert!(two.2 > one.2, "the second car elevator did not take more cars");
    assert!(one.3 > 0, "a full line turns cars away");
    assert!(two.3 < one.3, "fewer are turned away with two");
}

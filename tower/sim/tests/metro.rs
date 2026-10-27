//! BOX RULES' metro commuters (box_ext/metro.rs, docs/metro.md): with a
//! station and a lift down to it, a share of the office workers come and go
//! by train, and the lobby's morning rush is the lighter for it.

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::{y_to_floor, RouteState, GROUND_Y};

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
    assert!(r.accepted, "place {tile_type}@{x},{y}: {:?}", r.reason);
}

/// Offices on four floors over a lobby 100..=200; a station under the
/// lobby if asked; a lift at 190 from the station's floor (or the lobby) up
fn tower(station: bool) -> TowerSim {
    let mut sim = TowerSim::create_box("metro", "Metro", None);
    sim.free_build = true;
    for x in 100..=200 {
        place(&mut sim, x, GROUND_Y, "lobby");
        place(&mut sim, x, GROUND_Y + 1, "floor");
        for floor in 1..=4 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for floor in 1..=4 {
        for i in 0..8 {
            place(&mut sim, 110 + 9 * i, GROUND_Y - floor, "office");
        }
    }
    if station {
        place(&mut sim, 120, GROUND_Y + 1, "metro");
    }
    for floor in -1..=4 {
        place(&mut sim, 190, GROUND_Y - floor, "elevator");
    }
    sim.free_build = false;
    sim.set_star_count(2);
    sim
}

/// Over two days: the most people setting off from the station at once, and
/// the people setting off from the lobby, counted a tick at a time.
fn run(sim: &mut TowerSim) -> (usize, usize) {
    let station = y_to_floor(GROUND_Y + 1);
    let lobby = y_to_floor(GROUND_Y);
    let (mut from_station, mut lobby_peak) = (0, 0);
    for _ in 0..2 * 2600 {
        sim.step();
        let w = sim.world();
        let carriers = |f: i64| w.sims.iter().filter(|s| matches!(s.route, RouteState::Carrier { source, .. } if source == f)).count();
        from_station = from_station.max(carriers(station));
        lobby_peak += carriers(lobby);
    }
    (from_station, lobby_peak)
}

#[test]
fn some_come_by_train() {
    let (_, without) = run(&mut tower(false));
    let (station, with) = run(&mut tower(true));
    eprintln!("lobby rush: {without} without a station, {with} with; {station} at the station at once");
    assert!(station > 0, "people set off from the station");
    assert!(with < without, "the lobby is less busy");
}

#[test]
fn not_without_box_rules() {
    let mut sim = tower(true);
    sim.set_box_rules(false);
    let (station, _) = run(&mut sim);
    assert_eq!(station, 0);
}

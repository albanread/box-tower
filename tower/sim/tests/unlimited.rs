//! BOX RULES raise the funds' cap (ledger.rs BOX_CASH_CAP), so the
//! unlimited tower's billion is not cut to upstream's $99,999,999 by the
//! first income; a classic game keeps upstream's cap.

use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::world::GROUND_Y;

fn shops(box_rules: bool) -> TowerSim {
    let mut sim = if box_rules { TowerSim::create_box("u", "U", None) } else { TowerSim::create("u", "U", None) };
    sim.free_build = true;
    for x in 100..=160 {
        sim.submit_command(&SimCommand::PlaceTile { x, y: GROUND_Y, tile_type: "lobby".into() });
        sim.submit_command(&SimCommand::PlaceTile { x, y: GROUND_Y - 1, tile_type: "floor".into() });
    }
    for i in 0..5 {
        sim.submit_command(&SimCommand::PlaceTile { x: 100 + 9 * i, y: GROUND_Y - 1, tile_type: "office".into() });
    }
    sim.submit_command(&SimCommand::PlaceTile { x: 150, y: GROUND_Y, tile_type: "elevator".into() });
    sim.submit_command(&SimCommand::PlaceTile { x: 150, y: GROUND_Y - 1, tile_type: "elevator".into() });
    sim.free_build = false;
    sim.set_cash(1_000_000_000);
    sim
}

#[test]
fn a_billion_is_kept_under_box_rules() {
    let mut sim = shops(true);
    for _ in 0..3 * 2600 {
        sim.step();
    }
    assert!(sim.cash() > 900_000_000, "funds {}", sim.cash());
    // and survives saving and loading
    let again = TowerSim::from_snapshot(&sim.save_state());
    assert_eq!(again.ledger().cap(), tower_sim::ledger::BOX_CASH_CAP);
}

#[test]
fn a_classic_game_keeps_upstreams_cap() {
    let mut sim = shops(false);
    for _ in 0..3 * 2600 {
        sim.step();
    }
    assert!(sim.cash() <= 99_999_999, "funds {}", sim.cash());
    assert!(serde_json::to_value(sim.save_state()).unwrap()["ledger"].get("boxCashCap").is_none());
}

//! ts: determinism.test.ts
//!
//! Determinism tests for the lockstep architecture.
//!
//! The multiplayer flow assumes:
//!   1. `saveState()` → `fromSnapshot()` → `saveState()` round-trip is identity
//!      (hydration must not mutate state).
//!   2. After hydration, two sims started from the same snapshot must produce
//!      bit-equal state when stepped the same number of ticks (deterministic
//!      forward sim).
//!
//! Either property failing causes a new joiner's local TowerSim to drift from
//! the server's, manifesting as missing sims and out-of-sync unitStatus
//! (e.g. for-sale banner timing).
//!
//! Upstream's loops over warmups and step counts are loops inside one test
//! here, each failure naming its case; the fixture cases are one test each.
//! Ignored until the merge, as tests/trace.rs is.

use std::fs;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use tower_sim::index::{SimCommand, SimSnapshot, TowerSim};
use tower_sim::js;
use tower_sim::world::{LobbyMode, GROUND_Y};

fn fixture_dir() -> std::path::PathBuf {
    let upstream = std::env::var("TOWER_UPSTREAM").unwrap_or_else(|_| "/Volumes/xc/BOX/src/tower-together".into());
    std::path::PathBuf::from(upstream).join("apps/worker/src/sim/fixtures")
}

#[derive(Clone, Debug, Deserialize)]
struct FacilitySpec {
    #[serde(rename = "type")]
    kind: String,
    floor: Option<i64>,
    left: i64,
    right: Option<i64>,
    bottom: Option<i64>,
    top: Option<i64>,
    cars: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
struct Extent {
    left: i64,
    right: i64,
}

#[derive(Clone, Debug, Deserialize)]
struct BuildSpec {
    floor_extent: IndexMap<String, Extent>,
    facilities: Vec<FacilitySpec>,
}

const FIXTURE_TILE_MAP: &[(&str, &str)] = &[
    ("office", "office"),
    ("condo", "condo"),
    ("restaurant", "restaurant"),
    ("fast-food", "fastFood"),
    ("retail", "retail"),
    ("cinema", "cinema"),
    ("party-hall", "partyHall"),
    ("single", "hotelSingle"),
    ("twin", "hotelTwin"),
    ("suite", "hotelSuite"),
    ("stairs", "stairs"),
    ("security", "security"),
    ("housekeeping", "housekeeping"),
    ("medical", "medical"),
    ("lobby", "lobby"),
];

fn submit_place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) -> tower_sim::index::CommandResult {
    sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() })
}

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let result = submit_place(sim, x, y, tile_type);
    if !result.accepted {
        panic!("place {tile_type}@{x},{y}: {}", result.reason.unwrap_or_default());
    }
}

fn place_facility_list(sim: &mut TowerSim, facilities: &[FacilitySpec]) {
    for fac in facilities {
        if fac.kind == "elevator" || fac.kind == "elevatorExpress" || fac.kind == "elevatorService" {
            let bottom = fac.bottom.or(fac.floor).unwrap_or(0);
            let top = fac.top.or(fac.floor).unwrap_or(0);
            for f in bottom..=top {
                submit_place(sim, fac.left, GROUND_Y - f, &fac.kind);
            }
            let num_cars = fac.cars.unwrap_or(1);
            let span = top - bottom;
            for i in 1..num_cars {
                let home_floor = if num_cars <= 1 { bottom } else { bottom + js::floor_div(span * i, num_cars - 1) };
                sim.submit_command(&SimCommand::AddElevatorCar { x: fac.left, y: GROUND_Y - home_floor });
            }
            continue;
        }
        let Some(tile_type) = js::record_get(FIXTURE_TILE_MAP, fac.kind.as_str()).copied() else {
            panic!("Unknown fixture tile type: {}", fac.kind);
        };
        let Some(floor) = fac.floor else {
            panic!("Facility {} missing 'floor'", fac.kind);
        };
        let y = GROUND_Y - floor;
        if tile_type == "lobby" {
            if let Some(right) = fac.right {
                for x in fac.left..right {
                    submit_place(sim, x, y, tile_type);
                }
                continue;
            }
        }
        submit_place(sim, fac.left, y, tile_type);
    }
}

fn build_from_fixture(name: &str) -> TowerSim {
    let path = fixture_dir().join(format!("build_{name}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec: BuildSpec = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    // Fixtures dump the binary's floor 14, 29, 44, ... sky-lobby cadence.
    let mut sim = TowerSim::create("determinism-fixture", "Determinism Fixture", Some(LobbyMode::PerfectParity));
    sim.free_build = true;

    if let Some(ground_extent) = spec.floor_extent.get("0") {
        for x in ground_extent.left..ground_extent.right {
            submit_place(&mut sim, x, GROUND_Y, "lobby");
        }
    }

    let mut floors: Vec<i64> = spec.floor_extent.keys().map(|k| k.parse().unwrap()).filter(|&f| f != 0).collect();
    floors.sort();

    for floor in floors {
        let extent = &spec.floor_extent[&floor.to_string()];
        let y = GROUND_Y - floor;
        let tile_type = if floor % 15 == 14 { "lobby" } else { "floor" };
        for x in extent.left..extent.right {
            submit_place(&mut sim, x, y, tile_type);
        }
    }

    place_facility_list(&mut sim, &spec.facilities);

    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

fn build_small_mixed_tower() -> TowerSim {
    let mut sim = TowerSim::create("determinism", "Determinism", None);
    sim.free_build = true;

    // Ground floor lobby across x=50..110.
    for x in 50..=110 {
        place(&mut sim, x, GROUND_Y, "lobby");
    }

    // Floor support for floors 1..3.
    for floor in 1..=3 {
        let y = GROUND_Y - floor;
        for x in 50..=110 {
            place(&mut sim, x, y, "floor");
        }
    }

    // Floor 1: hotel rooms + a stair pair from ground.
    place(&mut sim, 60, GROUND_Y - 1, "hotelSingle");
    place(&mut sim, 64, GROUND_Y - 1, "hotelSingle");
    place(&mut sim, 68, GROUND_Y - 1, "hotelTwin");

    // Floor 2: offices.
    place(&mut sim, 60, GROUND_Y - 2, "office");
    place(&mut sim, 70, GROUND_Y - 2, "office");

    // Floor 3: condo.
    place(&mut sim, 60, GROUND_Y - 3, "condo");

    // Stairs from ground up through floor 3.
    for floor in 1..=3 {
        place(&mut sim, 84, GROUND_Y - floor, "stairs");
    }

    // Elevator shaft from ground up through floor 3 with one car.
    for floor in 0..=3 {
        place(&mut sim, 100, GROUND_Y - floor, "elevator");
    }

    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

fn step_n(sim: &mut TowerSim, ticks: i64) {
    for _ in 0..ticks {
        sim.step();
    }
}

fn preview(value: Option<&Value>) -> String {
    let Some(value) = value else { return "undefined".to_string() };
    let s = value.to_string();
    if s.chars().count() > 80 {
        format!("{}...", s.chars().take(77).collect::<String>())
    } else {
        s
    }
}

/// Returns the first JSON path where `a` and `b` differ, or None if equal.
/// (`None` for a value is a missing key: upstream's `undefined`.)
fn find_first_diff(a: Option<&Value>, b: Option<&Value>, path: &str) -> Option<String> {
    match (a, b) {
        (Some(Value::Array(x)), Some(Value::Array(y))) => {
            if x.len() != y.len() {
                return Some(format!("{path}.length: {} !== {}", x.len(), y.len()));
            }
            x.iter().zip(y).enumerate().find_map(|(i, (p, q))| find_first_diff(Some(p), Some(q), &format!("{path}[{i}]")))
        }
        (Some(Value::Array(_)), Some(_)) | (Some(_), Some(Value::Array(_))) if a != b => {
            Some(format!("{path}: array vs non-array"))
        }
        (Some(Value::Object(x)), Some(Value::Object(y))) => {
            let mut all: Vec<&String> = x.keys().collect();
            all.extend(y.keys().filter(|k| !x.contains_key(*k)));
            all.into_iter().find_map(|k| find_first_diff(x.get(k), y.get(k), &format!("{path}.{k}")))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: {} !== {}", preview(a), preview(b))),
    }
}

fn json_round_trip(snapshot: &SimSnapshot) -> Value {
    serde_json::to_value(snapshot).unwrap()
}

fn expect_snapshots_equal(a: &SimSnapshot, b: &SimSnapshot, label: &str) {
    if let Some(diff) = find_first_diff(Some(&json_round_trip(a)), Some(&json_round_trip(b)), "") {
        panic!("{label}: snapshots diverge at {diff}");
    }
}

// describe("snapshot hydration is idempotent")
#[test]
#[ignore = "fails at other groups' todo! stubs until the merge"]
fn save_state_from_snapshot_save_state_equals_original() {
    for warmup_ticks in [0, 50, 500, 2_500] {
        let mut server = build_small_mixed_tower();
        step_n(&mut server, warmup_ticks);

        let before = server.save_state();
        let rehydrated = TowerSim::from_snapshot(&before);
        let after = rehydrated.save_state();

        expect_snapshots_equal(&before, &after, &format!("warmup={warmup_ticks}"));
    }
}

// describe("two sims hydrated from the same snapshot stay in lockstep")
#[test]
#[ignore = "fails at other groups' todo! stubs until the merge"]
fn server_vs_client_diverge_after_n_ticks() {
    for warmup_ticks in [0, 50, 500, 2_500] {
        for step_ticks in [1, 50, 500] {
            let mut server = build_small_mixed_tower();
            step_n(&mut server, warmup_ticks);

            // "Client" hydrates from server's snapshot — same state lockstep
            // architecture sends to a new joiner.
            let snapshot = server.save_state();
            let mut client = TowerSim::from_snapshot(&snapshot);

            // Both step forward independently. With no inputs, they must
            // produce bit-equal state at every tick.
            step_n(&mut server, step_ticks);
            step_n(&mut client, step_ticks);

            expect_snapshots_equal(
                &server.save_state(),
                &client.save_state(),
                &format!("warmup={warmup_ticks} step={step_ticks}"),
            );
        }
    }
}

#[test]
#[ignore = "fails at other groups' todo! stubs until the merge"]
fn rng_advances_in_lockstep() {
    let mut server = build_small_mixed_tower();
    step_n(&mut server, 200);

    let snapshot = server.save_state();
    let mut client = TowerSim::from_snapshot(&snapshot);

    for _ in 0..250 {
        server.step();
        client.step();
        let a = server.save_state();
        let b = client.save_state();
        assert_eq!(b.world.rng_state, a.world.rng_state);
        assert_eq!(b.world.rng_call_count, a.world.rng_call_count);
        assert_eq!(b.time.total_ticks, a.time.total_ticks);
    }
}

// Fixture-based determinism: bigger towers exercise more code paths
// (parking, sky lobby, dense hotels/offices) where snapshot drift is
// most likely to surface. Larger fixtures step slowly, so cap warmup.
fn hydration_is_idempotent_after_warmup(fixture: &str) {
    let mut server = build_from_fixture(fixture);
    step_n(&mut server, 1_000);
    let before = server.save_state();
    let after = TowerSim::from_snapshot(&before).save_state();
    expect_snapshots_equal(&before, &after, &format!("{fixture} idempotent"));
}

fn server_vs_hydrated_client_stay_in_lockstep(fixture: &str) {
    let mut server = build_from_fixture(fixture);
    step_n(&mut server, 1_000);
    let mut client = TowerSim::from_snapshot(&server.save_state());

    step_n(&mut server, 500);
    step_n(&mut client, 500);

    expect_snapshots_equal(&server.save_state(), &client.save_state(), &format!("{fixture} lockstep"));
}

macro_rules! fixture_cases {
    ($($idempotent:ident, $lockstep:ident => $name:literal,)*) => {
        $(
            #[test]
            #[ignore = "fails at other groups' todo! stubs until the merge"]
            fn $idempotent() {
                hydration_is_idempotent_after_warmup($name);
            }

            #[test]
            #[ignore = "fails at other groups' todo! stubs until the merge"]
            fn $lockstep() {
                server_vs_hydrated_client_stay_in_lockstep($name);
            }
        )*
    };
}

fixture_cases! {
    fixture_mixed_idempotent, fixture_mixed_lockstep => "mixed",
    fixture_hotel_idempotent, fixture_hotel_lockstep => "hotel",
    fixture_offices_idempotent, fixture_offices_lockstep => "offices",
    fixture_condo_idempotent, fixture_condo_lockstep => "condo",
    fixture_commercial_idempotent, fixture_commercial_lockstep => "commercial",
    fixture_elevator_idempotent, fixture_elevator_lockstep => "elevator",
    fixture_mixed_elevator_idempotent, fixture_mixed_elevator_lockstep => "mixed_elevator",
}

#[test]
fn first_diff_names_the_path() {
    let a = serde_json::json!({"w": {"s": [1, 2, {"k": 3}]}, "x": 1});
    let b = serde_json::json!({"w": {"s": [1, 2, {"k": 4}]}, "x": 1});
    assert_eq!(find_first_diff(Some(&a), Some(&b), "").as_deref(), Some(".w.s[2].k: 3 !== 4"));
    let c = serde_json::json!({"x": 1, "y": 2});
    assert_eq!(find_first_diff(Some(&a), Some(&c), "").as_deref(), Some(".w: {\"s\":[1,2,{\"k\":3}]} !== undefined"));
    assert_eq!(find_first_diff(Some(&a), Some(&a), ""), None);
}

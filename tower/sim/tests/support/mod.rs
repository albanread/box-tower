//! The trace fixtures' tower building, shared by tests/trace.rs and the
//! lockstep binary (src/bin/tower-lockstep.rs includes this file by path),
//! so that both build every tower with the same code.  ts: trace.test.ts,
//! the parts that build and seed a tower.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::js;
use tower_sim::time::{DAY_TICK_MAX, DAY_TICK_NEW_DAY, NEW_GAME_DAY_TICK};
use tower_sim::world::{LobbyMode, GROUND_Y};

/// Upstream's fixture directory in the checkout (TOWER_UPSTREAM, default
/// /Volumes/xc/BOX/src/tower-together).
pub fn fixture_dir() -> PathBuf {
    let upstream = std::env::var("TOWER_UPSTREAM").unwrap_or_else(|_| "/Volumes/xc/BOX/src/tower-together".into());
    PathBuf::from(upstream).join("apps/worker/src/sim/fixtures")
}

// ─── Types ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct FacilitySpec {
    #[serde(rename = "type")]
    pub kind: String,
    pub floor: Option<i64>,
    pub left: i64,
    pub right: Option<i64>,
    pub bottom: Option<i64>,
    pub top: Option<i64>,
    pub cars: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScheduledBatch {
    pub day: i64,
    pub tick: i64,
    pub facilities: Vec<FacilitySpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Extent {
    pub left: i64,
    pub right: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BuildSpec {
    pub floor_extent: IndexMap<String, Extent>,
    pub facilities: Vec<FacilitySpec>,
    pub scheduled_facilities: Option<Vec<ScheduledBatch>>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct TraceCar {
    pub currentFloor: i64,
    pub directionFlag: i64,
    pub targetFloor: i64,
    pub stabilizeCounter: Option<i64>,
    pub dwellCounter: Option<i64>,
    pub assignedCount: Option<i64>,
    pub prevFloor: Option<i64>,
    pub arrivalSeen: Option<i64>,
    pub arrivalTick: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TraceCarrier {
    pub column: i64,
    pub mode: i64,
    pub cars: Vec<TraceCar>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Gates {
    pub security: bool,
    pub office: bool,
    pub recycling: bool,
    pub route: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SimGroup {
    pub count: i64,
    pub stress_avg: Option<f64>,
    pub stress_min: Option<f64>,
    pub stress_max: Option<f64>,
    pub states: BTreeMap<String, i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TraceFire {
    pub flags: i64,
    pub fire_active: bool,
    pub fire_floor: i64,
    pub fire_tile: i64,
    pub fire_start_tick: i64,
    pub rescue_countdown: i64,
    pub helicopter_extinguish_pos: i64,
    pub fire_left_pos: BTreeMap<String, i64>,
    pub fire_right_pos: BTreeMap<String, i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TraceEntry {
    pub day: i64,
    pub tick: i64,
    pub daypart: i64,
    pub stars: i64,
    pub cash: i64,
    pub metro_floor: i64,
    pub population: i64,
    pub rng_calls: Option<i64>,
    pub gates: Gates,
    pub sims: IndexMap<String, SimGroup>,
    pub carriers: Option<Vec<TraceCarrier>>,
    pub fire: Option<TraceFire>,
}

// ─── Fixture tile name → sim tile name mapping ─────────────────────────────

pub const FIXTURE_TILE_MAP: &[(&str, &str)] = &[
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
    ("metro", "metro"),
    ("cathedral", "cathedral"),
    ("lobby", "lobby"),
];

// ─── Helpers ────────────────────────────────────────────────────────────────

/// The spec, the trace parsed, and the trace's lines as JSON values (for
/// dropTerminalDuplicateDump's comparison).
pub fn load_fixture(name: &str) -> (BuildSpec, Vec<TraceEntry>, Vec<Value>) {
    let dir = fixture_dir();
    let spec_path = dir.join(format!("build_{name}.json"));
    let trace_path = dir.join(format!("build_{name}.jsonl"));
    let spec_text = fs::read_to_string(&spec_path)
        .unwrap_or_else(|e| panic!("{}: {e} (TOWER_UPSTREAM names the checkout)", spec_path.display()));
    let spec: BuildSpec = serde_json::from_str(&spec_text).unwrap_or_else(|e| panic!("{}: {e}", spec_path.display()));
    let trace_text =
        fs::read_to_string(&trace_path).unwrap_or_else(|e| panic!("{}: {e}", trace_path.display()));
    let raw: Vec<Value> = trace_text
        .trim()
        .split('\n')
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {e}", trace_path.display())))
        .collect();
    let trace: Vec<TraceEntry> = raw
        .iter()
        .map(|v| serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{}: {e}", trace_path.display())))
        .collect();
    (spec, trace, raw)
}

/// `JSON.stringify(withoutTime(entry))`.
pub fn without_time(entry: &Value) -> String {
    let mut v = entry.clone();
    if let Value::Object(m) = &mut v {
        m.shift_remove("day");
        m.shift_remove("tick");
    }
    serde_json::to_string(&v).unwrap()
}

pub fn drop_terminal_duplicate_dump(mut trace: Vec<TraceEntry>, raw: &[Value]) -> Vec<TraceEntry> {
    if trace.len() < 2 {
        return trace;
    }
    let prev = &raw[raw.len() - 2];
    let last = &raw[raw.len() - 1];
    if without_time(prev) != without_time(last) {
        return trace;
    }
    // The emulator trace writer appends one final dump after the run loop.
    // Most fixtures duplicate the same tick; build_sky_office labels that
    // terminal duplicate as the next tick even though no simulation state ran.
    trace.pop();
    trace
}

pub fn trace_tick_to_total_ticks(day: i64, tick: i64) -> i64 {
    // Day 0 starts at dayTick=2533, runs through 2599→0→...→2299.
    // Day D (D>=1) starts at dayTick=2300, runs 2600 ticks to next 2300.
    if day == 0 {
        return (tick - NEW_GAME_DAY_TICK + DAY_TICK_MAX) % DAY_TICK_MAX;
    }
    let day0_length = DAY_TICK_MAX - NEW_GAME_DAY_TICK + DAY_TICK_NEW_DAY;
    day0_length + (day - 1) * DAY_TICK_MAX + ((tick - DAY_TICK_NEW_DAY + DAY_TICK_MAX) % DAY_TICK_MAX)
}

pub fn place_tile(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
}

pub fn place_facility_list(sim: &mut TowerSim, facilities: &[FacilitySpec]) {
    for fac in facilities {
        if fac.kind == "elevator" || fac.kind == "elevatorExpress" || fac.kind == "elevatorService" {
            let bottom = fac.bottom.or(fac.floor).unwrap_or(0);
            let top = fac.top.or(fac.floor).unwrap_or(0);
            for f in bottom..=top {
                place_tile(sim, fac.left, GROUND_Y - f, &fac.kind);
            }
            let num_cars = fac.cars.unwrap_or(1);
            // Mirror upstream's emulator.py `build_carrier` add-car placement:
            // standard/service carriers get extras at evenly-spaced home
            // floors across [bottom, top]; express (mode 0) cycles through
            // the binary's valid stops (floors 1..10 and sky-lobby stops at
            // (floor-10)%15==14, i.e. 24, 39, 54, …).
            let is_express = fac.kind == "elevatorExpress";
            // Array.from({ length: numCars - 1 }): no elements for numCars < 2.
            let extra = (num_cars - 1).max(0);
            let home_floors: Vec<i64> = if is_express {
                let mut valid_stops = vec![bottom];
                for f in (bottom + 1)..=top {
                    let exe = f + 10;
                    if exe >= 11 && (exe - 10) % 15 == 14 {
                        valid_stops.push(f);
                    }
                }
                (0..extra)
                    .map(|i| valid_stops[((i + 1) % valid_stops.len() as i64) as usize])
                    .collect()
            } else {
                let span = top - bottom;
                (0..extra)
                    .map(|i| {
                        if num_cars <= 1 {
                            bottom
                        } else {
                            bottom + js::floor_div(span * (i + 1), num_cars - 1)
                        }
                    })
                    .collect()
            };
            for home_floor in home_floors {
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
        // Wide lobby placements span [left, right); iterate single tiles.
        if tile_type == "lobby" {
            if let Some(right) = fac.right {
                for x in fac.left..right {
                    place_tile(sim, x, y, tile_type);
                }
                continue;
            }
        }
        place_tile(sim, fac.left, y, tile_type);
    }
}

pub fn place_tiles_from_spec(sim: &mut TowerSim, spec: &BuildSpec) {
    sim.free_build = true;

    // Place lobby on ground floor
    if let Some(ground_extent) = spec.floor_extent.get("0") {
        for x in ground_extent.left..ground_extent.right {
            place_tile(sim, x, GROUND_Y, "lobby");
        }
    }

    // Place floor support for each non-ground floor (sorted bottom-up)
    let mut floors: Vec<i64> = spec
        .floor_extent
        .keys()
        .map(|k| k.parse::<i64>().unwrap_or_else(|_| panic!("floor_extent key {k:?}")))
        .filter(|&f| f != 0)
        .collect();
    floors.sort();

    for floor in floors {
        let extent = &spec.floor_extent[&floor.to_string()];
        let y = GROUND_Y - floor;
        // Sky lobbies sit on floors where (floor % 15 == 14) — game floors 14,
        // 29, 44, ... — matching the binary's express-stop convention and
        // `isValidLobbyY`. The Python emulator direct-writes tile 0x0b at those
        // levels so FAMILY_PARKING transfer-group entries form at internal
        // floor 24, 39, 54, ...
        let tile_type = if floor % 15 == 14 { "lobby" } else { "floor" };
        for x in extent.left..extent.right {
            place_tile(sim, x, y, tile_type);
        }
    }

    place_facility_list(sim, &spec.facilities);

    sim.free_build = false;
}

/// Compute the 32-bit LCG state after N calls starting from a given seed.
pub fn compute_rng_state(seed: i64, calls: i64) -> i64 {
    let mut state = seed;
    for _ in 0..calls {
        state = js::to_int32(js::imul(state, 0x15a4e35) + 1);
    }
    state
}

pub fn prepare_from_trace(spec: &BuildSpec, trace: &[TraceEntry]) -> TowerSim {
    // Trace fixtures are dumped from the binary, which places sky lobbies at
    // floors 14, 29, 44, ... — pin the world to perfect-parity so placement
    // validation and express-stop cadence match.
    let mut sim = TowerSim::create("trace-test", "Trace Test", Some(LobbyMode::PerfectParity));
    // Place tiles at dayTick=2533 (day -1), matching the binary.
    place_tiles_from_spec(&mut sim, spec);
    // Seed cash, rng state, and rng count from the day -1 baseline (trace[0]).
    let mut snap = sim.save_state();
    snap.ledger.cash_balance = trace[0].cash;
    snap.world.star_count = trace[0].stars;
    snap.world.current_population = trace[0].population;
    if let Some(rng_calls) = trace[0].rng_calls {
        snap.world.rng_call_count = rng_calls;
        snap.world.rng_state = compute_rng_state(1, rng_calls);
    }
    snap.world.event_state.disable_news_events = Some(true);
    // Seed each car's currentFloor / targetFloor / prevFloor from the
    // baseline dump. The emulator's `build_carrier` drives the binary's
    // own placement + add-car path, which for express carriers only
    // accepts floors 1..10 and sky-lobby stops (24, 39, 54, ...). Deriving
    // home positions from a simple even-spread across [bottom, top] would
    // place cars on non-stop floors the binary never allows, so use the
    // fixture's actual baseline instead.
    let baseline_carriers = trace[0].carriers.clone().unwrap_or_default();
    for ref_carrier in &baseline_carriers {
        let Some(carrier) = snap.world.carriers.iter_mut().find(|c| c.column == ref_carrier.column) else {
            continue;
        };
        let mut c = 0;
        while c < ref_carrier.cars.len() && c < carrier.cars.len() {
            let ref_car = &ref_carrier.cars[c];
            let our_car = &mut carrier.cars[c];
            our_car.current_floor = ref_car.currentFloor;
            our_car.target_floor = ref_car.targetFloor;
            our_car.prev_floor = ref_car.prevFloor.unwrap_or(ref_car.currentFloor);
            c += 1;
        }
    }
    TowerSim::from_snapshot(&snap)
    // No pre-advance — the test loop drives advancement through the day boundary.
}

pub fn advance_to(sim: &mut TowerSim, target_total_ticks: i64) {
    while sim.sim_time() < target_total_ticks {
        sim.step();
    }
}

/// The fire fixtures' common seeding: upstream's two tests differ in which
/// fields they write (the accept test leaves weekendFlag, population,
/// rescueCountdown and helicopterExtinguishPos alone).
pub fn seed_fire(spec: &BuildSpec, seed: &TraceEntry, tower_id: &str, name: &str, accept: bool) -> TowerSim {
    let fire = seed.fire.as_ref().expect("fixture must include fire state");
    let mut sim = TowerSim::create(tower_id, name, Some(LobbyMode::PerfectParity));
    place_tiles_from_spec(&mut sim, spec);
    let mut snap = sim.save_state();
    snap.time.day_counter = seed.day;
    snap.time.day_tick = seed.tick;
    snap.time.daypart_index = seed.daypart;
    if !accept {
        snap.time.weekend_flag = if seed.day % 3 == 2 { 1 } else { 0 };
    }
    snap.world.star_count = seed.stars;
    if !accept {
        snap.world.current_population = seed.population;
    }
    snap.ledger.cash_balance = seed.cash;
    let es = &mut snap.world.event_state;
    es.disable_news_events = Some(true);
    es.game_state_flags = fire.flags;
    es.fire_floor = fire.fire_floor;
    es.fire_tile = fire.fire_tile;
    es.fire_start_tick = fire.fire_start_tick;
    if !accept {
        es.rescue_countdown = fire.rescue_countdown;
        es.helicopter_extinguish_pos = fire.helicopter_extinguish_pos;
    }
    es.fire_left_pos.iter_mut().for_each(|v| *v = 0xffff);
    es.fire_right_pos.iter_mut().for_each(|v| *v = 0xffff);
    for (k, v) in &fire.fire_left_pos {
        es.fire_left_pos[k.parse::<usize>().unwrap()] = *v;
    }
    for (k, v) in &fire.fire_right_pos {
        es.fire_right_pos[k.parse::<usize>().unwrap()] = *v;
    }
    TowerSim::from_snapshot(&snap)
}


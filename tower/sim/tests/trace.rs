//! ts: trace.test.ts
//!
//! Trace tests — build towers from fixture JSON specs, step through
//! the simulation, and compare against reference JSONL traces.
//!
//! Each fixture pair (build_X.json + build_X.jsonl) defines:
//!   - .json: floor extents and facility placements
//!   - .jsonl: reference trace snapshots (day, tick, cash, sim states)
//!
//! The fixtures are upstream's, read from the checkout (TOWER_UPSTREAM,
//! default /Volumes/xc/BOX/src/tower-together; README.md): never copied
//! into this tree.  One #[test] per fixture, so a failure is named.
//!
//! Until every group's translation is merged these fail at the todo!
//! stubs, so they are #[ignore]d: `cargo test --release -- --ignored`
//! runs them.  Remove the `#[ignore]` lines at the merge.

mod support;

use std::collections::BTreeMap;

use indexmap::IndexMap;
use support::*;
use tower_sim::index::{SimCommand, TowerSim};
use tower_sim::js;
use tower_sim::time::DAY_TICK_MAX;
use tower_sim::world::{LobbyMode, PlacedObjectRecord};

// ─── Trace sim key → familyCode mapping ────────────────────────────────────

const TRACE_SIM_KEY_TO_FAMILY: &[(&str, i64)] = &[
    ("office", 7),
    ("condo", 9),
    ("restaurant", 6),
    ("fast-food", 12),
    ("retail", 10),
    ("cinema", 18),
    ("party-hall", 30),
    ("single", 3),
    ("twin", 4),
    ("suite", 5),
    ("security", 14),
    ("housekeeping", 15),
    ("cathedral-a", 0x24),
    ("cathedral-b", 0x24),
    ("cathedral-c", 0x24),
    ("cathedral-d", 0x24),
    ("cathedral-e", 0x24),
];

fn ref_fire_inactive(entry: &TraceEntry) -> bool {
    entry.fire.as_ref().is_some_and(|f| !f.fire_active)
}

/// Sum of all sim counts across families in a trace entry.
fn trace_sim_total(entry: &TraceEntry) -> i64 {
    entry.sims.values().map(|g| g.count).sum()
}

/// `Object.fromEntries` of value → count over `values`, as a map keyed by
/// the value's decimal string (assert.deepEqual on two objects ignores key
/// order, as a BTreeMap comparison does).
fn counts(values: impl IntoIterator<Item = i64>) -> BTreeMap<String, i64> {
    let mut m = BTreeMap::new();
    for v in values {
        *m.entry(v.to_string()).or_insert(0) += 1;
    }
    m
}

// ─── Test fixtures ──────────────────────────────────────────────────────────

// describe("trace: cathedral recovered path")
#[test]
#[ignore = "fails at other groups' todo! stubs until the merge"]
fn trace_cathedral_recovered_path_activates_the_40_evaluation_visitors() {
    let floor_extent: IndexMap<String, Extent> =
        (0..104).map(|floor| (floor.to_string(), Extent { left: 80, right: 180 })).collect();
    let mut sim = TowerSim::create("trace-cathedral", "Trace Cathedral", Some(LobbyMode::PerfectParity));
    place_tiles_from_spec(
        &mut sim,
        &BuildSpec {
            floor_extent,
            facilities: vec![FacilitySpec {
                kind: "cathedral".into(),
                floor: Some(103),
                left: 100,
                right: None,
                bottom: None,
                top: None,
                cars: None,
            }],
            scheduled_facilities: None,
        },
    );

    let before = sim.sims_to_array();
    assert_eq!(before.len(), 40);
    assert_eq!(counts(before.iter().map(|s| s.family_code)), counts([36; 40]));
    assert_eq!(counts(before.iter().map(|s| s.state_code)), counts([39; 40]));

    advance_to(&mut sim, trace_tick_to_total_ticks(0, 0));

    let after = sim.sims_to_array();
    assert_eq!(sim.star_count(), 1);
    assert_eq!(counts(after.iter().map(|s| s.state_code)), counts([32; 40]));
}

// describe("trace: build_fire (force-armed fire from binary)")
//
// build_fire: tower with security on F1 + offices on F11-13. The emulator
// fast-forwards to day 83 / tick 0, writes star_count=3 and security_count=1,
// then directly calls trigger_fire_event. Trace dumps capture spread,
// helicopter prompt (declined), cleanup, and post-cleanup steady state.
#[test]
#[ignore = "fails at other groups' todo! stubs until the merge"]
fn trace_build_fire_matches_binary_fire_spread_and_cleanup() {
    let (spec, trace, _) = load_fixture("fire");
    if trace.is_empty() {
        return;
    }

    // Seed time + economy + event state from trace[0] (the binary's
    // post-force-trigger snapshot). The TS sim has no force-trigger entry
    // point, so we mimic it by writing fire state directly.
    let seed = &trace[0];
    let fire = seed.fire.as_ref().expect("fixture must include fire state");
    let mut seeded = seed_fire(&spec, seed, "trace-fire", "Trace Fire", false);
    // Establish totalTicks baseline so advanceTo can compute relative steps.
    let base_total = seeded.sim_time();

    // At tick fireStartTick + helicopter_prompt_delay = 2 the binary fires
    // the rescue dialog and (in this fixture) records IDOK = decline. The
    // test bypasses the worker prompt queue (we seeded fire already active),
    // so simulate the decline by submitting a prompt_response at that tick
    // — that triggers spawnFireRescueHelpers like the binary does.
    let decline_at_tick = fire.fire_start_tick + 2;
    let mut decline_submitted = false;

    // Validate per-tick parity through the fire-active entries only. Once
    // the binary cleans up at dayTick==2000 and fast-forwards to 1500,
    // the trace's dayTick numbering diverges from a wall-clock step
    // counter and per-tick alignment becomes fragile. Verify cleanup
    // happened (fire flag cleared) and stop there.
    let last_fire_active = trace.iter().rposition(|e| e.fire.as_ref().is_some_and(|f| f.fire_active));
    let last_idx = match last_fire_active {
        Some(i) => i + 1,
        None => trace.len() - 1,
    };
    for entry_idx in 1..=last_idx {
        let entry = &trace[entry_idx];
        // Advance by the dayTick delta from previous entry. The fire fixture
        // stays on days 83-84, so dayTick deltas + day rollover suffice.
        let prev = &trace[entry_idx - 1];
        let mut delta = if entry.day == prev.day {
            entry.tick - prev.tick
        } else {
            // rollover: prev.day → entry.day. dayCounter increments at tick 2300.
            DAY_TICK_MAX - prev.tick + entry.tick
        };
        // Cleanup boundary: when the binary's fire resolves at dayTick==2000
        // it fast-forwards dayTick to 1500, so the trace shows a discontinuity
        // (e.g. tick 150 → tick 1541 with ~1891 actual ticks of simulation).
        // Detect prev=fire-active, current=fire-inactive within the same day
        // and add the extra 500 ticks the binary swallowed.
        if prev.fire.as_ref().is_some_and(|f| f.fire_active)
            && ref_fire_inactive(entry)
            && entry.day == prev.day
            && entry.tick >= 1500
        {
            delta += 500;
        }
        for _ in 0..delta {
            seeded.step();
            if !decline_submitted && seeded.save_state().time.day_tick >= decline_at_tick {
                seeded.submit_command(&SimCommand::PromptResponse {
                    prompt_id: format!("fire_{}", seed.day),
                    accepted: false,
                });
                decline_submitted = true;
            }
        }

        let ctx = format!("entry {entry_idx} day={} tick={}", entry.day, entry.tick);
        let ours = seeded.save_state();
        let our_fire = &ours.world.event_state;
        let ref_fire = entry.fire.as_ref().unwrap_or_else(|| panic!("trace entry {entry_idx} missing fire"));

        assert_eq!(our_fire.game_state_flags, ref_fire.flags, "gameStateFlags at {ctx}");
        assert_eq!(our_fire.fire_floor, ref_fire.fire_floor, "fireFloor at {ctx}");
        assert_eq!(our_fire.fire_tile, ref_fire.fire_tile, "fireTile at {ctx}");
        assert_eq!(our_fire.fire_start_tick, ref_fire.fire_start_tick, "fireStartTick at {ctx}");
        assert_eq!(
            our_fire.helicopter_extinguish_pos, ref_fire.helicopter_extinguish_pos,
            "helicopterExtinguishPos at {ctx}"
        );
        // Per-floor front comparison: collapse 120-entry arrays into the
        // same {floor: pos} sparse map the emulator dumps (omit 0xffff and 0).
        let sparse = |v: &[i64]| -> BTreeMap<String, i64> {
            v.iter()
                .enumerate()
                .filter(|(_, &p)| p != 0xffff)
                .map(|(f, &p)| (f.to_string(), p))
                .collect()
        };
        assert_eq!(sparse(&our_fire.fire_left_pos), ref_fire.fire_left_pos, "fireLeftPos at {ctx}");
        assert_eq!(sparse(&our_fire.fire_right_pos), ref_fire.fire_right_pos, "fireRightPos at {ctx}");
    }
    // Ensure we actually advanced (sanity check that delta loop ran).
    assert!(seeded.sim_time() > base_total, "test never stepped");

    // Post-fire coherence smoke check (the "game breaks after day-84 fire"
    // regression): advance well past cleanup and verify no sim points at
    // a destroyed facility, cash didn't go negative, and the fire flag
    // stays cleared. This is what would have caught the original bug.
    for _ in 0..200 {
        seeded.step();
    }
    let post = seeded.save_state();
    assert_eq!(post.world.event_state.game_state_flags & 8, 0, "fire flag re-armed after cleanup");
    assert_eq!(
        post.world.event_state.fire_rescue_helpers.len(),
        0,
        "firefighter helpers not drained at cleanup"
    );
    for sim in &post.world.sims {
        if sim.family_code == 0 {
            continue;
        }
        // Sims with a non-trivial home pointer must point to a real
        // placed object. Floor-anchor + home column should resolve.
        let obj_y = sim.floor_anchor;
        let mut home: Option<&PlacedObjectRecord> = None;
        for (key, rec) in post.world.placed_objects.iter() {
            let y: i64 = key.split(',').nth(1).and_then(|s| s.parse().ok()).unwrap();
            if y != obj_y {
                continue;
            }
            if sim.home_column >= rec.left_tile_index && sim.home_column <= rec.right_tile_index {
                home = Some(rec);
            }
        }
        let Some(home) = home else { continue };
        assert_eq!(
            home.object_type_code, sim.family_code,
            "orphaned sim post-fire: family={} home={},{}",
            sim.family_code, sim.floor_anchor, sim.home_column
        );
    }
    assert!(
        post.ledger.cash_balance >= 0,
        "cash went negative post-fire: {}",
        post.ledger.cash_balance
    );
}

// describe("trace: build_fire_accept (helicopter rescue accepted)")
//
// Companion to build_fire that accepts the helicopter prompt (DialogBox
// id 0xbc4 returns 2). The binary deducts $500K, sets
// `helicopter_extinguish_pos = bounds.right - 12`, and the extinguish
// loop sweeps fronts on the helicopter side until cleanup. Trace[0] is
// pre-decline; later entries show post-cleanup state with extinguish_pos
// non-zero and cash dropped by the rescue cost.
#[test]
#[ignore = "fails at other groups' todo! stubs until the merge"]
fn trace_build_fire_accept_deducts_rescue_cost_and_extinguishes_via_helicopter() {
    let (spec, trace, _) = load_fixture("fire_accept");
    if trace.is_empty() {
        return;
    }
    let seed = &trace[0];
    let fire = seed.fire.as_ref().expect("fixture must include fire state");
    let mut seeded = seed_fire(&spec, seed, "trace-fire-accept", "Trace Fire Accept", true);

    // Step until the helicopter prompt fires; submit ACCEPTED.
    let accept_at = fire.fire_start_tick + 2;
    let mut accepted = false;
    let cash_before = seeded.cash();
    let mut i = 0;
    while i < 250 && !accepted {
        seeded.step();
        if seeded.save_state().time.day_tick >= accept_at {
            seeded.submit_command(&SimCommand::PromptResponse {
                prompt_id: format!("fire_{}", seed.day),
                accepted: true,
            });
            accepted = true;
        }
        i += 1;
    }
    assert!(accepted, "never submitted accept");
    // Helicopter rescue cost = $500,000 deducted immediately.
    assert_eq!(
        cash_before - seeded.cash(),
        500_000,
        "expected $500K rescue deduction, got {}",
        cash_before - seeded.cash()
    );
    // helicopterExtinguishPos was seeded to a positive column (bounds.right - 12).
    assert!(
        seeded.save_state().world.event_state.helicopter_extinguish_pos > 0,
        "helicopterExtinguishPos not set on accept"
    );

    // Run through cleanup, then verify fire resolved and post-fire state coherent.
    for _ in 0..2500 {
        seeded.step();
    }
    let post = seeded.save_state();
    assert_eq!(
        post.world.event_state.game_state_flags & 8,
        0,
        "fire flag still set after helicopter extinguish + cleanup"
    );
}

// describe.each(FIXTURE_NAMES)("trace: build_%s") / it("matches full reference trace")
fn matches_full_reference_trace(fixture_name: &str) {
    let (spec, raw_trace, raw) = load_fixture(fixture_name);
    let trace = drop_terminal_duplicate_dump(raw_trace, &raw);

    // Entry 0 is the day -1 / tick 2533 baseline; simulation entries start at index 1.
    let sim_entries = &trace[1.min(trace.len())..];

    // Fields in the fixture that this suite does NOT currently check, because the
    // sim has no direct mapping for them yet:
    //   - calendar_phase       (global 12-day phase counter; not modeled in TimeState)
    //   - sim_allocated/initialized/uninitialized (sim pool allocator bookkeeping)
    if sim_entries.is_empty() {
        return;
    }
    let mut sim = prepare_from_trace(&spec, &trace);

    let mut prev_sim_calls: Option<i64> = None;
    let mut prev_trace_calls: Option<i64> = None;

    let mut pending_scheduled: Vec<ScheduledBatch> = spec.scheduled_facilities.clone().unwrap_or_default();

    for entry in sim_entries {
        advance_to(&mut sim, trace_tick_to_total_ticks(entry.day, entry.tick));
        // Fire any scheduled placement whose (day, tick) matches now, before
        // validating this entry — mirrors the emulator, which applies
        // placements at scheduler entry before the tick's dump. freeBuild
        // stays off so placement costs are charged (the initial batch was
        // placed in freeBuild mode; trace[0].cash already reflects its cost).
        let mut i = pending_scheduled.len();
        while i > 0 {
            i -= 1;
            if pending_scheduled[i].day == entry.day && pending_scheduled[i].tick == entry.tick {
                let batch = pending_scheduled.remove(i);
                place_facility_list(&mut sim, &batch.facilities);
            }
        }
        let ctx = format!("day={} tick={}", entry.day, entry.tick);

        // ── Scalar fields ──────────────────────────────────────────────
        let gf = sim.gate_flags().clone();
        assert_eq!(sim.daypart_index(), entry.daypart, "daypart mismatch at {ctx}");
        assert_eq!(sim.star_count(), entry.stars, "stars mismatch at {ctx}");
        assert_eq!(sim.current_population(), entry.population, "population mismatch at {ctx}");
        assert_eq!(gf.metro_station_floor_index, entry.metro_floor, "metro_floor mismatch at {ctx}");
        assert_eq!(gf.office_service_ok != 0, entry.gates.office, "gates.office at {ctx}");
        assert_eq!(gf.recycling_adequate != 0, entry.gates.recycling, "gates.recycling at {ctx}");
        assert_eq!(gf.routes_viable != 0, entry.gates.route, "gates.route at {ctx}");
        assert!(!entry.gates.security, "unexpected gates.security at {ctx}");

        // ── Cash ───────────────────────────────────────────────────────
        assert_eq!(sim.cash(), entry.cash, "cash mismatch at {ctx} fx={fixture_name}");

        // ── Sim counts & states (only for entries with named families) ─
        let is_active = !entry.sims.is_empty() && entry.sims.keys().any(|k| !k.starts_with("0x"));
        if is_active {
            let sim_array = sim.sims_to_array();

            // Total sim count
            assert_eq!(sim_array.len() as i64, trace_sim_total(entry), "sim total mismatch at {ctx}");

            // Per-family counts, state counts, and positive-stress values
            let mut by_family: IndexMap<i64, i64> = IndexMap::new();
            let mut by_family_state: IndexMap<i64, IndexMap<i64, i64>> = IndexMap::new();
            let mut by_family_stress: IndexMap<i64, Vec<i64>> = IndexMap::new();
            for sm in &sim_array {
                *by_family.entry(sm.family_code).or_insert(0) += 1;
                *by_family_state.entry(sm.family_code).or_default().entry(sm.state_code).or_insert(0) += 1;
                // Per-sim stress = accumulatedTicks / tripCount (truncated), 0 if no trips.
                let stress = if sm.trip_count > 0 { js::trunc_div(sm.accumulated_ticks, sm.trip_count) } else { 0 };
                if stress > 0 {
                    by_family_stress.entry(sm.family_code).or_default().push(stress);
                }
            }

            for (key, ref_group) in &entry.sims {
                let Some(family_code) = js::record_get(TRACE_SIM_KEY_TO_FAMILY, key.as_str()).copied() else {
                    continue;
                };
                assert_eq!(
                    by_family.get(&family_code).copied().unwrap_or(0),
                    ref_group.count,
                    "family {key} count mismatch at {ctx}"
                );
                let our_obj: BTreeMap<String, i64> = by_family_state
                    .get(&family_code)
                    .map(|m| m.iter().map(|(st, cnt)| (st.to_string(), *cnt)).collect())
                    .unwrap_or_default();
                assert_eq!(our_obj, ref_group.states, "family {key} state counts mismatch at {ctx}");

                // Stress aggregates: computed over sims with stress > 0 only,
                // to match the Python emulator's dump_tick_state.
                if let (Some(ref_avg), Some(ref_min), Some(ref_max)) =
                    (ref_group.stress_avg, ref_group.stress_min, ref_group.stress_max)
                {
                    let stresses = by_family_stress.get(&family_code).cloned().unwrap_or_default();
                    let avg = if stresses.is_empty() {
                        0
                    } else {
                        // Math.trunc(sum / length): sums of integers, exact in f64.
                        (stresses.iter().sum::<i64>() as f64 / stresses.len() as f64).trunc() as i64
                    };
                    let min = stresses.iter().copied().min().unwrap_or(0);
                    let max = stresses.iter().copied().max().unwrap_or(0);
                    assert_eq!(avg as f64, ref_avg, "family {key} stress_avg mismatch at {ctx}");
                    assert_eq!(min as f64, ref_min, "family {key} stress_min mismatch at {ctx}");
                    assert_eq!(max as f64, ref_max, "family {key} stress_max mismatch at {ctx}");
                }
            }
        }

        // ── RNG call deltas ────────────────────────────────────────────
        if let Some(rng_calls) = entry.rng_calls {
            if let (Some(prev_sim), Some(prev_trace)) = (prev_sim_calls, prev_trace_calls) {
                let sim_delta = sim.rng_call_count() - prev_sim;
                let trace_delta = rng_calls - prev_trace;
                assert_eq!(
                    sim_delta, trace_delta,
                    "rng_calls delta mismatch at {ctx}: sim={sim_delta} trace={trace_delta}"
                );
            }
            prev_sim_calls = Some(sim.rng_call_count());
            prev_trace_calls = Some(rng_calls);
        }

        // ── Carrier car positions ──────────────────────────────────────
        if let Some(ref_carriers) = entry.carriers.as_ref().filter(|c| !c.is_empty()) {
            let our_carriers = sim.carriers_to_array();
            let mut our_by_col: IndexMap<String, Vec<_>> = IndexMap::new();
            for rec in our_carriers {
                our_by_col.entry(format!("{}:{}", rec.column, rec.carrier_mode)).or_default().push(rec);
            }
            for ref_carrier in ref_carriers {
                let k = format!("{}:{}", ref_carrier.column, ref_carrier.mode);
                let cars = our_by_col.get(&k);
                assert!(cars.is_some(), "carrier {k} missing at {ctx}");
                let Some(cars) = cars else { continue };
                assert_eq!(cars.len(), ref_carrier.cars.len(), "car count for {k} at {ctx}");
                for (i, r) in ref_carrier.cars.iter().enumerate() {
                    let ours = &cars[i];
                    assert_eq!(ours.current_floor, r.currentFloor, "car {i} currentFloor at {k} {ctx}");
                    assert_eq!(ours.target_floor, r.targetFloor, "car {i} targetFloor at {k} {ctx}");
                    assert_eq!(ours.direction_flag, r.directionFlag, "car {i} directionFlag at {k} {ctx}");
                    if let Some(v) = r.stabilizeCounter {
                        assert_eq!(ours.settle_counter, v, "car {i} stabilizeCounter at {k} {ctx}");
                    }
                    if let Some(v) = r.dwellCounter {
                        assert_eq!(ours.dwell_counter, v, "car {i} dwellCounter at {k} {ctx}");
                    }
                    if let Some(v) = r.assignedCount {
                        assert_eq!(ours.assigned_count, v, "car {i} assignedCount at {k} {ctx}");
                    }
                    if let Some(v) = r.prevFloor {
                        assert_eq!(ours.prev_floor, v, "car {i} prevFloor at {k} {ctx}");
                    }
                    if let Some(v) = r.arrivalSeen {
                        assert_eq!(ours.arrival_seen, v, "car {i} arrivalSeen at {k} {ctx}");
                    }
                    if let Some(v) = r.arrivalTick {
                        assert_eq!(ours.arrival_tick, v, "car {i} arrivalTick at {k} {ctx}");
                    }
                }
            }
        }
    }
}

// FIXTURE_NAMES, one test each.
macro_rules! trace_fixtures {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            #[ignore = "fails at other groups' todo! stubs until the merge"]
            fn $test() {
                matches_full_reference_trace($name);
            }
        )*
    };
}

trace_fixtures! {
    trace_build_commercial => "commercial",
    trace_build_condo => "condo",
    trace_build_condo_office_elevator => "condo_office_elevator",
    trace_build_condo_stuck => "condo_stuck",
    trace_build_dense_hotel => "dense_hotel",
    trace_build_dense_office => "dense_office",
    trace_build_elevator => "elevator",
    trace_build_hotel => "hotel",
    trace_build_lobby_only => "lobby_only",
    trace_build_mixed => "mixed",
    trace_build_mixed_elevator => "mixed_elevator",
    trace_build_mixed_elevator_delayed => "mixed_elevator_delayed",
    trace_build_mixed_multicar => "mixed_multicar",
    trace_build_metro => "metro",
    trace_build_offices => "offices",
    trace_build_sky_office => "sky_office",
}

// Not upstream's: the parts of the harness that need no simulation.

#[test]
fn trace_ticks_map_to_total_ticks() {
    assert_eq!(trace_tick_to_total_ticks(0, 2533), 0);
    assert_eq!(trace_tick_to_total_ticks(0, 0), 67);
    assert_eq!(trace_tick_to_total_ticks(1, 2300), 67 + 2300);
    assert_eq!(trace_tick_to_total_ticks(2, 2300), 67 + 2300 + 2600);
    assert_eq!(compute_rng_state(1, 3), -1_427_733_316);
}

#[test]
fn fixtures_parse() {
    if !fixture_dir().exists() {
        eprintln!("SKIPPED: no fixtures at {} (TOWER_UPSTREAM)", fixture_dir().display());
        return;
    }
    for name in [
        "commercial", "condo", "condo_office_elevator", "condo_stuck", "dense_hotel", "dense_office", "elevator",
        "hotel", "lobby_only", "mixed", "mixed_elevator", "mixed_elevator_delayed", "mixed_multicar", "metro",
        "offices", "sky_office", "fire", "fire_accept",
    ] {
        let (spec, trace, raw) = load_fixture(name);
        assert!(!trace.is_empty() && trace.len() == raw.len(), "{name}");
        assert!(!spec.floor_extent.is_empty() || !spec.facilities.is_empty(), "{name}");
        let _ = drop_terminal_duplicate_dump(trace, &raw);
    }
}

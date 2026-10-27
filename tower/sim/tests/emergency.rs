//! BOX addition: BOX RULES and medical incidents (src/box_ext/emergency.rs,
//! docs/emergency-services.md); the street's service vehicles.

use serde_json::Value;
use tower_sim::box_ext::emergency::{GuardPhase, PersonPhase, Role, ADMISSION_INCOME, FIRE_STAIRS_TICKS_PER_FLOOR};
use tower_sim::index::{SimCommand, SimSnapshot, TowerSim};
use tower_sim::sims::population::sim_key;
use tower_sim::world::{RouteState, GROUND_Y};

fn place(sim: &mut TowerSim, x: i64, y: i64, tile_type: &str) {
    let r = sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile_type.to_string() });
    assert!(r.accepted, "place {tile_type}@{x},{y}: {}", r.reason.unwrap_or_default());
}

fn json(sim: &TowerSim) -> Value {
    serde_json::to_value(sim.save_state()).unwrap()
}

#[derive(Clone, Copy, Default)]
struct Opts {
    clinic: bool,
    security: bool,
    /// Also a sky lobby on floor 15, floors to 16, a second lift from the
    /// sky lobby to 16 and offices there: their workers change lifts.
    transfer: bool,
}

/// Lobby 100..=170, floors 1..4, offices on floors 2..4, a lift at 162 from
/// the lobby to floor 4 (or, with `transfer`, 0..2 at 162 and 2..4 at 140);
/// a medical centre and a security office on floor 1 if asked.
fn tower(o: Opts) -> TowerSim {
    let mut sim = TowerSim::create_box("emergency", "Emergency", None);
    sim.free_build = true;
    for x in 100..=170 {
        place(&mut sim, x, GROUND_Y, "lobby");
    }
    let top = if o.transfer { 16 } else { 4 };
    for floor in 1..=top {
        for x in 100..=170 {
            place(&mut sim, x, GROUND_Y - floor, if floor == 15 { "lobby" } else { "floor" });
        }
    }
    for floor in 2..=4 {
        for i in 0..3 {
            place(&mut sim, 100 + 12 * i, GROUND_Y - floor, "office");
        }
    }
    if o.clinic {
        place(&mut sim, 100, GROUND_Y - 1, "medical");
    }
    if o.security {
        place(&mut sim, 128, GROUND_Y - 1, "security");
    }
    for floor in 0..=if o.transfer { 15 } else { 4 } {
        place(&mut sim, 162, GROUND_Y - floor, "elevator");
    }
    if o.transfer {
        for i in 0..3 {
            place(&mut sim, 100 + 12 * i, GROUND_Y - 16, "office");
        }
        for floor in 15..=16 {
            place(&mut sim, 140, GROUND_Y - floor, "elevator");
        }
    }
    sim.free_build = false;
    sim.set_star_count(3);
    sim
}

/// Step until an office worker on `floor` is at work (standing still), and
/// return their index.
fn worker_at_work(sim: &mut TowerSim, floor: i64) -> i64 {
    for _ in 0..6000 {
        let snap = sim.save_state();
        if let Some(i) = snap.world.sims.iter().position(|s| {
            s.family_code == 7 && s.floor_anchor == floor && s.state_code == 0x21 && matches!(s.route, RouteState::Idle)
        }) {
            return i as i64;
        }
        sim.step();
    }
    panic!("nobody at work on floor {floor}");
}

#[derive(Default, Debug)]
struct Seen {
    ambulance: bool,
    ambulance_beacon: bool,
    medics: bool,
    staff: bool,
    guard: bool,
    medic_in_lift: bool,
    queued_as_passenger: bool,
    carriers_used: Vec<i64>,
    carried: bool,
    admitted: bool,
    fire_stairs: bool,
    done: bool,
}

fn watch(sim: &mut TowerSim, ticks: i64) -> Seen {
    let mut seen = Seen::default();
    for _ in 0..ticks {
        sim.step();
        let v = sim.box_views();
        for s in &v.service_vehicles {
            if s.style == "ambulance" {
                seen.ambulance = true;
                seen.ambulance_beacon |= s.beacon;
            }
        }
        for p in &v.people {
            match p.role {
                Role::Medic => seen.medics = true,
                Role::Staff => seen.staff = true,
                Role::Guard => seen.guard = true,
                Role::Patient => {}
            }
            if p.role != Role::Patient && p.role != Role::Guard && p.in_lift {
                seen.medic_in_lift = true;
            }
            seen.carried |= p.phase == PersonPhase::Carried;
            seen.admitted |= p.phase == PersonPhase::Admitted;
        }
        let snap = sim.save_state();
        for c in &snap.world.carriers {
            if c.pending_routes.iter().any(|r| r.sim_id.starts_with("box:er:")) {
                seen.queued_as_passenger = true;
                if !seen.carriers_used.contains(&c.carrier_id) {
                    seen.carriers_used.push(c.carrier_id);
                }
            }
        }
        for inc in &snap.world.box_ext.emergency.incidents {
            if inc.team.legs.iter().any(|l| l.kind == tower_sim::box_ext::emergency::LegKind::FireStairs) {
                seen.fire_stairs = true;
            }
        }
        if snap.world.box_ext.emergency.incidents.is_empty() && snap.world.box_ext.emergency.pending.is_empty() {
            seen.done = true;
            break;
        }
    }
    seen
}

fn find_sim(snap: &SimSnapshot, d: &tower_sim::box_ext::car_elevator::DriverRef) -> usize {
    snap.world
        .sims
        .iter()
        .position(|s| s.floor_anchor == d.floor_anchor && s.home_column == d.home_column && s.base_offset == d.base_offset && s.family_code == d.family_code)
        .unwrap()
}

#[test]
fn box_rules_are_off_unless_asked_for_and_saved_only_when_on() {
    let classic = TowerSim::create("c", "C", None);
    assert!(!classic.box_rules());
    assert!(json(&classic)["world"].get("boxExt").is_none());
    let mut boxed = TowerSim::create_box("b", "B", None);
    assert!(boxed.box_rules());
    let w = json(&boxed)["world"]["boxExt"].clone();
    assert_eq!(w["boxRules"], Value::Bool(true));
    assert!(w["rng"].as_i64().unwrap() != 0);
    let again = TowerSim::from_snapshot(&boxed.save_state());
    assert!(again.box_rules());
    boxed.set_box_rules(false);
    assert!(!boxed.box_rules());
    // Classic towers have no incidents, ever.
    let mut classic = TowerSim::create("c", "C", None);
    assert!(!classic.box_trigger_incident(false));
}

#[test]
fn without_a_medical_centre_an_ambulance_comes_and_medics_ride_the_lift() {
    let mut sim = tower(Opts::default());
    let who = worker_at_work(&mut sim, 14);
    assert!(sim.box_collapse_sim(who));
    let snap = sim.save_state();
    let inc = &snap.world.box_ext.emergency.incidents[0];
    let patient = inc.sim.clone().unwrap();
    assert!(inc.ambulance.is_some() && inc.site_floor == 14);
    let seen = watch(&mut sim, 4000);
    eprintln!("{seen:?}");
    assert!(seen.ambulance && seen.ambulance_beacon);
    assert!(seen.medics && !seen.staff && !seen.guard);
    assert!(seen.queued_as_passenger, "medics queue in the carrier like passengers");
    assert!(seen.medic_in_lift, "medics ride a car");
    assert!(seen.carried && seen.done);
    let snap = sim.save_state();
    let e = &snap.world.box_ext.emergency;
    assert!(e.held.is_empty() && e.people.is_empty());
    assert_eq!(e.collapses, 1);
    assert!(e.response_ticks > 0);
    let s = &snap.world.sims[find_sim(&snap, &patient)];
    assert!(s.state_code == 0x26 || s.state_code == 0x20, "home for the day: state {:#x}", s.state_code);
}

#[test]
fn with_a_medical_centre_its_staff_respond_admit_and_earn() {
    let mut sim = tower(Opts { clinic: true, ..Opts::default() });
    let who = worker_at_work(&mut sim, 13);
    assert!(sim.box_collapse_sim(who));
    let snap = sim.save_state();
    let inc = &snap.world.box_ext.emergency.incidents[0];
    assert!(inc.clinic.is_some() && inc.ambulance.is_none());
    assert!(snap.world.medical_service_slots[inc.slot as usize].active, "a medical slot is held");
    let seen = watch(&mut sim, 4000);
    eprintln!("{seen:?}");
    assert!(seen.staff && !seen.medics && !seen.ambulance);
    assert!(seen.carried && seen.admitted && seen.done);
    let snap = sim.save_state();
    assert_eq!(snap.world.box_ext.emergency.admissions, 1, "a collapse treated in the centre is paid for");
    assert!(snap.world.medical_service_slots.iter().all(|s| !s.sim_id.starts_with("box:")), "slot freed");
}

#[test]
fn an_ambulance_brings_a_patient_in_for_admission_and_it_pays() {
    let mut sim = tower(Opts { clinic: true, ..Opts::default() });
    for _ in 0..300 {
        sim.step();
    }
    assert!(sim.box_trigger_incident(true));
    let mut paid = false;
    let mut seen = Seen::default();
    for _ in 0..4000 {
        let c0 = sim.cash();
        let a0 = sim.save_state().world.box_ext.emergency.admissions;
        sim.step();
        let snap = sim.save_state();
        if snap.world.box_ext.emergency.admissions > a0 {
            assert!(sim.cash() >= c0 + ADMISSION_INCOME - 50_000);
            paid = true;
        }
        let v = sim.box_views();
        seen.ambulance |= v.service_vehicles.iter().any(|s| s.style == "ambulance");
        seen.admitted |= v.people.iter().any(|p| p.phase == PersonPhase::Admitted);
        seen.medic_in_lift |= v.people.iter().any(|p| p.role == Role::Medic && p.in_lift);
        if snap.world.box_ext.emergency.incidents.is_empty() {
            seen.done = true;
            break;
        }
    }
    assert!(seen.ambulance && seen.admitted && seen.medic_in_lift && paid && seen.done, "{seen:?}");
}

#[test]
fn medics_change_lifts_to_reach_the_patient() {
    let mut sim = tower(Opts { transfer: true, ..Opts::default() });
    let who = worker_at_work(&mut sim, 26);
    assert!(sim.box_collapse_sim(who));
    let seen = watch(&mut sim, 5000);
    eprintln!("{seen:?}");
    assert!(seen.done && seen.carried && !seen.fire_stairs);
    assert_eq!(seen.carriers_used.len(), 2, "both lifts, changing at the sky lobby");
}

#[test]
fn an_unreachable_patient_is_still_reached_by_the_fire_stairs_slowly() {
    let mut sim = tower(Opts { security: true, ..Opts::default() });
    let who = worker_at_work(&mut sim, 14);
    // The lift goes: the worker is stranded at work.
    for floor in 0..=4 {
        let r = sim.submit_command(&SimCommand::RemoveTile { x: 162, y: GROUND_Y - floor });
        assert!(r.accepted);
    }
    assert!(sim.box_collapse_sim(who));
    let seen = watch(&mut sim, 6000);
    assert!(seen.done && seen.fire_stairs, "{seen:?}");
    assert!(seen.guard, "guards know the building: one came by the fire stairs");
    let e = &sim.save_state().world.box_ext.emergency;
    assert!(e.response_ticks >= 4 * FIRE_STAIRS_TICKS_PER_FLOOR, "slow: {}", e.response_ticks);
}

#[test]
fn a_security_guard_comes_first_and_halves_the_stress_from_then_on() {
    let mut sim = tower(Opts { security: true, ..Opts::default() });
    let who = worker_at_work(&mut sim, 14);
    assert!(sim.box_collapse_sim(who));
    let mut arrived = -1;
    let mut seen_guard = false;
    let (mut response, mut stress, mut start) = (-1, 0, 0);
    for _ in 0..4000 {
        sim.step();
        seen_guard |= sim.box_views().people.iter().any(|p| p.role == Role::Guard && p.family == "security");
        let snap = sim.save_state();
        let Some(inc) = snap.world.box_ext.emergency.incidents.first() else { break };
        if let Some(g) = &inc.guard {
            if g.phase == GuardPhase::WithPatient {
                arrived = g.arrived_tick;
            }
        }
        if inc.response >= 0 && response < 0 {
            (response, stress, start) = (inc.response, inc.stress, inc.start_tick);
        }
    }
    assert!(seen_guard && arrived > 0, "the guard reached the patient first");
    assert!(response > 0);
    let g = arrived - start;
    assert_eq!(stress, g + (response - g) / 2);
    assert!(stress < response);
    assert_eq!(sim.save_state().world.box_ext.emergency.guards, 1);
    assert!(sim.box_views().people.is_empty(), "the guard went back");

    // Without a security office the whole wait counts.
    let mut sim = tower(Opts::default());
    let who = worker_at_work(&mut sim, 14);
    assert!(sim.box_collapse_sim(who));
    for _ in 0..4000 {
        sim.step();
        let snap = sim.save_state();
        let Some(inc) = snap.world.box_ext.emergency.incidents.first() else { break };
        if inc.response >= 0 {
            assert_eq!(inc.stress, inc.response);
            return;
        }
    }
    panic!("nobody came");
}

#[test]
fn anyone_in_the_tower_can_collapse_riders_at_their_stop() {
    let mut sim = tower(Opts::default());
    // A rider: someone aboard a car.
    let mut rider = None;
    for _ in 0..6000 {
        sim.step();
        let snap = sim.save_state();
        for c in &snap.world.carriers {
            if let Some(r) = c.pending_routes.iter().find(|r| r.boarded && !r.sim_id.starts_with("box:")) {
                let i = snap.world.sims.iter().position(|s| sim_key(s) == r.sim_id);
                if let Some(i) = i {
                    rider = Some((i as i64, r.destination_floor));
                }
            }
        }
        if rider.is_some() {
            break;
        }
    }
    let (who, stop) = rider.expect("nobody rode");
    assert!(sim.box_collapse_sim(who));
    assert_eq!(sim.save_state().world.box_ext.emergency.pending.len(), 1, "due to collapse at the stop");
    for _ in 0..400 {
        sim.step();
        if !sim.save_state().world.box_ext.emergency.incidents.is_empty() {
            break;
        }
    }
    let snap = sim.save_state();
    let inc = &snap.world.box_ext.emergency.incidents[0];
    assert_eq!(inc.site_floor, stop, "collapsed at their stop");
    assert!(watch(&mut sim, 5000).done);
}

#[test]
fn incidents_are_deterministic_and_survive_save_and_load() {
    let mut a = tower(Opts { security: true, ..Opts::default() });
    let mut b = tower(Opts { security: true, ..Opts::default() });
    let who = worker_at_work(&mut a, 14);
    assert_eq!(who, worker_at_work(&mut b, 14));
    assert!(a.box_collapse_sim(who));
    assert!(b.box_collapse_sim(who));
    for _ in 0..60 {
        a.step();
        b.step();
    }
    assert_eq!(json(&a), json(&b));
    let saved = a.save_state();
    assert!(!saved.world.box_ext.emergency.incidents.is_empty());
    let mut c = TowerSim::from_snapshot(&saved);
    assert_eq!(json(&c), serde_json::to_value(&saved).unwrap());
    for _ in 0..2000 {
        a.step();
        c.step();
    }
    assert_eq!(json(&a), json(&c));
}

#[test]
fn incidents_happen_by_themselves_to_all_sorts_of_people() {
    let mut sim = tower(Opts { clinic: true, ..Opts::default() });
    let mut n = 0;
    for _ in 0..40 * 2600 {
        sim.step();
        let e = &sim.save_state().world.box_ext.emergency;
        n = e.collapses;
        if n > 0 {
            break;
        }
    }
    assert!(n > 0, "no collapse in 40 days");
}

#[test]
fn street_services_drive_in_park_and_leave_with_beacons() {
    use tower_sim::box_ext::street;
    let mut w = tower_sim::snapshot::create_initial_snapshot("t", "t", 2_000_000, None).world;
    w.event_state.game_state_flags = 1 << 3;
    w.event_state.fire_floor = 12;
    w.event_state.fire_tile = 140;
    street::tick(&mut w);
    let v = street::service_vehicle_views(&w);
    assert_eq!(v.iter().map(|s| s.style).collect::<Vec<_>>(), ["fire-pumper", "fire-ladder"]);
    assert!(v.iter().all(|s| s.moving && s.beacon));
    let x0 = v[0].x;
    for _ in 0..200 {
        street::tick(&mut w);
    }
    let v = street::service_vehicle_views(&w);
    assert!(v.iter().all(|s| !s.moving));
    assert_ne!(v[0].x, x0);
    assert_eq!(v[1].style, "fire-ladder-deployed");
    assert_eq!(v[1].ladder_to, Some(12));
    // High up: the ladder stays down.
    w.event_state.fire_floor = 40;
    assert_eq!(street::service_vehicle_views(&w)[1].style, "fire-ladder");
    // The fire is out: they drive away, beacons on, and are gone.
    w.event_state.game_state_flags = 0;
    street::tick(&mut w);
    assert!(street::service_vehicle_views(&w).iter().all(|s| s.moving && s.beacon));
    for _ in 0..300 {
        street::tick(&mut w);
    }
    assert!(street::service_vehicle_views(&w).is_empty());
    // A bomb search: two police cars; nothing of this is saved.
    w.event_state.game_state_flags = 1;
    w.event_state.bomb_tile = 130;
    street::tick(&mut w);
    assert_eq!(street::service_vehicle_views(&w).iter().filter(|s| s.style == "police").count(), 2);
    assert!(serde_json::to_value(&w).unwrap().get("boxExt").is_none());
}

#[test]
fn people_collapse_wherever_they_are_in_proportion() {
    let mut sim = tower(Opts::default());
    sim.free_build = true;
    place(&mut sim, 140, GROUND_Y - 1, "fastFood");
    place(&mut sim, 140, GROUND_Y - 3, "condo");
    place(&mut sim, 100, GROUND_Y - 1, "hotelSingle");
    sim.free_build = false;
    let mut families: Vec<i64> = Vec::new();
    let mut seen_ids: Vec<i64> = Vec::new();
    for t in 0..4 * 2600 {
        sim.step();
        if t % 50 == 0 {
            sim.box_trigger_incident(false);
        }
        let snap = sim.save_state();
        for inc in &snap.world.box_ext.emergency.incidents {
            if !seen_ids.contains(&inc.id) {
                seen_ids.push(inc.id);
                if let Some(d) = &inc.sim {
                    if !families.contains(&d.family_code) {
                        families.push(d.family_code);
                    }
                }
            }
        }
    }
    families.sort();
    eprintln!("families {families:?} over {} incidents", seen_ids.len());
    assert!(families.len() >= 3, "{families:?}");
}

#[test]
fn crews_change_lifts_wherever_both_stop() {
    let mut sim = TowerSim::create_box("t", "T", None);
    sim.free_build = true;
    for x in 100..=170 {
        place(&mut sim, x, GROUND_Y, "lobby");
    }
    for floor in 1..=5 {
        for x in 100..=170 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    for floor in 0..=3 {
        place(&mut sim, 162, GROUND_Y - floor, "elevator");
    }
    for floor in 2..=5 {
        place(&mut sim, 130, GROUND_Y - floor, "elevatorService");
    }
    let w = sim.save_state().world;
    let (legs, _, complete) = tower_sim::box_ext::emergency::plan(&w, 10, 1600, 15);
    assert!(complete);
    let lifts: Vec<i64> = legs.iter().filter(|l| l.kind == tower_sim::box_ext::emergency::LegKind::Lift).map(|l| l.to_floor).collect();
    assert_eq!(lifts.len(), 2, "{legs:?}");
    assert!(lifts[0] == 12 || lifts[0] == 13, "changed at a floor both serve, not a lobby: {legs:?}");
    assert_eq!(lifts[1], 15);
}

/// The tower with a fire lit on floor 11 (the first floor up) as the game
/// lights one, BOX RULES on or off.
fn fire_tower(box_rules: bool) -> TowerSim {
    let mut sim = tower(Opts::default());
    for _ in 0..300 {
        sim.step();
    }
    let mut snap = sim.save_state();
    let es = &mut snap.world.event_state;
    es.fire_floor = 11;
    es.fire_tile = 170 - 0x20;
    es.game_state_flags |= 8;
    es.fire_start_tick = snap.time.day_tick;
    es.fire_left_pos.fill(0xffff);
    es.fire_right_pos.fill(0xffff);
    es.fire_left_pos[11] = es.fire_tile;
    es.fire_right_pos[11] = es.fire_tile;
    es.rescue_countdown = 0;
    es.helicopter_extinguish_pos = 0;
    let mut sim = TowerSim::from_snapshot(&snap);
    sim.set_box_rules(box_rules);
    sim
}

fn ticks_until_out(sim: &mut TowerSim) -> (i64, bool) {
    let mut ladder_up = false;
    for t in 0..3000 {
        sim.step();
        ladder_up |= sim.box_views().service_vehicles.iter().any(|v| v.style == "fire-ladder-deployed");
        if sim.save_state().world.event_state.game_state_flags & 8 == 0 {
            return (t, ladder_up);
        }
    }
    panic!("the fire never went out");
}

#[test]
fn fire_engines_put_a_low_fire_out_sooner_under_box_rules() {
    let (classic, _) = ticks_until_out(&mut fire_tower(false));
    let (boxed, ladder) = ticks_until_out(&mut fire_tower(true));
    eprintln!("fire out after {boxed} ticks with engines, {classic} without");
    assert!(ladder, "the ladder was raised");
    assert!(boxed * 3 < classic, "engines: {boxed}, classic: {classic}");
}

#[test]
fn under_box_rules_the_day_carries_on_when_a_fire_is_put_out() {
    for box_rules in [false, true] {
        let mut sim = fire_tower(box_rules);
        let start = sim.save_state().time.day_tick;
        let (t, _) = ticks_until_out(&mut sim);
        let now = sim.save_state().time.day_tick;
        if box_rules {
            assert_eq!(now, start + t + 1, "no jump");
            assert!(now < 1500);
        } else {
            assert!(now >= 1500, "classic: upstream's jump (or the cleanup at 2000)");
        }
    }
}

#[test]
fn police_find_the_bomb_under_box_rules() {
    let run = |box_rules: bool| {
        let mut sim = tower(Opts::default());
        for _ in 0..200 {
            sim.step();
        }
        let mut snap = sim.save_state();
        let es = &mut snap.world.event_state;
        es.bomb_floor = 13;
        es.bomb_tile = 110;
        es.game_state_flags |= 1;
        es.bomb_deadline = 1200;
        es.bomb_search_lower_bound = 12;
        es.bomb_search_upper_bound = 13;
        es.bomb_search_current_floor = -1;
        es.bomb_search_scan_tile = -1;
        let mut sim = TowerSim::from_snapshot(&snap);
        sim.set_box_rules(box_rules);
        for _ in 0..1200 {
            sim.step();
            let f = sim.save_state().world.event_state.game_state_flags;
            if f & 0x20 != 0 {
                return true; // found
            }
            if f & 1 == 0 {
                return false;
            }
        }
        false
    };
    assert!(!run(false), "classic: no security office, nobody searches");
    assert!(run(true), "BOX RULES: the police found it");
}

#[test]
fn under_box_rules_the_street_is_saved_and_continues_after_loading() {
    let mut a = fire_tower(true);
    for _ in 0..60 {
        a.step();
    }
    let saved = a.save_state();
    let street = serde_json::to_value(&saved).unwrap()["world"]["boxExt"]["street"].clone();
    assert!(street["vehicles"].as_array().is_some_and(|v| v.len() == 2), "{street}");
    let mut c = TowerSim::from_snapshot(&saved);
    for _ in 0..400 {
        a.step();
        c.step();
    }
    assert_eq!(json(&a), json(&c));
    // Classic: the picture only, nothing saved.
    let mut k = fire_tower(false);
    for _ in 0..60 {
        k.step();
    }
    assert!(!k.box_views().service_vehicles.is_empty());
    assert!(json(&k)["world"]["boxExt"].get("street").is_none());
}

#[test]
fn the_fire_stairs_are_the_fire_escape() {
    use tower_sim::box_ext::emergency::{plan, LegKind};
    use tower_sim::box_ext::escapes;
    // a lobby and four floors over it, 100..=150, no lift, stairs or
    // escalator: from the lobby to floor 13 there is nothing but the fire
    // escapes at the floors' ends (98-99 on the left, 151-152 on the right)
    let mut sim = TowerSim::create_box("escape", "Escape", None);
    sim.free_build = true;
    for x in 100..=150 {
        place(&mut sim, x, GROUND_Y, "lobby");
        for floor in 1..=4 {
            place(&mut sim, x, GROUND_Y - floor, "floor");
        }
    }
    let (legs, _, complete) = plan(sim.world(), 10, 140 * 16, 13);
    assert!(!complete);
    let leg = legs.last().unwrap();
    assert_eq!(leg.kind, LegKind::FireStairs);
    assert_eq!(leg.to_floor, 13);
    // the nearer end's escape (the right: 140 is nearer 151 than 99), its landing
    let e = escapes::for_trip(sim.world(), 140.0, GROUND_Y, GROUND_Y - 3).expect("an escape for the trip");
    assert_eq!((e.side, e.x), (1, 151));
    assert_eq!(leg.at_x, (e.landing() * 16.0) as i64);
    // and from the left half, the left one
    let (legs, _, _) = plan(sim.world(), 10, 105 * 16, 13);
    assert_eq!(legs.last().unwrap().at_x, (99.0 * 16.0) as i64);
}

/// The stranded worker's tower (the fire stairs' test): the guard and the
/// medics are seen climbing the fire escape; and, with ESCAPE_TOWER=path,
/// the tower is written just after the collapse for the play tests
#[test]
fn the_crews_are_seen_on_the_fire_escape() {
    use tower_sim::box_ext::emergency::people_views;
    let mut sim = tower(Opts { security: true, ..Opts::default() });
    let who = worker_at_work(&mut sim, 14);
    for floor in 0..=4 {
        assert!(sim.submit_command(&SimCommand::RemoveTile { x: 162, y: GROUND_Y - floor }).accepted);
    }
    assert!(sim.box_collapse_sim(who));
    if let Ok(out) = std::env::var("ESCAPE_TOWER") {
        std::fs::write(out, serde_json::to_vec(&sim.save_state()).unwrap()).unwrap();
    }
    let (mut climbing, mut rows) = (false, std::collections::BTreeSet::new());
    for _ in 0..6000 {
        sim.step();
        for v in people_views(sim.world()) {
            if let Some(c) = v.climb {
                climbing = true;
                assert!(c.escape.top <= c.from_row.min(c.to_row) && c.from_row.max(c.to_row) <= c.escape.bottom);
                rows.insert((c.progress * 10.0) as i64);
            }
        }
    }
    assert!(climbing, "nobody was seen on the fire escape");
    assert!(rows.len() > 3, "the climb goes on: {rows:?}");
}

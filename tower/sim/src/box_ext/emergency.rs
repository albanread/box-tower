//! BOX addition (BOX RULES only): medical incidents (docs/emergency-services.md).
//!
//! Now and then somebody in the tower collapses where they are: at home, at
//! work, in a shop, waiting for a lift, or (riding one) on arrival at their
//! stop.  A security office sends a guard first, if one can get there.  With
//! no medical centre an ambulance drives along the street to the lobby and
//! two medics go up to the patient by the tower's own lifts, stairs and
//! escalators, as passengers (they stand in the carriers' queues and take
//! room in the cars), and bring the patient back down and away.  With a
//! medical centre its staff go instead and admit the patient there.  With a
//! medical centre, ambulances also bring patients in from outside.
//!
//! All of it is deterministic: the choices come from BOX RULES' own random
//! generator (`box_ext::box_random`), never the world's LCG, and the state
//! is saved with the tower.
//!
//! Hooks (each `// BOX: emergency`):
//! - tick/service_idle_tasks.rs: `tick`, after the carrier tick;
//! - queue/dispatch_arrivals.rs: `collapse_on_arrival` (a rider due to
//!   collapse does so at their stop instead of going on) and
//!   `on_carrier_arrival` (a car delivers a rider that is no sim);
//! - sims/mod.rs advance_sim_refresh_stride: `is_held` (a patient does
//!   nothing until the incident lets them go).

use serde::{Deserialize, Serialize};

use crate::box_ext::car_elevator::DriverRef;
use crate::box_ext::{box_random, box_rules, BoxExtState};
use crate::carriers::slot::{floor_to_slot, is_express_stop_floor};
use crate::ledger::LedgerState;
use crate::queue::cancel::cancel_runtime_route_request;
use crate::queue::enqueue::enqueue_request_into_route_queue;
use crate::resources::{
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN, FAMILY_MEDICAL,
    FAMILY_OFFICE, FAMILY_RESTAURANT, FAMILY_RETAIL, FAMILY_SECURITY,
};
use crate::sims::population::sim_key;
use crate::stress::add_delay::add_delay_to_current_sim;
use crate::time::TimeState;
use crate::world::{
    floor_to_y, y_to_floor, RouteState, SidecarRecord, SimNotification, SimNotificationKind, WorldState, GRID_HEIGHT, GROUND_Y,
};

// ─── Rules ────────────────────────────────────────────────────────────────────

/// Sixteenths of a cell (positions), as the car elevator.
pub const SUB: i64 = 16;
/// A collapse happens on a tick with chance population / COLLAPSE_ODDS:
/// about population / 500 a day (a tower of 1,000 sees two a day).
pub const COLLAPSE_ODDS: i64 = 2600 * 500;
/// An outside admission is tried on a tick with chance (medical centres) /
/// ADMISSION_ODDS: about one a day for each medical centre.
pub const ADMISSION_ODDS: i64 = 2600;
/// At most this many incidents at once (riders due to collapse included).
pub const MAX_INCIDENTS: usize = 3;
/// Paid to the tower for each admission to a medical centre.
pub const ADMISSION_INCOME: i64 = 3_000;
/// Ticks an admitted patient stays.
pub const ADMIT_TICKS: i64 = 800;
/// Ticks the medics spend with the patient before moving them.
pub const TREAT_TICKS: i64 = 40;
/// Walking speed, sixteenths of a cell a tick (half a cell).
pub const WALK_SPEED: i64 = 8;
/// The ambulance: street speed and length (art/src/vehicles: 7 cells).
pub const AMBULANCE_SPEED: i64 = 32;
pub const AMBULANCE_LENGTH: i64 = 7;
/// Ticks a floor: up or down a flight of stairs, an escalator, and the fire
/// stairs (the last resort where nothing the player built reaches).
pub const STAIRS_TICKS_PER_FLOOR: i64 = 30;
pub const ESCALATOR_TICKS_PER_FLOOR: i64 = 15;
pub const FIRE_STAIRS_TICKS_PER_FLOOR: i64 = 120;
/// The route planner's estimate of a lift leg's wait and ride.
const LIFT_LEG_COST: i64 = 120;
/// The street's floor.
const STREET_FLOOR: i64 = GRID_HEIGHT - 1 - GROUND_Y;
/// Idle states that mean the person is not in the tower (not yet come,
/// room empty, gone home); condo residents are home whenever they are on
/// their own floor.
const GONE_STATES: &[i64] = &[0x20, 0x24, 0x25, 0x26, 0x27];
const LOBBY_FLOOR: i64 = 10;
pub const COLLAPSE_MESSAGE: &str = "Someone has collapsed: medics are on their way";

// ─── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmergencyState {
    #[serde(default)]
    pub next_id: i64,
    #[serde(default)]
    pub incidents: Vec<Incident>,
    #[serde(default)]
    pub people: Vec<Responder>,
    /// Sims collapsed or in care: their refresh is held.
    #[serde(default)]
    pub held: Vec<DriverRef>,
    /// Riders due to collapse when their car reaches their stop.
    #[serde(default)]
    pub pending: Vec<DriverRef>,
    /// Tallies: collapses, ticks from collapse to the medics' arrival,
    /// admissions, guards sent.
    #[serde(default)]
    pub collapses: i64,
    #[serde(default)]
    pub response_ticks: i64,
    #[serde(default)]
    pub admissions: i64,
    #[serde(default)]
    pub guards: i64,
}

impl EmergencyState {
    pub fn is_empty(&self) -> bool {
        *self == EmergencyState::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Medic,
    Staff,
    Guard,
    Patient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PersonPhase {
    /// Inside the ambulance (not drawn).
    InAmbulance,
    Walking,
    Standing,
    /// At a lift, queued or aboard (the views say which).
    Lift,
    /// On stairs or an escalator (or the fire stairs).
    Stairs,
    Treating,
    /// The patient, collapsed where they were.
    Lying,
    /// The patient on the stretcher, with the medics.
    Carried,
    /// The patient in the medical centre.
    Admitted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Responder {
    pub id: i64,
    pub incident: i64,
    pub role: Role,
    /// Sixteenths of a cell.
    pub x: i64,
    /// Floor index (whole floors; riding is read from the carrier).
    pub floor: i64,
    pub facing: i64,
    pub phase: PersonPhase,
    /// The carrier route id while at a lift ("box:er:ID:N"), else "".
    pub route: String,
    pub rides: i64,
    pub arrived: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IncidentKind {
    Collapse,
    Admission,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clinic {
    pub key: String,
    pub sidecar: i64,
    pub floor: i64,
    /// Sixteenths of a cell: where patients are taken.
    pub x: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AmbulancePhase {
    Arriving,
    Parked,
    Leaving,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ambulance {
    /// Left end, sixteenths of a cell, on the street.
    pub x: i64,
    /// The street end it comes from and goes to (-1 left, 1 right).
    pub side: i64,
    pub phase: AmbulancePhase,
    pub beacon: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegKind {
    Lift,
    Stairs,
    Escalator,
    /// No lift, stairs or escalator reaches: the building's fire stairs.
    FireStairs,
}

/// One leg of a planned route: go to `at_x` on the current floor, then by
/// `kind` (lift `carrier`) to `to_floor`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leg {
    pub kind: LegKind,
    pub carrier: i64,
    pub at_x: i64,
    pub to_floor: i64,
}

/// People who move together.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub members: Vec<i64>,
    pub floor: i64,
    pub x: i64,
    pub legs: Vec<Leg>,
    pub leg: i64,
    /// 0 plan, 1 walk to the leg, 2 on the leg, 3 walk to the goal.
    pub stage: i64,
    pub counter: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuardPhase {
    ToPatient,
    WithPatient,
    Returning,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guard {
    pub team: Team,
    pub phase: GuardPhase,
    /// time.totalTicks when the guard reached the patient (-1: not yet).
    pub arrived_tick: i64,
    pub office_floor: i64,
    pub office_x: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "step")]
pub enum Step {
    AmbulanceIn,
    /// The team goes to (floor, x).
    Travel { floor: i64, x: i64 },
    Treat,
    PickUp,
    Admit,
    /// Clinic staff go back into the centre.
    StaffHome,
    AmbulanceOut,
    /// The admitted patient leaves when their time is up.
    Discharge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: i64,
    pub kind: IncidentKind,
    /// Person id of the patient.
    pub patient: i64,
    /// The sim who collapsed (none for an outside patient), and their state then.
    pub sim: Option<DriverRef>,
    pub sim_state: i64,
    pub start_tick: i64,
    /// Ticks from the collapse to the responders' arrival (-1 until then).
    pub response: i64,
    /// The stress the wait laid on the patient's unit (halved from a
    /// guard's arrival on).
    #[serde(default)]
    pub stress: i64,
    pub clinic: Option<Clinic>,
    /// The medical service slot held (-1: none).
    pub slot: i64,
    pub site_floor: i64,
    pub site_x: i64,
    /// The lobby entrance on the street, sixteenths of a cell.
    pub entrance_x: i64,
    pub steps: Vec<Step>,
    pub pc: i64,
    pub counter: i64,
    pub team: Team,
    pub guard: Option<Guard>,
    pub ambulance: Option<Ambulance>,
    pub admitted_until: i64,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn key_xy(key: &str) -> (i64, i64) {
    let mut it = key.split(',').map(|p| p.parse::<i64>().expect("an \"x,y\" key"));
    (it.next().unwrap(), it.next().unwrap())
}

fn move_toward(pos: i64, target: i64, speed: i64) -> i64 {
    if pos < target {
        (pos + speed).min(target)
    } else {
        (pos - speed).max(target)
    }
}

fn serves(world: &WorldState, ci: usize, floor: i64) -> bool {
    let c = &world.carriers[ci];
    floor_to_slot(c, floor) >= 0
        && (c.carrier_mode != 0 || is_express_stop_floor(floor, world.lobby_mode))
        && c.cars.iter().any(|car| car.active)
}

/// Where riders wait at a shaft: one cell in from its left edge.
fn lift_x(column: i64) -> i64 {
    (column + 1) * SUB
}

/// The middle of the ground floor's lobby; failing that, of anything built
/// on the ground floor; failing that, `fallback`.
fn street_entrance(world: &WorldState, fallback: i64) -> i64 {
    let span = |lobby_only: bool| {
        let (mut lo, mut hi) = (i64::MAX, i64::MIN);
        for (key, t) in world.cells.iter() {
            if lobby_only && t != "lobby" {
                continue;
            }
            let (x, y) = key_xy(key);
            if y == GROUND_Y {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        (lo <= hi).then(|| (lo + hi) * SUB / 2)
    };
    span(true).or_else(|| span(false)).unwrap_or(fallback)
}

/// The walking ticks between two positions.
fn walk_ticks(a: i64, b: i64) -> i64 {
    (a - b).abs() / WALK_SPEED
}

/// Stairs and escalators: (column of the middle, lower floor, ticks a floor).
fn links(world: &WorldState) -> Vec<(i64, i64, i64)> {
    let mut out = Vec::new();
    for (key, t) in world.overlays.iter() {
        let tpf = match t.as_str() {
            "stairs" => STAIRS_TICKS_PER_FLOOR,
            "escalator" => ESCALATOR_TICKS_PER_FLOOR,
            _ => continue,
        };
        let (x, y) = key_xy(key);
        out.push(((x + 4) * SUB, y_to_floor(y), tpf));
    }
    out
}

/// The route from (`from`, `fx`) to `to` for crews who know the building:
/// lifts of every kind (service lifts too), changing from one to another on
/// any floor where both stop, and stairs and escalators; the fewest ticks by
/// the planner's estimate.  Where nothing
/// reaches `to`, the route to the reachable floor nearest it, then the fire
/// stairs the rest of the way.  Returns the legs, the estimate, and whether
/// it is all the player's own transport.
pub fn plan(world: &WorldState, from: i64, fx: i64, to: i64) -> (Vec<Leg>, i64, bool) {
    // Node f*2 + 1: on floor f, having come by lift; f*2: on foot (the two
    // are kept apart so that the legs read back cleanly).
    let n = GRID_HEIGHT as usize * 2;
    let mut dist = vec![i64::MAX; n];
    let mut pos = vec![0i64; n];
    let mut prev: Vec<Option<(usize, Leg)>> = vec![None; n];
    let mut done = vec![false; n];
    let links = links(world);
    let start = from as usize * 2;
    dist[start] = 0;
    pos[start] = fx;
    loop {
        let mut u = None;
        for v in 0..n {
            if !done[v] && dist[v] != i64::MAX && u.is_none_or(|w: usize| dist[v] < dist[w]) {
                u = Some(v);
            }
        }
        let Some(u) = u else { break };
        done[u] = true;
        let (fu, xu) = ((u / 2) as i64, pos[u]);
        let mut relax = |v: usize, cost: i64, leg: Leg| {
            if !done[v] && dist[u] + cost < dist[v] {
                dist[v] = dist[u] + cost;
                pos[v] = leg.at_x;
                prev[v] = Some((u, leg));
            }
        };
        {
            for ci in 0..world.carriers.len() {
                if !serves(world, ci, fu) {
                    continue;
                }
                let c = &world.carriers[ci];
                let lx = lift_x(c.column);
                for g in c.bottom_served_floor..=c.top_served_floor {
                    if g != fu && serves(world, ci, g) {
                        let cost = walk_ticks(xu, lx) + LIFT_LEG_COST + (g - fu).abs() * 4;
                        let leg = Leg { kind: LegKind::Lift, carrier: c.carrier_id, at_x: lx, to_floor: g };
                        relax(g as usize * 2 + 1, cost, leg);
                    }
                }
            }
        }
        for &(sx, lower, tpf) in &links {
            let g = if fu == lower {
                lower + 1
            } else if fu == lower + 1 {
                lower
            } else {
                continue;
            };
            let kind = if tpf == ESCALATOR_TICKS_PER_FLOOR { LegKind::Escalator } else { LegKind::Stairs };
            relax(g as usize * 2, walk_ticks(xu, sx) + tpf, Leg { kind, carrier: -1, at_x: sx, to_floor: g });
        }
    }
    let best_at = |f: usize| if dist[f * 2] <= dist[f * 2 + 1] { f * 2 } else { f * 2 + 1 };
    let goal = best_at(to as usize);
    let (end, complete) = if dist[goal] != i64::MAX {
        (goal, true)
    } else {
        // The reachable floor nearest the goal (the cheaper on a tie).
        let mut best = start;
        for f in 0..GRID_HEIGHT as usize {
            let v = best_at(f);
            if dist[v] == i64::MAX {
                continue;
            }
            let (df, db) = ((f as i64 - to).abs(), ((best / 2) as i64 - to).abs());
            if df < db || (df == db && dist[v] < dist[best]) {
                best = v;
            }
        }
        (best, false)
    };
    let mut legs = Vec::new();
    let mut v = end;
    while let Some((p, leg)) = prev[v].clone() {
        legs.push(leg);
        v = p;
    }
    legs.reverse();
    let mut cost = dist[end];
    if !complete {
        // the fire stairs: the fire escape beside this floor whose stair
        // reaches the goal's floor (escapes.rs), walked to and climbed; where
        // none does, from where the team stands, as before
        let from_floor = (end / 2) as i64;
        let at_x = crate::box_ext::escapes::for_trip(world, pos[end] as f64 / SUB as f64, floor_to_y(from_floor), floor_to_y(to))
            .map(|e| (e.landing() * SUB as f64) as i64)
            .unwrap_or(pos[end]);
        legs.push(Leg { kind: LegKind::FireStairs, carrier: -1, at_x, to_floor: to });
        cost += ((end / 2) as i64 - to).abs() * FIRE_STAIRS_TICKS_PER_FLOOR;
    }
    (legs, cost, complete)
}

/// The medical centres (sidecar alive), as sims/medical.rs collects them.
fn clinics(world: &WorldState) -> Vec<Clinic> {
    let mut out = Vec::new();
    for (key, o) in world.placed_objects.iter() {
        if o.object_type_code != FAMILY_MEDICAL || o.linked_record_index < 0 {
            continue;
        }
        let Some(SidecarRecord::MedicalCenter(m)) = world.sidecars.get(o.linked_record_index as usize) else {
            continue;
        };
        if m.owner_subtype_index == 0xff {
            continue;
        }
        let (_, y) = key_xy(key);
        out.push(Clinic {
            key: key.to_string(),
            sidecar: o.linked_record_index,
            floor: y_to_floor(y),
            x: (o.left_tile_index + o.right_tile_index + 1) * SUB / 2,
        });
    }
    out
}

/// sims/medical.rs's choice: a centre in the patient's zone of 15 floors if
/// there is one, else any (here with BOX RULES' generator); the centres the
/// staff can reach by the player's own transport first.
fn pick_clinic(world: &mut WorldState, from: i64, fx: i64) -> Option<Clinic> {
    let all = clinics(world);
    if all.is_empty() {
        return None;
    }
    let reach: Vec<Clinic> = all.iter().filter(|c| plan(world, c.floor, c.x, from).2).cloned().collect();
    let pool = if reach.is_empty() { all } else { reach };
    let _ = fx;
    let zone = crate::js::floor_div(from - 9, 15);
    let in_zone: Vec<Clinic> =
        pool.iter().filter(|c| crate::js::floor_div(c.floor - 9, 15) == zone).cloned().collect();
    let bucket = if in_zone.is_empty() { pool } else { in_zone };
    let i = box_random(world) % bucket.len() as i64;
    Some(bucket[i as usize].clone())
}

fn clinic_alive(world: &WorldState, c: &Clinic) -> bool {
    matches!(world.sidecars.get(c.sidecar as usize), Some(SidecarRecord::MedicalCenter(m)) if m.owner_subtype_index != 0xff)
}

/// Take one of the game's medical service slots for this patient (as an
/// office worker's medical trip does): -1 if all ten are in use.
fn alloc_slot(world: &mut WorldState, c: &Clinic, id: &str, from: i64) -> i64 {
    let Some(i) = world.medical_service_slots.iter().position(|s| !s.active) else {
        return -1;
    };
    let s = &mut world.medical_service_slots[i];
    s.active = true;
    s.sim_id = id.to_string();
    s.source_floor = from;
    s.target_sidecar_index = c.sidecar;
    s.retry_counter = 0;
    if let Some(SidecarRecord::MedicalCenter(m)) = world.sidecars.get_mut(c.sidecar as usize) {
        m.pending_visitors_count += 1;
    }
    i as i64
}

fn free_slot(world: &mut WorldState, slot: i64, id: &str) {
    if slot < 0 {
        return;
    }
    let Some(s) = world.medical_service_slots.get(slot as usize) else { return };
    if !s.active || s.sim_id != id {
        return; // demolition freed it already
    }
    let target = s.target_sidecar_index;
    if target >= 0 {
        if let Some(SidecarRecord::MedicalCenter(m)) = world.sidecars.get_mut(target as usize) {
            m.pending_visitors_count = 0.max(m.pending_visitors_count - 1);
        }
    }
    let s = &mut world.medical_service_slots[slot as usize];
    s.active = false;
    s.sim_id = String::new();
    s.source_floor = -1;
    s.target_sidecar_index = -1;
    s.retry_counter = 0;
}

fn slot_id(incident: i64) -> String {
    format!("box:patient:{incident}")
}

fn new_person(st: &mut BoxExtState, incident: i64, role: Role, x: i64, floor: i64, phase: PersonPhase) -> i64 {
    let e = &mut st.emergency;
    let id = e.next_id;
    e.next_id += 1;
    e.people.push(Responder {
        id,
        incident,
        role,
        x,
        floor,
        facing: 1,
        phase,
        route: String::new(),
        rides: 0,
        arrived: false,
    });
    id
}

/// The patient may go, as the game sends people away: an office worker
/// home for the day (0x26), a hotel guest to check out (0x04), a shopper or
/// diner home (0x27).  Anybody else (a condo resident, staff, a cinema or
/// party guest) comes back from treatment to where they belong, in the
/// state they were in.
fn release_patient(world: &mut WorldState, d: &DriverRef, state_then: i64) {
    world.box_ext.emergency.held.retain(|h| h != d);
    if let Some(s) = world.sims.iter_mut().find(|s| d.is(s)) {
        let f = s.family_code;
        let (state, floor) = if f == FAMILY_OFFICE {
            (0x26, LOBBY_FLOOR)
        } else if [FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_TWIN, FAMILY_HOTEL_SUITE].contains(&f) {
            (0x04, s.floor_anchor)
        } else if [FAMILY_RESTAURANT, FAMILY_FAST_FOOD, FAMILY_RETAIL].contains(&f) {
            (0x27, LOBBY_FLOOR)
        } else {
            (state_then & !0x40, s.floor_anchor)
        };
        s.state_code = state;
        s.selected_floor = floor;
        s.route = RouteState::Idle;
        s.destination_floor = -1;
    }
}

// ─── Hooks ────────────────────────────────────────────────────────────────────

/// Hook in advance_sim_refresh_stride: a held sim (collapsed or in care) is
/// skipped.  False at once while nothing is held.
pub fn is_held(world: &WorldState, sim_index: i64) -> bool {
    let held = &world.box_ext.emergency.held;
    !held.is_empty() && held.iter().any(|d| d.is(&world.sims[sim_index as usize]))
}

/// Hook in dispatch_destination_queue_entries for a rider that is no sim:
/// a medic, a member of staff, a guard or a patient has arrived at `floor`.
pub fn on_carrier_arrival(world: &mut WorldState, route_id: &str, floor: i64) {
    if !route_id.starts_with("box:er:") {
        return;
    }
    if let Some(p) = world.box_ext.emergency.people.iter_mut().find(|p| p.route == route_id) {
        p.floor = floor;
        p.arrived = true;
        p.route = String::new();
    }
}

/// Hook in dispatch_destination_queue_entries before the family's arrival
/// handler: a rider due to collapse does so at their stop, and the handler
/// is not run.  False at once while nobody is due.
pub fn collapse_on_arrival(world: &mut WorldState, time: &TimeState, sim_index: i64, floor: i64) -> bool {
    if world.box_ext.emergency.pending.is_empty() {
        return false;
    }
    let si = sim_index as usize;
    let Some(k) = world.box_ext.emergency.pending.iter().position(|d| d.is(&world.sims[si])) else {
        return false;
    };
    world.box_ext.emergency.pending.remove(k);
    let x = match world.sims[si].route {
        RouteState::Carrier { carrier_id, .. } => world
            .carriers
            .iter()
            .find(|c| c.carrier_id == carrier_id)
            .map(|c| lift_x(c.column))
            .unwrap_or(world.sims[si].home_column * SUB),
        _ => world.sims[si].home_column * SUB,
    };
    begin_collapse(world, time, si, floor, x);
    true
}

// ─── Starting incidents ───────────────────────────────────────────────────────

/// In the tower, as far as the game's state says: anybody riding, queued or
/// on the stairs, and anybody standing still who is not gone home, not yet
/// come, or (a condo resident) away from their own floor.
fn present(world: &WorldState, si: usize) -> bool {
    let s = &world.sims[si];
    if s.family_code == 0 {
        return false;
    }
    if !matches!(s.route, RouteState::Idle) {
        return true;
    }
    if s.family_code == FAMILY_CONDO {
        return s.state_code != 0x20 && s.selected_floor == s.floor_anchor;
    }
    !GONE_STATES.contains(&s.state_code)
}

/// Where an idle person is: on a trip to a shop or restaurant (state 0x22)
/// at that venue; a condo resident where the game last put them; anybody
/// else at their own unit (an office, a room, the shop they visit as a
/// customer, the post they staff).
fn idle_position(world: &WorldState, si: usize) -> (i64, i64) {
    let s = &world.sims[si];
    let home = (s.home_column + 1 + s.base_offset % 4) * SUB;
    if s.state_code & 0x3f == 0x22 && s.commercial_venue_slot >= 0 {
        for (key, o) in world.placed_objects.iter() {
            if o.linked_record_index == s.commercial_venue_slot {
                let (_, y) = key_xy(key);
                return (y_to_floor(y), (o.left_tile_index + 1 + s.base_offset % 4) * SUB);
            }
        }
    }
    if s.family_code == FAMILY_CONDO && (0..GRID_HEIGHT).contains(&s.selected_floor) {
        return (s.selected_floor, home);
    }
    (s.floor_anchor, home)
}

fn incidents_under_way(world: &WorldState) -> usize {
    world.box_ext.emergency.incidents.len() + world.box_ext.emergency.pending.len()
}

fn try_collapse(world: &mut WorldState, time: &TimeState) {
    if incidents_under_way(world) >= MAX_INCIDENTS || world.sims.is_empty() {
        return;
    }
    if box_random(world) % COLLAPSE_ODDS >= world.current_population {
        return;
    }
    // Weighted by where people are: one of those in the tower, all alike.
    let here: Vec<usize> = (0..world.sims.len()).filter(|&i| present(world, i) && !is_held(world, i as i64)).collect();
    if here.is_empty() {
        return;
    }
    let si = here[(box_random(world) % here.len() as i64) as usize];
    collapse(world, time, si);
}

/// Make an incident happen now (a test, or a scenario's script): someone in
/// the tower collapses, or with `admission` an ambulance brings a patient
/// in.  True if one started (a rider counts: they collapse at their stop).
pub fn trigger(world: &mut WorldState, time: &TimeState, admission: bool) -> bool {
    if !box_rules(world) || incidents_under_way(world) >= MAX_INCIDENTS {
        return false;
    }
    if admission {
        let before = world.box_ext.emergency.incidents.len();
        start_admission(world, time);
        return world.box_ext.emergency.incidents.len() > before;
    }
    let here: Vec<usize> = (0..world.sims.len()).filter(|&i| present(world, i) && !is_held(world, i as i64)).collect();
    if here.is_empty() {
        return false;
    }
    let si = here[(box_random(world) % here.len() as i64) as usize];
    collapse(world, time, si);
    true
}

/// Sim `sim_index` collapses now, if BOX RULES are on and they are in the
/// tower (a scenario's script, a test).  True if it happened (a rider
/// collapses at their stop).
pub fn collapse_sim(world: &mut WorldState, time: &TimeState, sim_index: i64) -> bool {
    let si = sim_index as usize;
    if !box_rules(world)
        || si >= world.sims.len()
        || incidents_under_way(world) >= MAX_INCIDENTS
        || !present(world, si)
        || is_held(world, sim_index)
        || world.box_ext.emergency.pending.iter().any(|d| d.is(&world.sims[si]))
    {
        return false;
    }
    collapse(world, time, si);
    true
}

/// Sim `si` collapses where they are: standing, queued for a lift (they
/// leave the queue), on the stairs, or aboard a car (at its next stop).
fn collapse(world: &mut WorldState, time: &TimeState, si: usize) {
    match world.sims[si].route.clone() {
        RouteState::Carrier { carrier_id, .. } => {
            let key = sim_key(&world.sims[si]);
            let Some(ci) = world.carriers.iter().position(|c| c.carrier_id == carrier_id) else {
                let (f, x) = idle_position(world, si);
                begin_collapse(world, time, si, f, x);
                return;
            };
            let route = world.carriers[ci].pending_routes.iter().find(|r| r.sim_id == key).cloned();
            match route {
                Some(r) if r.boarded => {
                    let d = DriverRef::of(&world.sims[si]);
                    world.box_ext.emergency.pending.push(d);
                }
                Some(r) => {
                    cancel_runtime_route_request(&mut world.carriers[ci], &key);
                    let x = lift_x(world.carriers[ci].column);
                    begin_collapse(world, time, si, r.source_floor, x);
                }
                None => {
                    let (f, x) = idle_position(world, si);
                    begin_collapse(world, time, si, f, x);
                }
            }
        }
        RouteState::Segment { .. } => {
            let f = world.sims[si].origin_floor;
            let (_, x) = idle_position(world, si);
            begin_collapse(world, time, si, f, x);
        }
        RouteState::Idle => {
            let (f, x) = idle_position(world, si);
            begin_collapse(world, time, si, f, x);
        }
    }
}

/// The incident for sim `si`, collapsed at (`floor`, `x`).
fn begin_collapse(world: &mut WorldState, time: &TimeState, si: usize, site_floor: i64, site_x: i64) {
    let d = DriverRef::of(&world.sims[si]);
    let sim_state = world.sims[si].state_code;
    {
        let s = &mut world.sims[si];
        s.route = RouteState::Idle;
        s.destination_floor = -1;
    }
    let id = world.box_ext.emergency.next_id;
    world.box_ext.emergency.next_id += 1;

    // A medical centre's staff, if there is a centre with a free slot.
    let mut clinic = pick_clinic(world, site_floor, site_x);
    let mut slot = -1;
    if let Some(c) = &clinic {
        slot = alloc_slot(world, c, &slot_id(id), site_floor);
        if slot < 0 {
            clinic = None;
        }
    }
    let entrance_x = street_entrance(world, site_x);
    let guard = send_guard(world, id, site_floor, site_x);

    let st = &mut world.box_ext;
    let patient = new_person(st, id, Role::Patient, site_x, site_floor, PersonPhase::Lying);
    st.emergency.held.push(d.clone());
    st.emergency.collapses += 1;
    let mut incident = Incident {
        id,
        kind: IncidentKind::Collapse,
        patient,
        sim: Some(d),
        sim_state,
        start_tick: time.total_ticks,
        response: -1,
        stress: 0,
        clinic: clinic.clone(),
        slot,
        site_floor,
        site_x,
        entrance_x,
        steps: Vec::new(),
        pc: 0,
        counter: 0,
        team: Team::default(),
        guard,
        ambulance: None,
        admitted_until: -1,
    };
    match clinic {
        Some(c) => {
            for k in 0..2 {
                let m = new_person(st, id, Role::Staff, c.x + k * SUB, c.floor, PersonPhase::Walking);
                incident.team.members.push(m);
            }
            incident.team.floor = c.floor;
            incident.team.x = c.x;
            incident.steps = vec![
                Step::Travel { floor: site_floor, x: site_x },
                Step::Treat,
                Step::PickUp,
                Step::Travel { floor: c.floor, x: c.x },
                Step::Admit,
                Step::StaffHome,
                Step::Discharge,
            ];
        }
        None => {
            for _ in 0..2 {
                let m = new_person(st, id, Role::Medic, entrance_x, STREET_FLOOR, PersonPhase::InAmbulance);
                incident.team.members.push(m);
            }
            incident.team.floor = STREET_FLOOR;
            incident.team.x = entrance_x;
            incident.ambulance = Some(new_ambulance(world.width, entrance_x));
            incident.steps = vec![
                Step::AmbulanceIn,
                Step::Travel { floor: site_floor, x: site_x },
                Step::Treat,
                Step::PickUp,
                Step::Travel { floor: STREET_FLOOR, x: entrance_x },
                Step::AmbulanceOut,
            ];
        }
    }
    world.box_ext.emergency.incidents.push(incident);
    world.pending_notifications.push(SimNotification {
        kind: SimNotificationKind::Event,
        message: Some(COLLAPSE_MESSAGE.to_string()),
    });
}

/// A guard from the security office nearest the patient by the route
/// planner's estimate: guards know the building, so where nothing the player
/// built reaches the patient they take the fire stairs.
fn send_guard(world: &mut WorldState, incident: i64, floor: i64, x: i64) -> Option<Guard> {
    let mut best: Option<(i64, i64, i64)> = None; // (cost, floor, x)
    for (key, o) in world.placed_objects.iter() {
        if o.object_type_code != FAMILY_SECURITY {
            continue;
        }
        let (_, y) = key_xy(key);
        let (of, ox) = (y_to_floor(y), (o.left_tile_index + o.right_tile_index + 1) * SUB / 2);
        let (_, cost, _) = plan(world, of, ox, floor);
        let cost = cost + walk_ticks(ox, x);
        if best.is_none_or(|(b, _, _)| cost < b) {
            best = Some((cost, of, ox));
        }
    }
    let (_, of, ox) = best?;
    let g = new_person(&mut world.box_ext, incident, Role::Guard, ox, of, PersonPhase::Walking);
    world.box_ext.emergency.guards += 1;
    Some(Guard {
        team: Team { members: vec![g], floor: of, x: ox, ..Team::default() },
        phase: GuardPhase::ToPatient,
        arrived_tick: -1,
        office_floor: of,
        office_x: ox,
    })
}

fn new_ambulance(width: i64, entrance_x: i64) -> Ambulance {
    let side = if entrance_x * 2 < width * SUB { -1 } else { 1 };
    Ambulance {
        x: if side < 0 { -AMBULANCE_LENGTH * SUB } else { width * SUB },
        side,
        phase: AmbulancePhase::Arriving,
        beacon: true,
    }
}

fn try_admission(world: &mut WorldState, time: &TimeState) {
    if incidents_under_way(world) >= MAX_INCIDENTS {
        return;
    }
    let n = clinics(world).len() as i64;
    if n == 0 || box_random(world) % ADMISSION_ODDS >= n {
        return;
    }
    start_admission(world, time);
}

fn start_admission(world: &mut WorldState, time: &TimeState) {
    let entrance_x = street_entrance(world, world.width * SUB / 2);
    let Some(c) = pick_clinic(world, STREET_FLOOR, entrance_x) else { return };
    let id = world.box_ext.emergency.next_id;
    let slot = alloc_slot(world, &c, &slot_id(id), STREET_FLOOR);
    if slot < 0 {
        return;
    }
    world.box_ext.emergency.next_id += 1;
    let st = &mut world.box_ext;
    let patient = new_person(st, id, Role::Patient, entrance_x, STREET_FLOOR, PersonPhase::InAmbulance);
    let mut members = Vec::new();
    for _ in 0..2 {
        members.push(new_person(st, id, Role::Medic, entrance_x, STREET_FLOOR, PersonPhase::InAmbulance));
    }
    members.push(patient);
    world.box_ext.emergency.incidents.push(Incident {
        id,
        kind: IncidentKind::Admission,
        patient,
        sim: None,
        sim_state: 0,
        start_tick: time.total_ticks,
        response: -1,
        stress: 0,
        clinic: Some(c.clone()),
        slot,
        site_floor: STREET_FLOOR,
        site_x: entrance_x,
        entrance_x,
        steps: vec![
            Step::AmbulanceIn,
            Step::Travel { floor: c.floor, x: c.x },
            Step::Admit,
            Step::Travel { floor: STREET_FLOOR, x: entrance_x },
            Step::AmbulanceOut,
            Step::Discharge,
        ],
        pc: 0,
        counter: 0,
        team: Team { members, floor: STREET_FLOOR, x: entrance_x, ..Team::default() },
        guard: None,
        ambulance: Some(new_ambulance(world.width, entrance_x)),
        admitted_until: -1,
    });
}

// ─── The tick ─────────────────────────────────────────────────────────────────

/// Hook at the end of service_idle_tasks (before the car elevator's tick):
/// with BOX RULES on, perhaps start an incident, then move every incident
/// on a step.  Nothing while BOX RULES are off.
pub fn tick(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    if !box_rules(world) {
        return;
    }
    // A rider due to collapse who is no longer aboard anything (the route
    // was dropped) collapses where they are.
    let mut k = 0;
    while k < world.box_ext.emergency.pending.len() {
        let d = world.box_ext.emergency.pending[k].clone();
        match world.sims.iter().position(|s| d.is(s)) {
            None => {
                world.box_ext.emergency.pending.remove(k);
            }
            Some(si) if !matches!(world.sims[si].route, RouteState::Carrier { .. }) => {
                world.box_ext.emergency.pending.remove(k);
                let (f, x) = idle_position(world, si);
                begin_collapse(world, time, si, f, x);
            }
            _ => k += 1,
        }
    }
    try_collapse(world, time);
    try_admission(world, time);
    let mut i = 0;
    while i < world.box_ext.emergency.incidents.len() {
        advance_guard(world, time, i);
        if advance(world, ledger, time, i) {
            let inc = world.box_ext.emergency.incidents.remove(i);
            world.box_ext.emergency.people.retain(|p| p.incident != inc.id);
        } else {
            i += 1;
        }
    }
}

fn person_mut(world: &mut WorldState, id: i64) -> &mut Responder {
    world.box_ext.emergency.people.iter_mut().find(|p| p.id == id).expect("a responder")
}

#[derive(Clone, Copy)]
enum Who {
    Team,
    Guard,
}

fn team_mut(world: &mut WorldState, i: usize, who: Who) -> &mut Team {
    let inc = &mut world.box_ext.emergency.incidents[i];
    match who {
        Who::Team => &mut inc.team,
        Who::Guard => &mut inc.guard.as_mut().expect("a guard").team,
    }
}

/// Lay the team out along the corridor at its x, facing `facing`.
fn place_team(world: &mut WorldState, i: usize, who: Who, phase: PersonPhase, facing: i64) {
    let t = team_mut(world, i, who).clone();
    for (k, id) in t.members.into_iter().enumerate() {
        let p = person_mut(world, id);
        p.x = t.x - facing * k as i64 * 12;
        p.floor = t.floor;
        p.facing = facing;
        p.phase = if p.role == Role::Patient { PersonPhase::Carried } else { phase };
    }
}

fn advance_guard(world: &mut WorldState, time: &TimeState, i: usize) {
    let inc = &world.box_ext.emergency.incidents[i];
    let Some(g) = &inc.guard else { return };
    let (site_floor, site_x, responded) = (inc.site_floor, inc.site_x, inc.response >= 0);
    match g.phase {
        GuardPhase::ToPatient => {
            if responded || travel(world, i, Who::Guard, site_floor, site_x - SUB) {
                let g = world.box_ext.emergency.incidents[i].guard.as_mut().unwrap();
                if !responded {
                    g.arrived_tick = time.total_ticks;
                }
                g.phase = if responded { GuardPhase::Returning } else { GuardPhase::WithPatient };
                g.team.stage = 0;
                place_team(world, i, Who::Guard, PersonPhase::Standing, 1);
            }
        }
        GuardPhase::WithPatient => {
            if responded {
                let g = world.box_ext.emergency.incidents[i].guard.as_mut().unwrap();
                g.phase = GuardPhase::Returning;
                g.team.stage = 0;
            }
        }
        GuardPhase::Returning => {
            let (of, ox) = (g.office_floor, g.office_x);
            if travel(world, i, Who::Guard, of, ox) {
                let g = world.box_ext.emergency.incidents[i].guard.take().unwrap();
                world.box_ext.emergency.people.retain(|p| !g.team.members.contains(&p.id));
            }
        }
    }
}

/// One tick of incident `i`; true when it is over.
fn advance(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, i: usize) -> bool {
    let inc = &world.box_ext.emergency.incidents[i];
    if (inc.pc as usize) < inc.steps.len() {
        let step = inc.steps[inc.pc as usize].clone();
        if run_step(world, ledger, time, i, step) {
            let inc = &mut world.box_ext.emergency.incidents[i];
            inc.pc += 1;
            inc.counter = 0;
            inc.team.stage = 0;
        }
    }
    let inc = &world.box_ext.emergency.incidents[i];
    inc.pc as usize >= inc.steps.len() && inc.ambulance.is_none() && inc.guard.is_none()
}

/// One tick of a step; true when it is done.
fn run_step(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState, i: usize, step: Step) -> bool {
    match step {
        Step::AmbulanceIn => {
            let inc = &mut world.box_ext.emergency.incidents[i];
            let stop = inc.entrance_x - AMBULANCE_LENGTH * SUB / 2;
            let a = inc.ambulance.as_mut().expect("an ambulance");
            a.x = move_toward(a.x, stop, AMBULANCE_SPEED);
            if a.x != stop {
                return false;
            }
            a.phase = AmbulancePhase::Parked;
            place_team(world, i, Who::Team, PersonPhase::Walking, 1);
            true
        }
        Step::Travel { floor, x } => travel(world, i, Who::Team, floor, x),
        Step::Treat => {
            let inc = &mut world.box_ext.emergency.incidents[i];
            if inc.counter == 0 && inc.response < 0 {
                let now = time.total_ticks;
                inc.response = now - inc.start_tick;
                // The wait counts against the tower as a slow lift does:
                // every other person of the patient's unit takes it as
                // stress; from a guard's arrival on, at half.
                let stress = match &inc.guard {
                    Some(g) if g.arrived_tick >= 0 => (g.arrived_tick - inc.start_tick) + (now - g.arrived_tick) / 2,
                    _ => inc.response,
                };
                inc.stress = stress;
                let (response, sim, team) = (inc.response, inc.sim.clone(), inc.team.members.clone());
                world.box_ext.emergency.response_ticks += response;
                for id in team {
                    person_mut(world, id).phase = PersonPhase::Treating;
                }
                if let Some(d) = sim {
                    for s in world.sims.iter_mut() {
                        if s.floor_anchor == d.floor_anchor && s.home_column == d.home_column && !d.is(s) {
                            add_delay_to_current_sim(s, stress);
                        }
                    }
                }
            }
            let inc = &mut world.box_ext.emergency.incidents[i];
            inc.counter += 1;
            inc.counter >= TREAT_TICKS
        }
        Step::PickUp => {
            let inc = &mut world.box_ext.emergency.incidents[i];
            let p = inc.patient;
            inc.team.members.push(p);
            person_mut(world, p).phase = PersonPhase::Carried;
            true
        }
        Step::Admit => {
            let inc = &mut world.box_ext.emergency.incidents[i];
            let p = inc.patient;
            inc.team.members.retain(|&m| m != p);
            inc.admitted_until = time.total_ticks + ADMIT_TICKS;
            let clinic = inc.clinic.clone().expect("a medical centre");
            let alive = clinic_alive(world, &clinic);
            if !alive {
                world.box_ext.emergency.incidents[i].admitted_until = time.total_ticks;
            }
            let person = person_mut(world, p);
            person.phase = PersonPhase::Admitted;
            person.x = clinic.x;
            person.floor = clinic.floor;
            if alive {
                ledger.cash_balance = ledger.cap().min(ledger.cash_balance + ADMISSION_INCOME);
                world.box_ext.emergency.admissions += 1;
            }
            true
        }
        Step::StaffHome => {
            let team = std::mem::take(&mut world.box_ext.emergency.incidents[i].team.members);
            world.box_ext.emergency.people.retain(|p| !team.contains(&p.id));
            true
        }
        Step::AmbulanceOut => {
            let inc = &mut world.box_ext.emergency.incidents[i];
            let a = inc.ambulance.as_mut().expect("an ambulance");
            if a.phase == AmbulancePhase::Parked {
                a.phase = AmbulancePhase::Leaving;
                let team = std::mem::take(&mut inc.team.members);
                let carried = if inc.kind == IncidentKind::Collapse { inc.sim.clone() } else { None };
                let state_then = inc.sim_state;
                world.box_ext.emergency.people.retain(|p| !team.contains(&p.id));
                if let Some(d) = carried {
                    release_patient(world, &d, state_then);
                }
            }
            let width = world.width;
            let a = world.box_ext.emergency.incidents[i].ambulance.as_mut().unwrap();
            let end = if a.side < 0 { -AMBULANCE_LENGTH * SUB } else { width * SUB };
            a.x = move_toward(a.x, end, AMBULANCE_SPEED);
            if a.x != end {
                return false;
            }
            world.box_ext.emergency.incidents[i].ambulance = None;
            true
        }
        Step::Discharge => {
            let inc = &world.box_ext.emergency.incidents[i];
            if time.total_ticks < inc.admitted_until {
                return false;
            }
            let (slot, id, sim, patient, state_then) =
                (inc.slot, slot_id(inc.id), inc.sim.clone(), inc.patient, inc.sim_state);
            free_slot(world, slot, &id);
            if let Some(d) = sim {
                release_patient(world, &d, state_then);
            }
            world.box_ext.emergency.people.retain(|p| p.id != patient);
            true
        }
    }
}

/// One tick of a team's journey to (`floor`, `x`): plan, then each leg
/// (walk to it; ride, or climb), then walk to the goal.  True on arrival.
fn travel(world: &mut WorldState, i: usize, who: Who, floor: i64, x: i64) -> bool {
    let t = team_mut(world, i, who).clone();
    match t.stage {
        0 => {
            if t.floor == floor {
                team_mut(world, i, who).stage = 3;
                return false;
            }
            let (legs, _, _) = plan(world, t.floor, t.x, floor);
            let t = team_mut(world, i, who);
            t.legs = legs;
            t.leg = 0;
            t.stage = 1;
            false
        }
        1 => {
            let Some(leg) = t.legs.get(t.leg as usize).cloned() else {
                team_mut(world, i, who).stage = 3;
                return false;
            };
            let facing = if leg.at_x >= t.x { 1 } else { -1 };
            let tm = team_mut(world, i, who);
            tm.x = move_toward(t.x, leg.at_x, WALK_SPEED);
            let there = tm.x == leg.at_x;
            place_team(world, i, who, PersonPhase::Walking, facing);
            if there {
                let tm = team_mut(world, i, who);
                tm.stage = 2;
                tm.counter = (leg.to_floor - t.floor).abs()
                    * match leg.kind {
                        LegKind::Stairs => STAIRS_TICKS_PER_FLOOR,
                        LegKind::Escalator => ESCALATOR_TICKS_PER_FLOOR,
                        LegKind::FireStairs => FIRE_STAIRS_TICKS_PER_FLOOR,
                        LegKind::Lift => 0,
                    };
                let members = tm.members.clone();
                for id in members {
                    let p = person_mut(world, id);
                    p.arrived = false;
                    p.route = String::new();
                    p.phase = if leg.kind == LegKind::Lift { PersonPhase::Lift } else { PersonPhase::Stairs };
                }
            }
            false
        }
        2 => {
            let leg = t.legs[t.leg as usize].clone();
            let leg_done = if leg.kind == LegKind::Lift {
                ride(world, &t, &leg)
            } else {
                let tm = team_mut(world, i, who);
                tm.counter -= 1;
                Some(tm.counter <= 0)
            };
            match leg_done {
                Some(true) => {
                    let tm = team_mut(world, i, who);
                    tm.floor = leg.to_floor;
                    tm.leg += 1;
                    tm.stage = if (tm.leg as usize) < tm.legs.len() { 1 } else { 3 };
                    place_team(world, i, who, PersonPhase::Walking, 1);
                }
                Some(false) => {}
                // The lift no longer serves: plan again from here.
                None => team_mut(world, i, who).stage = 0,
            }
            false
        }
        _ => {
            let facing = if x >= t.x { 1 } else { -1 };
            let tm = team_mut(world, i, who);
            tm.x = move_toward(t.x, x, WALK_SPEED);
            let there = tm.x == x;
            place_team(world, i, who, if there { PersonPhase::Standing } else { PersonPhase::Walking }, facing);
            there
        }
    }
}

/// A lift leg: queue each member as a passenger, wait for every one to be
/// delivered.  Some(true) all there, Some(false) still going, None the lift
/// is gone or no longer serves both floors (the members leave its queue).
fn ride(world: &mut WorldState, t: &Team, leg: &Leg) -> Option<bool> {
    let from = t.floor;
    let ci = world.carriers.iter().position(|c| c.carrier_id == leg.carrier);
    let ok = ci.is_some_and(|ci| serves(world, ci, from) && serves(world, ci, leg.to_floor));
    if !ok {
        for &id in &t.members {
            let route = person_mut(world, id).route.clone();
            if let (Some(ci), false) = (ci, route.is_empty()) {
                cancel_runtime_route_request(&mut world.carriers[ci], &route);
            }
            let p = person_mut(world, id);
            p.route = String::new();
            p.floor = from;
        }
        return None;
    }
    let ci = ci.unwrap();
    let lobby_mode = world.lobby_mode;
    let mut all = true;
    for &id in &t.members {
        let p = person_mut(world, id);
        if p.arrived {
            continue;
        }
        all = false;
        let route = p.route.clone();
        let c = &world.carriers[ci];
        let aboard_or_queued = !route.is_empty()
            && (c.pending_routes.iter().any(|r| r.sim_id == route)
                || c.cars.iter().any(|car| car.active_route_slots.iter().any(|s| s.active && s.route_id == route)));
        if aboard_or_queued {
            continue;
        }
        // Not queued (just came, or the queue was flushed): queue.
        let p = person_mut(world, id);
        let route = if route.is_empty() {
            p.rides += 1;
            format!("box:er:{}:{}", p.id, p.rides)
        } else {
            route
        };
        let dir = if leg.to_floor > from { 1 } else { 0 };
        if enqueue_request_into_route_queue(&mut world.carriers[ci], &route, from, leg.to_floor, dir, lobby_mode) {
            person_mut(world, id).route = route;
        }
    }
    Some(all)
}

// ─── Views ────────────────────────────────────────────────────────────────────

/// A medic, member of staff, guard or patient as the presenter draws them.
/// `family` is the people figure to draw ("medical", or "security" for a
/// guard); `x` in cells (fractional); `floor` the floor index (fractional
/// while riding: the car's position); `row` the grid row.  `inLift`: aboard
/// a car (rather than queued for one) when `phase` is `lift`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonView {
    /// On a fire escape: the stair, the rows it is climbed between and how
    /// far up (or down) it the person is, 0 to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub climb: Option<Climb>,
    pub id: i64,
    pub incident: i64,
    pub family: &'static str,
    pub role: Role,
    pub x: f64,
    pub floor: f64,
    pub row: f64,
    pub facing: i64,
    pub phase: PersonPhase,
    pub in_lift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Climb {
    pub escape: crate::box_ext::escapes::Escape,
    pub from_row: i64,
    pub to_row: i64,
    pub progress: f64,
}

/// The fire-escape climb of the team person `id` is in, if it is on one
fn climb_of(world: &WorldState, id: i64) -> Option<Climb> {
    for inc in &world.box_ext.emergency.incidents {
        let teams = std::iter::once(&inc.team).chain(inc.guard.as_ref().map(|g| &g.team));
        for t in teams {
            if !t.members.contains(&id) || t.stage != 2 {
                continue;
            }
            let leg = t.legs.get(t.leg as usize)?;
            if leg.kind != LegKind::FireStairs {
                return None;
            }
            let (from_row, to_row) = (floor_to_y(t.floor), floor_to_y(leg.to_floor));
            let escape = crate::box_ext::escapes::for_trip(world, t.x as f64 / SUB as f64, from_row, to_row)?;
            let total = ((leg.to_floor - t.floor).abs() * FIRE_STAIRS_TICKS_PER_FLOOR).max(1);
            return Some(Climb { escape, from_row, to_row, progress: (1.0 - t.counter as f64 / total as f64).clamp(0.0, 1.0) });
        }
    }
    None
}

pub fn people_views(world: &WorldState) -> Vec<PersonView> {
    let mut out = Vec::new();
    for p in &world.box_ext.emergency.people {
        if p.phase == PersonPhase::InAmbulance {
            continue;
        }
        let mut floor = p.floor as f64;
        let mut x = p.x as f64 / SUB as f64;
        let mut in_lift = false;
        if p.phase == PersonPhase::Lift && !p.route.is_empty() {
            for c in &world.carriers {
                if let Some(r) = c.pending_routes.iter().find(|r| r.sim_id == p.route) {
                    if r.boarded && r.assigned_car_index >= 0 {
                        if let Some(car) = c.cars.get(r.assigned_car_index as usize) {
                            floor = car.current_floor as f64;
                            x = c.column as f64 + 1.0;
                            in_lift = true;
                        }
                    }
                }
            }
        }
        out.push(PersonView {
            climb: if p.phase == PersonPhase::Stairs { climb_of(world, p.id) } else { None },
            id: p.id,
            incident: p.incident,
            family: if p.role == Role::Guard { "security" } else { "medical" },
            role: p.role,
            x,
            floor,
            row: (GRID_HEIGHT - 1) as f64 - floor,
            facing: p.facing,
            phase: p.phase,
            in_lift,
        });
    }
    out
}

//! BOX addition: the service vehicles in the street (docs/emergency-services.md).
//!
//! - A fire: a pumper and a ladder truck drive in from the nearer end of the
//!   street, park below the fire, and drive away when it is out; the ladder
//!   is raised toward the burning floor while parked if that floor is within
//!   LADDER_REACH floors of the street.
//! - A bomb search: two police cars, likewise.
//! - BOX RULES' ambulances (box_ext::emergency).
//!
//! Beacons are on throughout.  They are drawn BOX RULES or not.  With BOX
//! RULES off they are a picture only: runtime state, never saved, read by no
//! rule.  Under BOX RULES they are saved with the tower and they work:
//!
//! - parked fire engines fight the fire on the floors their ladder reaches:
//!   the fronts there spread at half speed, and every ENGINE_KNOCKDOWN_TICKS
//!   the engines put out the lowest burning floor within reach;
//! - a fire cannot climb past a floor that is not burning (under BOX RULES
//!   generally), and a fire with nothing left burning is out;
//! - parked police cars search for the bomb: a search step a tick of their
//!   own, from the bomb's floor outward, beside any security guards'.
//!
//! Hooks in events.rs (each `// BOX: street`): `fire_ignition_blocked`,
//! `fire_spread_held`, `fire_out`, `police_search`.  Each is false or
//! nothing at once with BOX RULES off.

use serde::{Deserialize, Serialize};

use crate::box_ext::emergency::{AmbulancePhase, AMBULANCE_LENGTH, SUB};
use crate::world::{WorldState, GRID_HEIGHT, GROUND_Y};

/// Floors above the street the raised ladder reaches (the art's ladder tip
/// is a little over three floors up).
pub const LADDER_REACH: i64 = 3;
/// Two cells a tick, as cars.
pub const SERVICE_SPEED: i64 = 32;
/// Ticks of parked engines' work to put out one floor within reach.
pub const ENGINE_KNOCKDOWN_TICKS: i64 = 40;
const STREET_FLOOR: i64 = GRID_HEIGHT - 1 - GROUND_Y;
const FIRE_ACTIVE: i64 = 1 << 3;
const BOMB_SEARCH: i64 = 1;
const NO_FRONT: i64 = 0xffff;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Event {
    Fire,
    Bomb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreetKind {
    FirePumper,
    FireLadder,
    Police,
}

impl StreetKind {
    /// The art/src/vehicles type.
    pub fn style(self) -> &'static str {
        match self {
            StreetKind::FirePumper => "fire-pumper",
            StreetKind::FireLadder => "fire-ladder",
            StreetKind::Police => "police",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreetPhase {
    Arriving,
    Parked,
    Leaving,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetVehicle {
    pub event: Event,
    pub kind: StreetKind,
    pub length: i64,
    /// Left end, sixteenths of a cell; `stop` where it parks.
    pub x: i64,
    pub stop: i64,
    pub side: i64,
    pub phase: StreetPhase,
}

/// The fire engines and police cars in the street.  Saved only under BOX
/// RULES (`kept`); in a classic game runtime only.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetState {
    /// BOX RULES on: saved with the tower.
    #[serde(default)]
    pub kept: bool,
    #[serde(default)]
    pub vehicles: Vec<StreetVehicle>,
    /// Parked engines' work toward the next floor put out.
    #[serde(default)]
    pub knockdown: i64,
    /// The police have begun their search of this bomb.
    #[serde(default)]
    pub police_searching: bool,
}

impl StreetState {
    /// Left out of the snapshot: always in a classic game, and under BOX
    /// RULES while the street is empty.
    pub fn not_saved(&self) -> bool {
        !self.kept || (self.vehicles.is_empty() && self.knockdown == 0 && !self.police_searching)
    }
}

fn spawn(width: i64, event: Event, kind: StreetKind, length: i64, at: i64) -> StreetVehicle {
    let stop = at.clamp(0, (width - length).max(0)) * SUB;
    let side = if stop * 2 < width * SUB { -1 } else { 1 };
    StreetVehicle {
        event,
        kind,
        length,
        x: if side < 0 { -length * SUB } else { width * SUB },
        stop,
        side,
        phase: StreetPhase::Arriving,
    }
}

/// Hook at the end of service_idle_tasks, BOX RULES or not: the street's
/// vehicles move.
pub fn tick(world: &mut WorldState) {
    let kept = world.box_ext.box_rules;
    let es = &world.event_state;
    let w = world.width;
    let fire = es.game_state_flags & FIRE_ACTIVE != 0;
    let bomb = es.game_state_flags & BOMB_SEARCH != 0;
    let (fire_tile, bomb_tile) = (es.fire_tile.clamp(0, w - 1), es.bomb_tile.clamp(0, w - 1));
    let street = &mut world.box_ext.street;
    street.kept = kept;
    if !fire {
        street.knockdown = 0;
    }
    if !bomb {
        street.police_searching = false;
    }
    if street.vehicles.is_empty() && !fire && !bomb {
        return;
    }
    let present = |street: &StreetState, e: Event| {
        street.vehicles.iter().any(|v| v.event == e && v.phase != StreetPhase::Leaving)
    };
    if fire && !present(street, Event::Fire) {
        street.vehicles.push(spawn(w, Event::Fire, StreetKind::FirePumper, 9, fire_tile - 11));
        street.vehicles.push(spawn(w, Event::Fire, StreetKind::FireLadder, 10, fire_tile));
    }
    if bomb && !present(street, Event::Bomb) {
        street.vehicles.push(spawn(w, Event::Bomb, StreetKind::Police, 6, bomb_tile - 7));
        street.vehicles.push(spawn(w, Event::Bomb, StreetKind::Police, 6, bomb_tile + 1));
    }
    for v in street.vehicles.iter_mut() {
        let on = match v.event {
            Event::Fire => fire,
            Event::Bomb => bomb,
        };
        if !on && v.phase != StreetPhase::Leaving {
            v.phase = StreetPhase::Leaving;
        }
        let target = if v.phase == StreetPhase::Leaving {
            if v.side < 0 {
                -v.length * SUB
            } else {
                w * SUB
            }
        } else {
            v.stop
        };
        v.x = if v.x < target { (v.x + SERVICE_SPEED).min(target) } else { (v.x - SERVICE_SPEED).max(target) };
        if v.phase == StreetPhase::Arriving && v.x == v.stop {
            v.phase = StreetPhase::Parked;
        }
    }
    street.vehicles.retain(|v| !(v.phase == StreetPhase::Leaving && (v.x == -v.length * SUB || v.x == w * SUB)));
}

// ─── Under BOX RULES: what they do ───────────────────────────────────────────

/// The pumper and the ladder truck are both parked (BOX RULES on).
fn engines_at_work(world: &WorldState) -> bool {
    let s = &world.box_ext.street;
    world.box_ext.box_rules
        && [StreetKind::FirePumper, StreetKind::FireLadder].iter().all(|k| {
            s.vehicles.iter().any(|v| v.kind == *k && v.event == Event::Fire && v.phase == StreetPhase::Parked)
        })
}

fn within_reach(floor: i64) -> bool {
    (STREET_FLOOR + 1..=STREET_FLOOR + LADDER_REACH).contains(&floor)
}

fn burning(world: &WorldState, floor: i64) -> bool {
    let es = &world.event_state;
    (0..GRID_HEIGHT).contains(&floor)
        && (es.fire_left_pos[floor as usize] != NO_FRONT || es.fire_right_pos[floor as usize] != NO_FRONT)
}

/// Hook in advance_fire_spread at a floor's ignition: under BOX RULES a fire
/// climbs only from a floor that is burning.
pub fn fire_ignition_blocked(world: &WorldState, floor: i64) -> bool {
    world.box_ext.box_rules && floor > world.event_state.fire_floor && !burning(world, floor - 1)
}

/// Hook in advance_fire_spread before a floor's fronts move (spread step
/// `step`): within the ladder's reach of parked engines, every other step is
/// held (half speed).
pub fn fire_spread_held(world: &WorldState, floor: i64, step: i64) -> bool {
    step % 2 == 1 && within_reach(floor) && engines_at_work(world)
}

/// Hook in tick_fire_event after the spread: parked engines put out the
/// lowest burning floor within reach every ENGINE_KNOCKDOWN_TICKS; under BOX
/// RULES a fire with nothing burning is out (true: resolve it).
pub fn fire_out(world: &mut WorldState) -> bool {
    if !world.box_ext.box_rules {
        return false;
    }
    if engines_at_work(world) {
        world.box_ext.street.knockdown += 1;
        if world.box_ext.street.knockdown >= ENGINE_KNOCKDOWN_TICKS {
            world.box_ext.street.knockdown = 0;
            if let Some(f) = (STREET_FLOOR + 1..=STREET_FLOOR + LADDER_REACH).find(|&f| burning(world, f)) {
                let es = &mut world.event_state;
                es.fire_left_pos[f as usize] = NO_FRONT;
                es.fire_right_pos[f as usize] = NO_FRONT;
            }
        }
    }
    !(0..GRID_HEIGHT).any(|f| burning(world, f))
}

/// What the police do this tick (hook in tick_bomb_event).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoliceSearch {
    None,
    /// Begin the search at the bomb's floor (nobody was searching).
    Start,
    /// One more search step.
    Step,
}

/// Hook in tick_bomb_event: parked police cars (BOX RULES on) search: they
/// begin at the bomb's floor if nobody is searching yet, and add a search
/// step a tick of their own.
pub fn police_search(world: &mut WorldState) -> PoliceSearch {
    if !world.box_ext.box_rules {
        return PoliceSearch::None;
    }
    let parked = world
        .box_ext
        .street
        .vehicles
        .iter()
        .any(|v| v.kind == StreetKind::Police && v.phase == StreetPhase::Parked);
    if !parked {
        return PoliceSearch::None;
    }
    if !world.box_ext.street.police_searching {
        world.box_ext.street.police_searching = true;
        if world.event_state.bomb_search_current_floor < 0 {
            return PoliceSearch::Start;
        }
    }
    PoliceSearch::Step
}

// ─── Views ────────────────────────────────────────────────────────────────────

/// A service vehicle as the presenter draws it.  `style` is an
/// art/src/vehicles type ("ambulance", "police", "fire-pumper",
/// "fire-ladder", "fire-ladder-deployed"); `x` its left end in cells, on
/// the street (`floor` 10); `ladderTo` the floor the raised ladder points
/// at (fire-ladder-deployed only).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceVehicleView {
    pub style: &'static str,
    pub x: f64,
    pub floor: f64,
    pub row: f64,
    pub length: f64,
    pub facing: i64,
    pub beacon: bool,
    pub moving: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ladder_to: Option<i64>,
}

pub fn service_vehicle_views(world: &WorldState) -> Vec<ServiceVehicleView> {
    let mut out = Vec::new();
    let fire_floor = world.event_state.fire_floor;
    let up = fire_floor - STREET_FLOOR;
    for v in &world.box_ext.street.vehicles {
        let raised =
            v.kind == StreetKind::FireLadder && v.phase == StreetPhase::Parked && (1..=LADDER_REACH).contains(&up);
        out.push(ServiceVehicleView {
            style: if raised { "fire-ladder-deployed" } else { v.kind.style() },
            x: v.x as f64 / SUB as f64,
            floor: STREET_FLOOR as f64,
            row: GROUND_Y as f64,
            length: v.length as f64,
            facing: if v.phase == StreetPhase::Leaving { v.side } else { -v.side },
            beacon: true,
            moving: v.phase != StreetPhase::Parked,
            ladder_to: raised.then_some(fire_floor),
        });
    }
    for inc in &world.box_ext.emergency.incidents {
        if let Some(a) = &inc.ambulance {
            out.push(ServiceVehicleView {
                style: "ambulance",
                x: a.x as f64 / SUB as f64,
                floor: STREET_FLOOR as f64,
                row: GROUND_Y as f64,
                length: AMBULANCE_LENGTH as f64,
                facing: match a.phase {
                    AmbulancePhase::Leaving => a.side,
                    _ => -a.side,
                },
                beacon: a.beacon,
                moving: a.phase != AmbulancePhase::Parked,
                ladder_to: None,
            });
        }
    }
    out
}

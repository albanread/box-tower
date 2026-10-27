//! BOX addition (BOX RULES only): a fire the tower lives through
//! (docs/fire.md).
//!
//! Upstream's fire burns rooms out along a floor and climbs; the people in
//! the tower carry on as if nothing were happening.  Under BOX RULES:
//!
//! - the alarm sounds when a fire starts: everyone at home or at work on
//!   the fire floor, the floor below it and every floor above goes out (the
//!   engine draws them walking calmly down the fire escapes), and stays out
//!   until the all-clear, ALL_CLEAR_TICKS after the fire is out;
//! - the floors the fire reached are closed for the rest of the day: their
//!   people come back the next morning, their shops and restaurants take no
//!   more visitors, and the engine draws the rooms dark;
//! - going out is a trip of its own: EVACUATION_STRESS on each person's
//!   trip stress, so the ratings show it for a while;
//! - a sprinkler room (an amenity, amenities.rs) covers the floors within
//!   SPRINKLER_REACH of its own: a fire there spreads at half speed and does
//!   not climb into them.
//!
//! Nobody is hurt: everyone gets out.
//!
//! Hooks: service_idle_tasks.rs `tick`; sims/mod.rs `is_out` (the sims'
//! refresh); events.rs advance_fire_spread `spread_held`, `ignition_blocked`;
//! day_scheduler.rs `daily`.  Each does nothing with BOX RULES off.

use serde::{Deserialize, Serialize};

use crate::box_ext::box_rules;
use crate::box_ext::car_elevator::DriverRef;
use crate::sims::states::STATE_TRANSIT_FLAG;
use crate::time::TimeState;
use crate::world::{y_to_floor, RouteState, SidecarRecord, WorldState, GRID_HEIGHT, VENUE_CLOSED, VENUE_DORMANT};

/// Ticks from the fire's being out to the all-clear (about an hour).
pub const ALL_CLEAR_TICKS: i64 = 100;
/// What going out adds to a person's trip stress.
pub const EVACUATION_STRESS: i64 = 40;
/// Floors a sprinkler room covers each side of its own.
pub const SPRINKLER_REACH: i64 = 5;
/// How often the alarm looks for people to send out (ticks).
const SWEEP_TICKS: i64 = 8;
const FIRE_ACTIVE: i64 = 1 << 3;
const NO_FRONT: i64 = 0xffff;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FireState {
    /// The alarm is sounding (a fire is burning).
    #[serde(default)]
    pub alarm: bool,
    /// The lowest floor sent out.
    #[serde(default)]
    pub from_floor: i64,
    /// People out of the tower, and the floor each went from.
    #[serde(default)]
    pub out: Vec<Evacuee>,
    /// Floors the fire reached, closed until the morning.
    #[serde(default)]
    pub burned: Vec<i64>,
    /// total_ticks of the all-clear (0: none due).
    #[serde(default)]
    pub all_clear_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evacuee {
    pub who: DriverRef,
    /// The floor they left from, and the total_ticks they left at.
    pub floor: i64,
    pub since: i64,
}

impl FireState {
    pub fn is_empty(&self) -> bool {
        !self.alarm && self.out.is_empty() && self.burned.is_empty() && self.all_clear_at == 0
    }
}

fn fire_active(world: &WorldState) -> bool {
    world.event_state.game_state_flags & FIRE_ACTIVE != 0
}

fn burning(world: &WorldState, floor: i64) -> bool {
    let es = &world.event_state;
    (0..GRID_HEIGHT).contains(&floor)
        && (es.fire_left_pos[floor as usize] != NO_FRONT || es.fire_right_pos[floor as usize] != NO_FRONT)
}

/// Hook in the service tick: sound the alarm, send people out, note the
/// floors burning, and give the all-clear.
pub fn tick(world: &mut WorldState, time: &TimeState) {
    if !box_rules(world) {
        return;
    }
    let fire = fire_active(world);
    if fire {
        if !world.box_ext.fire.alarm {
            let f = &mut world.box_ext.fire;
            f.alarm = true;
            f.all_clear_at = 0;
            f.from_floor = world.event_state.fire_floor - 1;
        }
        for floor in 0..GRID_HEIGHT {
            if burning(world, floor) && !world.box_ext.fire.burned.contains(&floor) && built(world, floor) {
                world.box_ext.fire.burned.push(floor);
                close_venues(world, floor);
            }
        }
        if time.total_ticks % SWEEP_TICKS == 0 {
            send_out(world, time);
        }
        return;
    }
    let f = &mut world.box_ext.fire;
    if f.alarm {
        f.alarm = false;
        f.all_clear_at = time.total_ticks + ALL_CLEAR_TICKS;
    }
    if f.all_clear_at != 0 && time.total_ticks >= f.all_clear_at {
        // the all-clear: everyone back but those from closed floors
        f.all_clear_at = 0;
        let burned = f.burned.clone();
        f.out.retain(|e| burned.contains(&e.who.floor_anchor));
    }
}

/// The shops, restaurants and fast food on a floor the fire reached close
/// for the rest of the day (they open again with the others in the morning).
fn close_venues(world: &mut WorldState, floor: i64) {
    let links: Vec<i64> = world
        .placed_objects
        .iter()
        .filter(|(k, o)| {
            o.linked_record_index >= 0
                && k.split_once(',').and_then(|(_, y)| y.parse::<i64>().ok()).is_some_and(|y| y_to_floor(y) == floor)
        })
        .map(|(_, o)| o.linked_record_index)
        .collect();
    for i in links {
        if let Some(SidecarRecord::CommercialVenue(v)) = world.sidecars.get_mut(i as usize) {
            if v.availability_state != VENUE_DORMANT {
                v.availability_state = VENUE_CLOSED;
            }
        }
    }
}

/// Everyone at rest on the floors the alarm covers goes out (people already
/// on their way somewhere carry on and are sent out when they stop there).
fn send_out(world: &mut WorldState, time: &TimeState) {
    let from = world.box_ext.fire.from_floor;
    let mut going: Vec<Evacuee> = Vec::new();
    for sim in world.sims.iter() {
        let at_rest = sim.route == RouteState::Idle && sim.state_code & STATE_TRANSIT_FLAG == 0;
        if !at_rest || sim.selected_floor < from || sim.selected_floor <= street_floor() {
            continue;
        }
        if world.box_ext.fire.out.iter().any(|e| e.who.is(sim)) {
            continue;
        }
        going.push(Evacuee { who: DriverRef::of(sim), floor: sim.selected_floor, since: time.total_ticks });
    }
    for e in &going {
        if let Some(sim) = world.sims.iter_mut().find(|s| e.who.is(s)) {
            sim.accumulated_ticks += EVACUATION_STRESS;
        }
    }
    world.box_ext.fire.out.extend(going);
}

fn street_floor() -> i64 {
    y_to_floor(crate::world::GROUND_Y)
}

/// Hook in the sims' refresh: someone out of the tower does nothing.
pub fn is_out(world: &WorldState, sim_index: i64) -> bool {
    if world.box_ext.fire.out.is_empty() {
        return false;
    }
    let Some(sim) = world.sims.get(sim_index as usize) else { return false };
    world.box_ext.fire.out.iter().any(|e| e.who.is(sim))
}

/// Hook at the day's end: the closed floors open and everyone comes home.
pub fn daily(world: &mut WorldState) {
    if world.box_ext.fire.is_empty() || fire_active(world) {
        return;
    }
    let f = &mut world.box_ext.fire;
    f.out.clear();
    f.burned.clear();
    f.all_clear_at = 0;
}

/// Floors covered by a sprinkler room.
fn sprinkled(world: &WorldState, floor: i64) -> bool {
    world.box_ext.amenities.rooms.iter().any(|r| {
        r.tile == "sprinklers"
            && r.built_day != crate::box_ext::amenities::NEW
            && (y_to_floor(r.y) - floor).abs() <= SPRINKLER_REACH
    })
}

/// Hook in advance_fire_spread: sprinklers halve the spread on the floors
/// they cover.
pub fn spread_held(world: &WorldState, floor: i64, step: i64) -> bool {
    box_rules(world) && step % 2 == 1 && sprinkled(world, floor)
}

/// Anything built on `floor`.
fn built(world: &WorldState, floor: i64) -> bool {
    world.cells.keys().any(|k| k.split_once(',').and_then(|(_, y)| y.parse::<i64>().ok()).is_some_and(|y| y_to_floor(y) == floor))
}

/// Hook in advance_fire_spread at a floor's ignition: a fire does not climb
/// into a floor sprinklers cover, nor into the sky above the tower.
pub fn ignition_blocked(world: &WorldState, floor: i64) -> bool {
    box_rules(world) && (sprinkled(world, floor) || !built(world, floor))
}

/// The engine's view: people out, by the floor and cell they left from,
/// and the floors closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireView {
    pub alarm: bool,
    /// (floor, cell, total_ticks they left at)
    pub out: Vec<(i64, i64, i64)>,
    pub closed: Vec<i64>,
}

pub fn view(world: &WorldState) -> FireView {
    let f = &world.box_ext.fire;
    FireView {
        alarm: f.alarm,
        out: f.out.iter().map(|e| (e.floor, e.who.home_column, e.since)).collect(),
        closed: f.burned.clone(),
    }
}

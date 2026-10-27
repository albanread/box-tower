//! BOX addition (BOX RULES only): the tower's residents who work in its
//! offices go to work from home (docs/people.md, "Working at home").
//!
//! The register (people.rs) says how many of the tower's jobs its own
//! residents hold.  Here that becomes people: at each day's end the office
//! jobs among them are given to that many office workers (by a hash of
//! their place, never one who drives), each with a home in one of the
//! tower's condominiums.  In the morning such a worker sets off from their
//! home's floor instead of the lobby, and in the evening goes back to it,
//! when a lift, stairs or escalator joins the two floors.  So a tower that
//! houses its own workers has less traffic through its lobby.
//!
//! Hooks in sims/office.rs (each `// BOX: locals`), beside the metro's;
//! `daily` at the day's end.  Each is None, false or nothing with BOX RULES
//! off.

use serde::{Deserialize, Serialize};

use crate::box_ext::car_elevator::DriverRef;
use crate::box_ext::{box_rules, fnv, people};
use crate::resources::{FAMILY_CONDO, FAMILY_OFFICE};
use crate::route_scoring::select_candidate::select_best_route_candidate;
use crate::world::{y_to_floor, WorldState};

/// An office worker who lives in the tower, and their home ("x,y").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Local {
    pub who: DriverRef,
    pub home: String,
}

fn key_xy(key: &str) -> (i64, i64) {
    let mut it = key.split(',').map(|p| p.parse::<i64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0))
}

/// Hook at the day's end: the office jobs the tower's residents hold, given
/// to that many office workers, each with a home.
pub fn daily(world: &mut WorldState) {
    if !box_rules(world) {
        if !world.box_ext.locals.is_empty() {
            world.box_ext.locals.clear();
        }
        return;
    }
    let r = people::register(world);
    let office_jobs = r.jobs[0];
    let n = if r.jobs_total > 0 { (r.held_by_residents * office_jobs + r.jobs_total / 2) / r.jobs_total } else { 0 } as usize;
    let mut homes: Vec<String> = world
        .placed_objects
        .iter()
        .filter(|(_, o)| o.object_type_code == FAMILY_CONDO && o.unit_status <= 0x17)
        .map(|(k, _)| k.to_string())
        .collect();
    homes.sort_by_key(|k| fnv(&format!("home/{k}")));
    let mut workers: Vec<(i64, DriverRef)> = world
        .sims
        .iter()
        .filter(|s| s.family_code == FAMILY_OFFICE && (s.floor_anchor + s.home_column) % 4 != 1)
        .map(|s| {
            let d = DriverRef::of(s);
            (fnv(&format!("local/{}/{}/{}", s.floor_anchor, s.home_column, s.base_offset)), d)
        })
        .collect();
    workers.sort_by_key(|w| w.0);
    let locals: Vec<Local> = if homes.is_empty() {
        Vec::new()
    } else {
        workers
            .into_iter()
            .take(n)
            .enumerate()
            .map(|(i, (_, who))| Local { who, home: homes[i % homes.len()].clone() })
            .collect()
    };
    world.box_ext.locals = locals;
}

fn home_floor_of(world: &WorldState, sim_index: i64) -> Option<(i64, i64)> {
    if world.box_ext.locals.is_empty() {
        return None;
    }
    let sim = world.sims.get(sim_index as usize)?;
    let l = world.box_ext.locals.iter().find(|l| l.who.is(sim))?;
    let (_, y) = key_xy(&l.home);
    Some((y_to_floor(y), sim.floor_anchor))
}

fn way(world: &WorldState, from: i64, to: i64) -> bool {
    from == to || select_best_route_candidate(world, from, to, Some(true), None).is_some()
}

/// Hook in the office morning gate: the floor a resident worker sets off
/// from, their home's, if a way joins it to the office's.
pub fn arrival_floor(world: &WorldState, sim_index: i64) -> Option<i64> {
    let (home, office) = home_floor_of(world, sim_index)?;
    (home != office && way(world, home, office)).then_some(home)
}

/// Hook in the office evening departure: the floor a resident worker goes
/// home to.
pub fn exit_floor(world: &WorldState, sim_index: i64) -> Option<i64> {
    let (home, office) = home_floor_of(world, sim_index)?;
    (home != office && way(world, office, home)).then_some(home)
}

/// Hook in the office arrival: a resident worker going home is there.
pub fn is_exit(world: &WorldState, sim_index: i64, floor: i64) -> bool {
    home_floor_of(world, sim_index).is_some_and(|(home, _)| home == floor)
}

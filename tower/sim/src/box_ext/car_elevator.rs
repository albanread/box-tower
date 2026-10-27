//! BOX addition: the car elevator (docs/car-elevator.md has the rules).
//!
//! A BOX RULES feature: it can be built only with BOX RULES on, and stands
//! inert (no coverage, no traffic) if they are later turned off.
//!
//! A car elevator is a shaft CAR_ELEVATOR_WIDTH cells wide from the street
//! (the ground floor's row) straight down to a basement floor the player
//! chooses.  Its landing on every basement floor it passes is a parking
//! anchor, as a parking ramp is: the parking spaces it reaches by the ramp's
//! walk are covered.  Cars come to the tower along the street, wait their
//! turn at the shaft, ride its platform (one car at a time) down to the
//! floor of their bay, drive along the floor and park; when the driver
//! leaves the tower the car goes back the same way.
//!
//! Everything here is integer arithmetic on the saved state and uses no
//! random numbers (choices come from a hash of the driver's identity), so
//! the traffic is the same on every machine and survives save and load.
//!
//! The translated files call in at these hooks, each marked
//! `// BOX: car elevator` (docs/car-elevator.md lists them all, with the
//! state's field in world.rs and snapshot.rs):
//! - commands.rs handle_place_tile: `intercept_place` (builds a car
//!   elevator; refuses an overlay laid over one);
//! - commands.rs handle_remove_tile: `intercept_remove`;
//! - sims/parking.rs rebuild_parking_coverage: `coverage_anchors`;
//! - sims/parking.rs try_assign_parking_service: under BOX RULES,
//!   `assign_parking_box_rules` (the moment a person arrives by car);
//! - tick/service_idle_tasks.rs: `tick`, after the carrier tick;
//! - index.rs query_cell: `query`.
//!
//! Each returns at once, touching nothing, while BOX RULES are off (removal
//! excepted, so that an inert car elevator can still be pulled down).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::box_ext::{fnv, BoxExtState};
use crate::commands::{append_parking_coverage_patches, run_global_rebuilds, CellPatch, CommandResult};
use crate::ledger::LedgerState;
use crate::resources::{FAMILY_PARKING, FAMILY_PARKING_RAMP};
use crate::stress::add_delay::add_delay_to_current_sim;
use crate::stress::rebase_elapsed::rebase_sim_elapsed_from_clock;
use crate::time::TimeState;
use crate::world::{
    floor_to_y, y_to_floor, SimNotification, SimNotificationKind, SimRecord, WorldState, GRID_HEIGHT,
    GROUND_Y, UNDERGROUND_Y,
};

// ─── Rules ────────────────────────────────────────────────────────────────────

/// The tile type the PlaceTile command names, and the cells' tile type.
pub const CAR_ELEVATOR_TILE: &str = "carElevator";
/// Shaft width in cells: one car (CAR_LENGTH) with room either side; the
/// art (art/src/transport/NOTES.md, "Car lift") draws it 8 cells wide.
pub const CAR_ELEVATOR_WIDTH: i64 = 8;
/// A new car elevator: the street landing, the shaft and the B1 landing.
pub const CAR_ELEVATOR_COST: i64 = 100_000;
/// Each basement floor below B1 that the shaft reaches, built or extended.
pub const CAR_ELEVATOR_FLOOR_COST: i64 = 25_000;
/// Stars needed, as for parking spaces and ramps (enforced, as theirs is,
/// by the front end through `get_tile_star_requirement`).
pub const CAR_ELEVATOR_STAR_REQUIREMENT: i64 = 3;
/// At most this many car elevators in a tower.
pub const MAX_CAR_ELEVATORS: usize = 8;

/// Positions are in sixteenths of a cell (x) and of a floor (height).
pub const SUB: i64 = 16;
/// A car's length in cells (art/src/vehicles/NOTES.md: 24 units, 6 cells).
pub const CAR_LENGTH: i64 = 6;
/// A parking space's width in cells (resources.rs TILE_WIDTHS).
const PARKING_WIDTH: i64 = 4;
/// Speeds in sixteenths per tick.
pub const STREET_SPEED: i64 = 32; // 2 cells a tick along the street
pub const GARAGE_SPEED: i64 = 16; // 1 cell a tick on a basement floor, and onto the platform
pub const PLATFORM_SPEED: i64 = 4; // a quarter of a floor a tick
/// Ticks for the landing doors to open, and again to close.
pub const DOOR_TICKS: i64 = 4;
/// Gap in cells between cars waiting in line, and between the first car and
/// the shaft.
pub const QUEUE_GAP: i64 = 1;
/// A car leaves at the latest this many ticks (one game day) after it came.
pub const MAX_STAY_TICKS: i64 = 2600;
/// The vehicle pictures (art/src/vehicles), each with its own colour;
/// `Vehicle::style` indexes this.
pub const CAR_STYLES: &[&str] = &["saloon", "hatchback", "estate", "van", "taxi", "pickup"];

// BOX RULES only:
/// Cars waiting beyond this many in a line are backing up into the street:
/// their waiting counts double, and the tower is told once a day.
pub const STREET_QUEUE_LIMIT: i64 = 3;
/// A car that would join a line this long goes elsewhere (or away).
pub const LINE_MAX: i64 = 24;
/// BOX RULES: the cars a parking space holds (a space drawn stands for
/// several, as a drawn person stands for traffic).
pub const BAY_CARS: usize = 5;
/// A parking ramp's width in cells (TILE_WIDTHS' parkingRamp).
const RAMP_WIDTH: i64 = 15;
/// On a ramp a car goes down (or up) a sixteenth of a floor a tick.
const RAMP_SPEED: i64 = 1;
/// Stress (ticks, add_delay_to_current_sim) for a driver who finds no free
/// reachable bay, or is turned away.
pub const PARKING_DENIED_DELAY: i64 = 300;
/// `Vehicle::elevator` for a car parked by a ramp (no traffic drawn).
pub const RAMP: i64 = -1;
/// BOX RULES: ticks a car waits in its bay, once its driver is going, for
/// its people to come down and get in.
pub const GETTING_IN_TICKS: i64 = 60;
/// BOX RULES: what a car pays to park, a day.
pub const PARKING_FEE: i64 = 20;
/// Residents' cars come home from 17:00 (over the next three hours) and go
/// out from 7:15 (over the next two), in day ticks.
pub const RESIDENT_HOME_FROM: i64 = 1600;
pub const RESIDENT_OUT_FROM: i64 = 20;
pub const BACKED_UP_MESSAGE: &str = "Car elevator queue is backing up into the street";
pub const TURNED_AWAY_MESSAGE: &str = "Cars are being turned away at the car elevator";
pub const HOTEL_PARKING_MESSAGE: &str = "Hotel guests demand Parking";

/// The ground floor's floor index (the street).
const STREET_FLOOR: i64 = GRID_HEIGHT - 1 - GROUND_Y;
const LOBBY_FLOOR: i64 = STREET_FLOOR;

/// Driver states that mean the person has gone (or is going) home: the car
/// leaves.  0x05/0x45 departure, 0x24 hotel room vacated, 0x25/0x26/0x27
/// night park / parked.
const DRIVER_AWAY_STATES: &[i64] = &[0x05, 0x45, 0x24, 0x25, 0x26, 0x27];

// ─── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlatformPhase {
    /// Doors shut, nothing to do.
    Idle,
    /// Travelling to `target`, doors shut.
    Moving,
    Opening,
    /// Doors open: a car is driving on or off.
    Open,
    Closing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    /// Height in sixteenths of a floor (floor index * SUB).
    pub floor: i64,
    /// Where it is going, in the same units.
    pub target: i64,
    pub phase: PlatformPhase,
    /// 0 = doors shut .. DOOR_TICKS = open.
    pub door: i64,
    /// The vehicle it is working for (-1: none).
    pub serving: i64,
    /// The served vehicle is aboard.
    pub loaded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarElevator {
    /// Leftmost column; the shaft is CAR_ELEVATOR_WIDTH cells wide.  Also
    /// the car elevator's identity.
    pub column: i64,
    /// The deepest basement floor served (floor index: B1 = 9 .. B10 = 0).
    pub bottom_floor: i64,
    pub platform: Platform,
    /// Vehicles waiting for the platform, first come first served.
    pub queue: Vec<i64>,
    /// BOX RULES: ticks cars have waited for this platform, all told, and
    /// the cars that have boarded it.
    #[serde(default)]
    pub wait_total: i64,
    #[serde(default)]
    pub rides: i64,
    /// BOX RULES: the day the tower was last told the line is backing up.
    #[serde(default = "minus_one")]
    pub backed_up_day: i64,
}

fn minus_one() -> i64 {
    -1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VehiclePhase {
    /// Driving along the street to its place in the line at the shaft.
    Approaching,
    /// Stopped in the line at the shaft.
    Queued,
    /// Driving onto the platform (at the street or from its bay).
    Boarding,
    /// On the platform.
    Riding,
    /// Driving off the platform along the basement floor to its bay.
    ToBay,
    Parked,
    /// Parked, the driver gone: waiting for the platform.
    AwaitingLift,
    /// Driving off the platform along the street and away.
    Leaving,
    /// Out of its bay and on its way out: along the floor to the platform's
    /// landing, or up the ramps to the street.
    ToExit,
}

/// Which sim drives the car (sims/population.rs sim_key's four fields).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverRef {
    pub floor_anchor: i64,
    pub home_column: i64,
    pub family_code: i64,
    pub base_offset: i64,
}

impl DriverRef {
    pub(crate) fn of(sim: &SimRecord) -> Self {
        DriverRef {
            floor_anchor: sim.floor_anchor,
            home_column: sim.home_column,
            family_code: sim.family_code,
            base_offset: sim.base_offset,
        }
    }

    pub fn is(&self, sim: &SimRecord) -> bool {
        sim.floor_anchor == self.floor_anchor
            && sim.home_column == self.home_column
            && sim.family_code == self.family_code
            && sim.base_offset == self.base_offset
    }

    fn key(&self) -> String {
        format!("{}:{}:{}:{}", self.floor_anchor, self.home_column, self.family_code, self.base_offset)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vehicle {
    pub id: i64,
    /// The car elevator's column.
    pub elevator: i64,
    pub driver: DriverRef,
    /// The parking space's anchor key ("x,y"), its column and floor.
    pub bay: String,
    pub bay_x: i64,
    pub bay_floor: i64,
    /// Index into CAR_STYLES.
    pub style: i64,
    /// The street side it comes from and leaves by (-1 left, 1 right of the shaft).
    pub side: i64,
    /// The side of the shaft its bay is on.
    pub bay_side: i64,
    /// Left end of the car, in sixteenths of a cell.
    pub x: i64,
    /// Height, in sixteenths of a floor (floor index * SUB).
    pub floor: i64,
    /// The way the car faces (-1 left, 1 right).
    pub facing: i64,
    pub phase: VehiclePhase,
    /// On the way out (boarding or riding to leave).
    pub outbound: bool,
    /// time.totalTicks when it appeared.
    pub arrival_tick: i64,
    /// BOX RULES: ticks waited for the platform on this trip so far.
    #[serde(default)]
    pub waited: i64,
    /// The way still to drive, (x, floor) in sixteenths: in to the bay, or
    /// out of the car park.
    #[serde(default)]
    pub path: Vec<[i64; 2]>,
    /// The way in (from the platform's landing, or from the street at the
    /// top of the ramps) to the bay: driven back the other way to leave.
    #[serde(default)]
    pub route: Vec<[i64; 2]>,
    /// Where the car meets the platform (sixteenths of a floor), when that
    /// is not its bay's floor: it took the ramps on from there.
    #[serde(default)]
    pub lift_floor: Option<i64>,
    /// Parked in its bay: drawn nose out, as if reversed in.
    #[serde(default)]
    pub in_bay: bool,
    /// The tick its driver was seen to be going (0: not yet): it leaves
    /// GETTING_IN_TICKS later, its people in.
    #[serde(default)]
    pub going_since: i64,
    /// BOX RULES: a resident's car, home for the night, and its household's
    /// condominium ("x,y").
    #[serde(default, skip_serializing_if = "is_false")]
    pub resident: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub home: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

// ─── Small helpers ────────────────────────────────────────────────────────────

fn truthy(o: &crate::js::JsObject<String>, k: &str) -> bool {
    o.get(k).is_some_and(|v| !v.is_empty())
}

fn key_xy(key: &str) -> (i64, i64) {
    let mut it = key.split(',').map(|p| p.parse::<i64>().expect("an \"x,y\" key"));
    (it.next().unwrap(), it.next().unwrap())
}

fn rejected(reason: &str) -> CommandResult {
    CommandResult { accepted: false, patch: None, reason: Some(reason.to_string()), economy_changed: None }
}

fn move_toward(pos: i64, target: i64, speed: i64) -> i64 {
    if pos < target {
        (pos + speed).min(target)
    } else {
        (pos - speed).max(target)
    }
}

/// The rows a car elevator occupies, street first.
fn rows(bottom_floor: i64) -> std::ops::RangeInclusive<i64> {
    GROUND_Y..=floor_to_y(bottom_floor)
}

fn elevator_at(state: &BoxExtState, x: i64, y: i64) -> Option<usize> {
    state.car_elevators.iter().position(|e| {
        x >= e.column && x < e.column + CAR_ELEVATOR_WIDTH && y >= GROUND_Y && y <= floor_to_y(e.bottom_floor)
    })
}

/// Left end of a car standing in the middle of the platform.
fn platform_x(column: i64) -> i64 {
    column * SUB + (CAR_ELEVATOR_WIDTH - CAR_LENGTH) * SUB / 2
}

/// Left end of the first car waiting at the shaft on `side`.
fn wait_x(column: i64, side: i64) -> i64 {
    if side < 0 {
        (column - QUEUE_GAP - CAR_LENGTH) * SUB
    } else {
        (column + CAR_ELEVATOR_WIDTH + QUEUE_GAP) * SUB
    }
}

/// Just off the map on `side`: where cars appear and disappear.
fn street_end_x(width: i64, side: i64) -> i64 {
    if side < 0 {
        -CAR_LENGTH * SUB
    } else {
        width * SUB
    }
}

/// Left end of a car parked in the bay at `bay_x`: centred on the space
/// (a space is PARKING_WIDTH cells, a car CAR_LENGTH), but never into the
/// shaft.
fn parked_x(bay_x: i64, bay_side: i64, column: i64) -> i64 {
    let centred = bay_x * SUB + (PARKING_WIDTH - CAR_LENGTH) * SUB / 2;
    if bay_side < 0 {
        centred.min((column - CAR_LENGTH) * SUB)
    } else {
        centred.max((column + CAR_ELEVATOR_WIDTH) * SUB)
    }
}

/// The car is wholly outside the shaft's columns.
fn clear_of_shaft(x: i64, column: i64) -> bool {
    x + CAR_LENGTH * SUB <= column * SUB || x >= (column + CAR_ELEVATOR_WIDTH) * SUB
}

// ─── Coverage ─────────────────────────────────────────────────────────────────

/// The parking anchors a car elevator adds to rebuild_parking_coverage:
/// (x, y, right) of its landing on every basement floor it serves.
pub fn coverage_anchors(world: &WorldState) -> Vec<(i64, i64, i64)> {
    let mut out = Vec::new();
    if !world.box_ext.box_rules {
        return out; // inert with BOX RULES off
    }
    for e in &world.box_ext.car_elevators {
        for y in rows(e.bottom_floor) {
            if y >= UNDERGROUND_Y {
                out.push((e.column, y, e.column + CAR_ELEVATOR_WIDTH - 1));
            }
        }
    }
    out
}

/// A ramp stack: the ramps at column `x0`, from B1 down to row `bottom`
/// (each hangs from the one above, so the stack is unbroken).  Each ramp
/// takes cars down one floor, the way alternating: B1's from its left end
/// at the street to its right end on B1, B2's from there back to its left
/// end on B2, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stack {
    pub x0: i64,
    pub bottom: i64,
}

pub fn ramp_stacks(world: &WorldState) -> Vec<Stack> {
    let mut out = Vec::new();
    for (key, obj) in world.placed_objects.iter() {
        let (x0, y) = key_xy(key);
        if obj.object_type_code != FAMILY_PARKING_RAMP || y != UNDERGROUND_Y {
            continue;
        }
        let mut bottom = y;
        while world.cells.get(&format!("{x0},{}", bottom + 1)).is_some_and(|t| t == "parkingRamp")
            && world.placed_objects.get(&format!("{x0},{}", bottom + 1)).is_some()
        {
            bottom += 1;
        }
        out.push(Stack { x0, bottom });
    }
    out.sort_by_key(|s| s.x0);
    out
}

/// A car's left end at the top and at the foot of the ramp on row `y` of
/// stack `s`, in sixteenths of a cell.
fn ramp_ends(s: &Stack, y: i64) -> (i64, i64) {
    let l = (s.x0 + 1) * SUB;
    let r = (s.x0 + RAMP_WIDTH - 1 - CAR_LENGTH) * SUB;
    if (y - UNDERGROUND_Y) % 2 == 0 {
        (l, r)
    } else {
        (r, l)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ground {
    /// Drivable: a parking space, a ramp, bare basement floor (BOX RULES).
    Drive,
    Empty,
    Block,
}

fn ground(world: &WorldState, x: i64, y: i64, driveways: bool) -> Ground {
    if x < 0 || x >= world.width {
        return Ground::Block;
    }
    let k = format!("{x},{y}");
    let anchor = world.cell_to_anchor.get(&k).cloned().unwrap_or_else(|| k.clone());
    if world
        .placed_objects
        .get(&anchor)
        .is_some_and(|o| o.object_type_code == FAMILY_PARKING || o.object_type_code == FAMILY_PARKING_RAMP)
    {
        return Ground::Drive;
    }
    if driveways && world.cells.get(&k).is_some_and(|t| t == "floor") {
        return Ground::Drive;
    }
    if !truthy(&world.cells, &k) && !truthy(&world.cell_to_anchor, &k) {
        Ground::Empty
    } else {
        Ground::Block
    }
}

/// From cell `x` going `dir`, the last cell of row `y` a car reaches (across
/// gaps of at most three empty cells, as the coverage walk); `x - dir` if
/// none.
fn reach(world: &WorldState, y: i64, x: i64, dir: i64, driveways: bool) -> i64 {
    const MAX_EMPTY_GAP: i64 = 3;
    let mut last = x - dir;
    let mut c = x;
    loop {
        match ground(world, c, y, driveways) {
            Ground::Drive => {
                last = c;
                c += dir;
            }
            Ground::Empty => {
                let mut run = 0;
                while ground(world, c, y, driveways) == Ground::Empty && run <= MAX_EMPTY_GAP {
                    c += dir;
                    run += 1;
                }
                if run > MAX_EMPTY_GAP || ground(world, c, y, driveways) != Ground::Drive {
                    return last;
                }
            }
            Ground::Block => return last,
        }
    }
}

/// How a car gets to a parking space: its anchor key, column and floor;
/// `from_row` where it starts (a car elevator's landing row, or the street's
/// for the ramps from the street); `side` the side of the shaft it drives
/// off to (car elevators); `via` the ramp stack it goes down, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct BayRoute {
    pub key: String,
    pub bay_x: i64,
    pub bay_floor: i64,
    pub from_row: i64,
    pub side: i64,
    pub via: Option<Stack>,
}

fn bays_in(world: &WorldState, y: i64, lo: i64, hi: i64) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = world
        .placed_objects
        .iter()
        .filter(|(_, o)| o.object_type_code == FAMILY_PARKING)
        .map(|(k, _)| (k.to_string(), key_xy(k)))
        .filter(|(_, (x, by))| *by == y && *x >= lo && *x <= hi)
        .map(|(k, (x, _))| (k, x))
        .collect();
    out.sort_by_key(|b| b.1);
    out
}

/// Every row's span a car reaches from stack `s`'s foot on that row, from
/// row `first` down to its bottom, and the spaces in it.
fn down_the_stack(world: &WorldState, s: Stack, first: i64, driveways: bool) -> Vec<(i64, Vec<(String, i64)>)> {
    let mut out = Vec::new();
    for y in first..=s.bottom {
        let lo = reach(world, y, s.x0, -1, driveways);
        let hi = reach(world, y, s.x0, 1, driveways);
        out.push((y, bays_in(world, y, lo, hi)));
    }
    out
}

/// The parking spaces a car elevator's cars reach: along each basement
/// floor it serves, either side of the shaft, and (BOX RULES) down any ramp
/// stack they come to, floor by floor.  Each once, the nearest way first.
pub fn bay_routes(world: &WorldState, e: &CarElevator) -> Vec<BayRoute> {
    let driveways = crate::box_ext::box_rules(world);
    let stacks = if driveways { ramp_stacks(world) } else { Vec::new() };
    let mut out: Vec<BayRoute> = Vec::new();
    let mut via_ramps: Vec<BayRoute> = Vec::new();
    for y0 in rows(e.bottom_floor) {
        if y0 < UNDERGROUND_Y {
            continue;
        }
        for (side, start, dir) in [(-1, e.column - 1, -1), (1, e.column + CAR_ELEVATOR_WIDTH, 1)] {
            let end = reach(world, y0, start, dir, driveways);
            if end == start - dir {
                continue;
            }
            let (lo, hi) = if dir < 0 { (end, start) } else { (start, end) };
            for (key, bx) in bays_in(world, y0, lo, hi) {
                out.push(BayRoute { key, bay_x: bx, bay_floor: y_to_floor(y0), from_row: y0, side, via: None });
            }
            for s in &stacks {
                if y0 > s.bottom || s.x0 > hi || s.x0 + RAMP_WIDTH - 1 < lo {
                    continue;
                }
                for (y, bays) in down_the_stack(world, *s, y0 + 1, driveways) {
                    for (key, bx) in bays {
                        via_ramps.push(BayRoute { key, bay_x: bx, bay_floor: y_to_floor(y), from_row: y0, side, via: Some(*s) });
                    }
                }
            }
        }
    }
    for r in via_ramps {
        if !out.iter().any(|o| o.key == r.key) {
            out.push(r);
        }
    }
    let mut seen: Vec<String> = Vec::new();
    out.retain(|r| {
        let fresh = !seen.contains(&r.key);
        seen.push(r.key.clone());
        fresh
    });
    out
}

/// BOX RULES: the spaces cars reach from the street down the ramps alone.
pub fn street_routes(world: &WorldState) -> Vec<BayRoute> {
    let mut out: Vec<BayRoute> = Vec::new();
    for s in ramp_stacks(world) {
        for (y, bays) in down_the_stack(world, s, UNDERGROUND_Y, true) {
            for (key, bx) in bays {
                if !out.iter().any(|o| o.key == key) {
                    out.push(BayRoute { key, bay_x: bx, bay_floor: y_to_floor(y), from_row: GROUND_Y, side: 0, via: Some(s) });
                }
            }
        }
    }
    out
}

/// The parking spaces (anchor keys, with column and floor) a car elevator's
/// cars reach (bay_routes).
pub fn reachable_bays(world: &WorldState, e: &CarElevator) -> Vec<(String, i64, i64)> {
    bay_routes(world, e).into_iter().map(|r| (r.key, r.bay_x, r.bay_floor)).collect()
}

/// The way in for route `r`: (x, floor) waypoints in sixteenths from where
/// the car starts (off the platform, or at the top of the ramps) to its
/// bay.  Down a ramp the car drives across it as it drops a floor.
fn path_in(r: &BayRoute, column: i64) -> Vec<[i64; 2]> {
    let bf = r.bay_floor * SUB;
    let side = if r.side == 0 { 1 } else { r.side };
    let target = if r.via.is_none() {
        parked_x(r.bay_x, side, column)
    } else {
        r.bay_x * SUB + (PARKING_WIDTH - CAR_LENGTH) * SUB / 2
    };
    let Some(s) = r.via else { return vec![[target, bf]] };
    let first = r.from_row + 1;
    let bay_row = floor_to_y(r.bay_floor);
    let mut p = vec![[ramp_ends(&s, first).0, y_to_floor(r.from_row) * SUB]];
    for y in first..=bay_row {
        p.push([ramp_ends(&s, y).1, y_to_floor(y) * SUB]);
    }
    p.push([target, bf]);
    p
}

/// A commuter driving in (BOX RULES): held from the morning gate until
/// their car has parked, then on their way from its floor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commuter {
    pub driver: DriverRef,
    /// The office's key.
    pub facility: String,
    /// time.totalTicks when the car set off.
    pub since: i64,
}

/// The longest a commuter waits for their car to park before setting off
/// from the lobby after all.
pub const COMMUTE_WAIT_MAX: i64 = 600;

/// Hook in the office morning gate: hold sim `sim_index` while its car,
/// just sent, drives in (true), or not (false: BOX RULES off, no car).
pub fn hold_commuter(world: &mut WorldState, time: &TimeState, sim_index: i64, facility: &str) -> bool {
    if !crate::box_ext::box_rules(world) {
        return false;
    }
    let Some(sim) = world.sims.get(sim_index as usize) else { return false };
    let driver = DriverRef::of(sim);
    if !world.box_ext.vehicles.iter().any(|v| v.driver == driver && !v.in_bay && !v.outbound) {
        return false;
    }
    if !world.box_ext.commuters.iter().any(|c| c.driver == driver) {
        world.box_ext.commuters.push(Commuter { driver, facility: facility.to_string(), since: time.total_ticks });
    }
    true
}

/// Hook in the sims' refresh: a commuter driving in does nothing.
pub fn is_driving(world: &WorldState, sim_index: i64) -> bool {
    if world.box_ext.commuters.is_empty() {
        return false;
    }
    let Some(sim) = world.sims.get(sim_index as usize) else { return false };
    world.box_ext.commuters.iter().any(|c| c.driver.is(sim))
}

/// Hook after the car tick: each commuter whose car has parked sets off from
/// its floor (or, if no lift, stairs or escalator leaves that floor towards
/// the office, from the lobby, having walked up); one whose car is gone or
/// has taken too long sets off from the lobby.
pub fn commute(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    if world.box_ext.commuters.is_empty() {
        return;
    }
    let commuters = std::mem::take(&mut world.box_ext.commuters);
    for c in commuters {
        let Some(si) = world.sims.iter().position(|s| c.driver.is(s)) else { continue };
        let car = world.box_ext.vehicles.iter().find(|v| v.driver == c.driver);
        let source = match car {
            Some(v) if v.in_bay => Some(v.bay_floor),
            Some(_) if time.total_ticks - c.since < COMMUTE_WAIT_MAX => None,
            _ => Some(LOBBY_FLOOR),
        };
        let Some(mut source) = source else {
            world.box_ext.commuters.push(c);
            continue;
        };
        if world.placed_objects.get(&c.facility).is_none() {
            continue;
        }
        let to = world.sims[si].floor_anchor;
        if source != LOBBY_FLOOR
            && crate::route_scoring::select_candidate::select_best_route_candidate(
                world,
                source,
                to,
                Some(true),
                Some(world.sims[si].home_column),
            )
            .is_none()
        {
            source = LOBBY_FLOOR;
        }
        crate::sims::office::office_set_off(world, ledger, time, si as i64, &c.facility, source);
    }
}

/// Hook at the day's ledger rollover (BOX RULES): the day's parking fees,
/// PARKING_FEE a car that came to park.
pub fn daily(world: &mut WorldState, ledger: &mut LedgerState) {
    let n = std::mem::take(&mut world.box_ext.parked_today);
    if n > 0 && crate::box_ext::box_rules(world) {
        let paid = n * PARKING_FEE;
        ledger.cash_balance = (ledger.cash_balance + paid).min(ledger.cap());
        world.box_ext.parking_income += paid;
        world.box_ext.period_parking += paid;
    }
}

/// Beside the shaft on `side`, where a car waits for the platform below ground.
fn landing_x(column: i64, side: i64) -> i64 {
    if side < 0 {
        (column - CAR_LENGTH) * SUB
    } else {
        (column + CAR_ELEVATOR_WIDTH) * SUB
    }
}

/// The way out: the way in backwards, from the bay (its last point) to its
/// first, then on to `then` (beside the shaft, on that floor) if given.
fn way_out(route: &[[i64; 2]], then: Option<i64>) -> Vec<[i64; 2]> {
    let mut p: Vec<[i64; 2]> = route.iter().rev().skip(1).copied().collect();
    if let (Some(x), Some(first)) = (then, route.first()) {
        p.push([x, first[1]]);
    }
    p
}

/// Drive a step along `v.path`; true when it is driven.
fn follow(v: &mut Vehicle) -> bool {
    let Some(&[tx, tf]) = v.path.first() else { return true };
    if v.floor == tf {
        if tx != v.x {
            v.facing = if tx > v.x { 1 } else { -1 };
        }
        v.x = move_toward(v.x, tx, GARAGE_SPEED);
    } else {
        // on a ramp: across it as it climbs or drops, together
        let (rx, rf) = (tx - v.x, tf - v.floor);
        let steps = ((rx.abs() + GARAGE_SPEED / 2 - 1) / (GARAGE_SPEED / 2)).max(rf.abs() / RAMP_SPEED).max(1);
        if rx != 0 {
            v.facing = rx.signum();
        }
        v.x += rx / steps;
        v.floor += rf / steps;
        if steps == 1 {
            v.x = tx;
            v.floor = tf;
        }
    }
    if v.x == tx && v.floor == tf {
        v.path.remove(0);
    }
    v.path.is_empty()
}

// ─── Placement and removal ────────────────────────────────────────────────────

/// Overlays that could be laid over a car elevator's cells (they need only
/// a base tile), with their widths.
fn overlay_width(tile_type: &str) -> Option<i64> {
    match tile_type {
        "elevator" | "elevatorExpress" | "elevatorService" | "stairs" | "escalator" => {
            Some(crate::js::record_get(crate::resources::TILE_WIDTHS, tile_type).copied().unwrap_or(1))
        }
        _ => None,
    }
}

/// Hook in handle_place_tile, with the normalised tile type.  `Some` is the
/// command's result: a car elevator built (or refused), or an overlay
/// refused because it would lie over a car elevator.  `None`: not ours.
pub fn intercept_place(
    x: i64,
    y: i64,
    tile_type: &str,
    world: &mut WorldState,
    ledger: &mut LedgerState,
    free_build: bool,
) -> Option<CommandResult> {
    if tile_type == CAR_ELEVATOR_TILE {
        // A BOX addition: classic games are upstream's game, which has none.
        if !crate::box_ext::box_rules(world) {
            return Some(rejected("Invalid tile type"));
        }
        return Some(place(x, y, world, ledger, free_build));
    }
    if world.box_ext.car_elevators.is_empty() {
        return None;
    }
    let w = overlay_width(tile_type)?;
    if (0..w).any(|dx| elevator_at(&world.box_ext, x + dx, y).is_some()) {
        return Some(rejected("Cannot build over the car elevator"));
    }
    None
}

/// Build a car elevator whose platform reaches row `y` (a basement row), or
/// extend the one at column `x` down to it.
pub fn place(x: i64, y: i64, world: &mut WorldState, ledger: &mut LedgerState, free_build: bool) -> CommandResult {
    if x < 0 || x + CAR_ELEVATOR_WIDTH > world.width || y < 0 || y >= world.height {
        return rejected("Out of bounds");
    }
    if y < UNDERGROUND_Y {
        return rejected("Click the basement floor the car elevator should reach");
    }
    let bottom_floor = y_to_floor(y);

    // Extending: same column, deeper.
    let existing = world.box_ext.car_elevators.iter().position(|e| e.column == x);
    let (new_rows, cost): (Vec<i64>, i64) = match existing {
        Some(i) => {
            let old_bottom = world.box_ext.car_elevators[i].bottom_floor;
            if bottom_floor >= old_bottom {
                return rejected("The car elevator already reaches that floor");
            }
            let from = floor_to_y(old_bottom) + 1;
            ((from..=y).collect(), (old_bottom - bottom_floor) * CAR_ELEVATOR_FLOOR_COST)
        }
        None => {
            if world.box_ext.car_elevators.len() >= MAX_CAR_ELEVATORS {
                return rejected("Too many car elevators in this tower");
            }
            let below_b1 = (STREET_FLOOR - 1) - bottom_floor;
            ((GROUND_Y..=y).collect(), CAR_ELEVATOR_COST + below_b1 * CAR_ELEVATOR_FLOOR_COST)
        }
    };

    if !free_build && cost > ledger.cash_balance {
        return rejected("Insufficient funds");
    }
    let mut floor_to_replace: Vec<String> = Vec::new();
    for &row in &new_rows {
        for dx in 0..CAR_ELEVATOR_WIDTH {
            let key = format!("{},{}", x + dx, row);
            if truthy(&world.overlays, &key) || truthy(&world.overlay_to_anchor, &key) {
                return rejected("Cell already has an overlay");
            }
            if truthy(&world.cell_to_anchor, &key) {
                return rejected("Cell already occupied");
            }
            match world.cells.get(&key).filter(|s| !s.is_empty()) {
                None => {}
                Some(t) if t == "floor" => floor_to_replace.push(key),
                Some(_) => return rejected("Cell already occupied"),
            }
        }
    }

    // Apply.
    for key in &floor_to_replace {
        world.cells.remove(key);
    }
    let mut patch: Vec<CellPatch> = Vec::new();
    for &row in &new_rows {
        world.cells.insert(format!("{x},{row}"), CAR_ELEVATOR_TILE.to_string());
        for dx in 1..CAR_ELEVATOR_WIDTH {
            world.cells.insert(format!("{},{}", x + dx, row), CAR_ELEVATOR_TILE.to_string());
            world.cell_to_anchor.insert(format!("{},{}", x + dx, row), format!("{x},{row}"));
        }
        for dx in 0..CAR_ELEVATOR_WIDTH {
            patch.push(CellPatch::new(x + dx, row, CAR_ELEVATOR_TILE, dx == 0));
        }
    }
    match existing {
        Some(i) => world.box_ext.car_elevators[i].bottom_floor = bottom_floor,
        None => world.box_ext.car_elevators.push(CarElevator {
            column: x,
            bottom_floor,
            platform: Platform {
                floor: STREET_FLOOR * SUB,
                target: STREET_FLOOR * SUB,
                phase: PlatformPhase::Idle,
                door: 0,
                serving: -1,
                loaded: false,
            },
            queue: Vec::new(),
            wait_total: 0,
            rides: 0,
            backed_up_day: -1,
        }),
    }
    if !free_build {
        ledger.cash_balance -= cost;
    }

    run_global_rebuilds(world, ledger);
    append_parking_coverage_patches(world, &mut patch);
    CommandResult { accepted: true, patch: Some(patch), reason: None, economy_changed: Some(cost > 0) }
}

/// Hook in place_tile's support check (BOX RULES only): an underground tile
/// whose cells above are not all built still stands when a built cell is
/// beside it on its own row -- the basement dug out sideways, under the
/// street, as far as a car elevator's landings.
pub fn side_support(world: &WorldState, x: i64, y: i64, width: i64) -> bool {
    let built = |cx: i64| truthy(&world.cells, &format!("{cx},{y}"));
    crate::box_ext::box_rules(world) && y >= UNDERGROUND_Y && (built(x - 1) || built(x + width))
}

/// Hook in handle_remove_tile: a click on any of a car elevator's cells
/// removes the whole of it, and the vehicles that use it.  `None`: not ours.
pub fn intercept_remove(x: i64, y: i64, world: &mut WorldState, ledger: &mut LedgerState) -> Option<CommandResult> {
    let i = elevator_at(&world.box_ext, x, y)?;
    let e = world.box_ext.car_elevators.remove(i);
    world.box_ext.vehicles.retain(|v| v.elevator != e.column);
    if world.box_ext.car_elevators.is_empty() && world.box_ext.vehicles.is_empty() {
        world.box_ext.next_vehicle_id = 0;
    }

    let cols = e.column..e.column + CAR_ELEVATOR_WIDTH;
    let bottom_y = floor_to_y(e.bottom_floor);
    for row in rows(e.bottom_floor) {
        for cx in cols.clone() {
            let key = format!("{cx},{row}");
            world.cells.remove(&key);
            world.cell_to_anchor.remove(&key);
        }
    }

    // A row becomes floor where something still needs it: tiles either side
    // on the row (as handle_remove_tile's rule), something standing on the
    // street row, or something hanging below a basement row.
    let built = |world: &WorldState, cx: i64, row: i64| truthy(&world.cells, &format!("{cx},{row}"));
    let mut patch: Vec<CellPatch> = Vec::new();
    let mut below_is_floor = false;
    for row in rows(e.bottom_floor).rev() {
        let has_left = (0..e.column).any(|cx| built(world, cx, row));
        let has_right = (e.column + CAR_ELEVATOR_WIDTH..world.width).any(|cx| built(world, cx, row));
        let floor = if row >= UNDERGROUND_Y {
            (has_left && has_right)
                || below_is_floor
                || (row == bottom_y && cols.clone().any(|cx| built(world, cx, row + 1)))
        } else {
            (has_left && has_right) || cols.clone().any(|cx| built(world, cx, row - 1))
        };
        below_is_floor = floor;
        for cx in cols.clone() {
            if floor {
                world.cells.insert(format!("{cx},{row}"), "floor".to_string());
            }
            patch.push(CellPatch::new(cx, row, if floor { "floor" } else { "empty" }, true));
        }
    }

    run_global_rebuilds(world, ledger);
    append_parking_coverage_patches(world, &mut patch);
    Some(CommandResult { accepted: true, patch: Some(patch), reason: None, economy_changed: None })
}

// ─── Arrivals ─────────────────────────────────────────────────────────────────

/// A car for `driver`, bound for route `r`'s bay by the car elevator at
/// `column` (it appears at the street end and joins the line), or by the
/// ramps (`column` RAMP: it drives along the street to the top of them, and
/// down).
fn spawn_vehicle(world: &mut WorldState, time: &TimeState, driver: DriverRef, column: i64, r: &BayRoute) {
    let look = fnv(&driver.key());
    let route = path_in(r, column);
    let (side, bay_side, lift_floor) = if column == RAMP {
        let top = route[0][0];
        (if top * 2 < world.width * SUB { -1 } else { 1 }, 0, None)
    } else {
        (
            if column * 2 + CAR_ELEVATOR_WIDTH < world.width { -1 } else { 1 },
            r.side,
            r.via.map(|_| y_to_floor(r.from_row) * SUB),
        )
    };
    let x = street_end_x(world.width, side);
    let state = &mut world.box_ext;
    let id = state.next_vehicle_id;
    state.next_vehicle_id += 1;
    state.parked_today += 1;
    state.vehicles.push(Vehicle {
        id,
        elevator: column,
        driver,
        bay: r.key.clone(),
        bay_x: r.bay_x,
        bay_floor: r.bay_floor,
        style: look % CAR_STYLES.len() as i64,
        side,
        bay_side,
        x,
        floor: STREET_FLOOR * SUB,
        facing: -side,
        phase: VehiclePhase::Approaching,
        outbound: false,
        arrival_tick: time.total_ticks,
        waited: 0,
        path: Vec::new(),
        route,
        lift_floor,
        in_bay: false,
        going_since: 0,
        resident: false,
        home: String::new(),
    });
    if let Some(e) = state.car_elevators.iter_mut().find(|e| e.column == column) {
        e.queue.push(id);
    }
}

/// Cars waiting in the street line of the car elevator at `column`.
fn street_line(state: &BoxExtState, column: i64) -> i64 {
    state
        .vehicles
        .iter()
        .filter(|v| {
            v.elevator == column
                && !v.outbound
                && matches!(v.phase, VehiclePhase::Approaching | VehiclePhase::Queued)
        })
        .count() as i64
}

/// BOX RULES' try_assign_parking_service (the hook calls it instead of the
/// classic body).  Parking that makes sense: a reachable (covered) parking
/// space serves the driver.  A free bay a ramp reaches is taken at once (the
/// car simply parks); otherwise the car elevator with the shortest line that
/// reaches a free bay takes the car, unless its line is LINE_MAX long (the
/// car is turned away).  Met: the driver's elapsed time is rebased from the
/// clock as upstream's success path does, and true.  Not met: the driver
/// takes PARKING_DENIED_DELAY of stress, the tower is told (the office
/// caller tells it itself), and false.  Draws no random numbers.
pub fn assign_parking_box_rules(world: &mut WorldState, time: &TimeState, sim_index: i64) -> bool {
    let si = sim_index as usize;
    let Some(sim) = world.sims.get(si) else { return false };
    let driver = DriverRef::of(sim);
    let family = sim.family_code;
    let met = |world: &mut WorldState| {
        rebase_sim_elapsed_from_clock(&mut world.sims[si], time);
        true
    };
    if world.box_ext.vehicles.iter().any(|v| v.driver == driver) {
        return met(world);
    }
    match park(world, time, driver) {
        Ok(()) => met(world),
        Err(turned_away) => {
            add_delay_to_current_sim(&mut world.sims[si], PARKING_DENIED_DELAY);
            let message = if turned_away {
                Some(TURNED_AWAY_MESSAGE)
            } else if family != crate::resources::FAMILY_OFFICE {
                Some(HOTEL_PARKING_MESSAGE)
            } else {
                None
            };
            if let Some(m) = message {
                world
                    .pending_notifications
                    .push(SimNotification { kind: SimNotificationKind::RouteFailure, message: Some(m.to_string()) });
            }
            false
        }
    }
}

/// A free space for `driver`'s car, and the car sent to it (Ok), or none
/// (Err: true when the car elevators' lines are too long, false when the
/// spaces are full).
fn park(world: &mut WorldState, time: &TimeState, driver: DriverRef) -> Result<(), bool> {
    // a space holds BAY_CARS cars
    let full = |world: &WorldState, key: &str| world.box_ext.vehicles.iter().filter(|v| v.bay == key).count() >= BAY_CARS;

    // Every bay a car elevator reaches, by elevator.
    let mut by_elevator: Vec<(i64, Vec<BayRoute>)> = Vec::new();
    let mut reached: Vec<String> = Vec::new();
    for e in &world.box_ext.car_elevators {
        let mut free = Vec::new();
        for r in bay_routes(world, e) {
            if reached.contains(&r.key) {
                continue;
            }
            reached.push(r.key.clone());
            if !full(world, &r.key) {
                free.push(r);
            }
        }
        by_elevator.push((e.column, free));
    }

    // Ramp bays: spaces the ramps reach from the street that no car
    // elevator does.  Cars drive down to them at once (no line).
    let mut ramp_free: Vec<BayRoute> = street_routes(world)
        .into_iter()
        .filter(|r| !reached.contains(&r.key) && !full(world, &r.key))
        .collect();
    let pick = |n: usize| (fnv(&format!("{}/{}", driver.key(), time.day_counter)) % n as i64) as usize;
    if !ramp_free.is_empty() {
        let r = ramp_free.swap_remove(pick(ramp_free.len()));
        spawn_vehicle(world, time, driver, RAMP, &r);
        return Ok(());
    }

    let mut best: Option<(i64, i64)> = None; // (line, column)
    let mut turned_away = false;
    for (column, free) in &by_elevator {
        if free.is_empty() {
            continue;
        }
        let line = street_line(&world.box_ext, *column);
        if line >= LINE_MAX {
            turned_away = true;
            continue;
        }
        if best.is_none_or(|(l, _)| line < l) {
            best = Some((line, *column));
        }
    }
    if let Some((_, column)) = best {
        let free = &mut by_elevator.iter_mut().find(|(c, _)| *c == column).unwrap().1;
        let r = free.swap_remove(pick(free.len()));
        spawn_vehicle(world, time, driver, column, &r);
        return Ok(());
    }
    Err(turned_away)
}

// ─── Running costs (BOX RULES) ────────────────────────────────────────────────

/// A parking space's upkeep every three days under BOX RULES: less than its
/// five cars' fees for the three days (BAY_CARS * PARKING_FEE * 3 = $300),
/// so that a car park in use pays its way.  Upstream charges a space as it
/// charges a lobby cell, by the star rating ($40,000 a space at four
/// stars), which no fee could meet.
pub const SPACE_UPKEEP: i64 = 100;
/// A car elevator's running cost every three days, a standard lift car's.
pub const CAR_ELEVATOR_UPKEEP: i64 = 10_000;

/// Hook in do_expense_sweep's parking branch: under BOX RULES, a parking
/// space's upkeep (None: a lobby cell, or BOX RULES off).
pub fn space_upkeep(world: &WorldState, key: &str) -> Option<i64> {
    if !crate::box_ext::box_rules(world) || world.cells.get(key).map(String::as_str) != Some("parking") {
        return None;
    }
    Some(SPACE_UPKEEP)
}

/// Hook at the end of do_expense_sweep's lifts: under BOX RULES each car
/// elevator's running cost, with the lifts' (expense family 1).
pub fn upkeep(ledger: &mut LedgerState, world: &WorldState) {
    if !crate::box_ext::box_rules(world) || world.box_ext.car_elevators.is_empty() {
        return;
    }
    let amount = CAR_ELEVATOR_UPKEEP * world.box_ext.car_elevators.len() as i64;
    ledger.cash_balance = 0.max(ledger.cash_balance - amount);
    ledger.expense_ledger[1] += amount;
}

// ─── Residents' cars (BOX RULES) ──────────────────────────────────────────────

/// When a resident's car comes home (from 17:00) and goes out again (from
/// 7:15), in day ticks, by its household's place.
fn home_tick(key: &str) -> i64 {
    RESIDENT_HOME_FROM + fnv(&format!("home/{key}")) % 300
}
fn out_tick(key: &str) -> i64 {
    RESIDENT_OUT_FROM + fnv(&format!("out/{key}")) % 180
}

/// The households with a car: half the occupied condominiums, by their
/// place, and the sim who drives (the household's first).
fn car_households(world: &WorldState) -> Vec<(String, DriverRef)> {
    let mut out = Vec::new();
    for (key, o) in world.placed_objects.iter() {
        if o.object_type_code != crate::resources::FAMILY_CONDO || o.unit_status > 0x17 || fnv(&format!("car/{key}")) % 2 != 0 {
            continue;
        }
        let (x, y) = key_xy(key);
        let floor = y_to_floor(y);
        if let Some(s) = world.sims.iter().find(|s| {
            s.family_code == crate::resources::FAMILY_CONDO && s.floor_anchor == floor && s.home_column == x && s.base_offset == 0
        }) {
            out.push((key.to_string(), DriverRef::of(s)));
        }
    }
    out
}

/// Hook after the commuters (service_idle_tasks): a resident's car comes
/// home in the evening, if there is a space; if not, the household is the
/// less happy for it, as an office worker turned away is.
pub fn residents(world: &mut WorldState, time: &TimeState) {
    if !crate::box_ext::box_rules(world) || world.box_ext.car_elevators.is_empty() && !world.placed_objects.values().any(|o| o.object_type_code == crate::resources::FAMILY_PARKING_RAMP) {
        return;
    }
    if !(RESIDENT_HOME_FROM..RESIDENT_HOME_FROM + 300).contains(&time.day_tick) {
        return;
    }
    for (key, driver) in car_households(world) {
        if home_tick(&key) != time.day_tick || world.box_ext.vehicles.iter().any(|v| v.driver == driver) {
            continue;
        }
        match park(world, time, driver.clone()) {
            Ok(()) => {
                if let Some(v) = world.box_ext.vehicles.last_mut() {
                    v.resident = true;
                    v.home = key;
                }
            }
            Err(_) => {
                if let Some(s) = world.sims.iter_mut().find(|s| driver.is(s)) {
                    add_delay_to_current_sim(s, PARKING_DENIED_DELAY);
                }
            }
        }
    }
}

// ─── The tick ─────────────────────────────────────────────────────────────────

/// Take a vehicle off its elevator's queue and platform.
fn release(e: &mut CarElevator, id: i64) {
    e.queue.retain(|&q| q != id);
    if e.platform.serving == id {
        e.platform.serving = -1;
        e.platform.loaded = false;
        e.platform.phase = if e.platform.door > 0 { PlatformPhase::Closing } else { PlatformPhase::Idle };
    }
}

fn driver_away(sims: &[SimRecord], d: &DriverRef) -> bool {
    match sims.iter().find(|s| d.is(s)) {
        None => true,
        Some(s) => DRIVER_AWAY_STATES.contains(&s.state_code),
    }
}

/// BOX RULES: the car's wait becomes its driver's stress (the
/// add_delay_to_current_sim path a queue-full lift wait uses), and the
/// platform's tally.
fn charge_wait(sims: &mut [SimRecord], v: &mut Vehicle, e: &mut CarElevator) {
    e.wait_total += v.waited;
    e.rides += 1;
    if v.waited > 0 {
        if let Some(s) = sims.iter_mut().find(|s| v.driver.is(s)) {
            add_delay_to_current_sim(s, v.waited);
        }
    }
    v.waited = 0;
}

/// Hook at the end of service_idle_tasks: one tick of the platforms and the
/// traffic.  Order: bays and drivers checked, platforms moved, vehicles
/// moved (in arrival order), vehicles gone dropped.
pub fn tick(world: &mut WorldState, time: &TimeState) {
    if world.box_ext.is_empty() || !world.box_ext.box_rules {
        return; // nothing built, or (BOX RULES turned off) inert
    }
    let WorldState { box_ext, sims, placed_objects, width, pending_notifications, .. } = world;
    let width = *width;
    let BoxExtState { car_elevators, vehicles, box_rules, .. } = box_ext;
    let rules = *box_rules;

    // 1. A bay demolished takes its car with it; a car whose driver has gone
    //    asks for the platform.
    let mut gone: Vec<i64> = Vec::new();
    for v in vehicles.iter_mut() {
        let bay_ok = placed_objects.get(&v.bay).is_some_and(|o| o.object_type_code == FAMILY_PARKING);
        let leaving = if v.resident {
            // a resident's car goes out on a weekday morning (offices are
            // shut at the weekend, and so are its people); it is home, so
            // MAX_STAY_TICKS is not for it
            time.weekend_flag == 0 && time.day_tick == out_tick(&v.home)
        } else {
            driver_away(sims, &v.driver) || time.total_ticks - v.arrival_tick >= MAX_STAY_TICKS
        };
        if v.phase == VehiclePhase::Parked && v.going_since == 0 && leaving {
            v.going_since = time.total_ticks.max(1);
        }
        let due_out = v.phase == VehiclePhase::Parked
            && v.going_since > 0
            && time.total_ticks - v.going_since >= GETTING_IN_TICKS;
        if v.elevator == RAMP {
            // A ramp's car: back up the ramps to the street when its driver
            // goes; gone at once when its bay is.
            if !bay_ok {
                gone.push(v.id);
            } else if due_out {
                v.in_bay = false;
                v.outbound = true;
                v.path = way_out(&v.route, None);
                v.phase = VehiclePhase::ToExit;
            }
            continue;
        }
        let Some(e) = car_elevators.iter_mut().find(|e| e.column == v.elevator) else {
            gone.push(v.id);
            continue;
        };
        if !bay_ok {
            release(e, v.id);
            gone.push(v.id);
            continue;
        }
        if due_out {
            v.outbound = true;
            if v.lift_floor.is_none() {
                // its bay is on the platform's floor: it waits in the bay
                v.phase = VehiclePhase::AwaitingLift;
                e.queue.push(v.id);
            } else {
                v.in_bay = false;
                v.path = way_out(&v.route, Some(landing_x(e.column, v.bay_side)));
                v.phase = VehiclePhase::ToExit;
            }
        }
    }
    vehicles.retain(|v| !gone.contains(&v.id));

    // 2. Platforms.
    for e in car_elevators.iter_mut() {
        let p = &mut e.platform;
        match p.phase {
            PlatformPhase::Idle => {
                if let Some(&head) = e.queue.first() {
                    if let Some(v) = vehicles.iter().find(|v| v.id == head) {
                        p.serving = head;
                        p.loaded = false;
                        p.target = if v.outbound { v.lift_floor.unwrap_or(v.bay_floor * SUB) } else { STREET_FLOOR * SUB };
                        p.phase = if p.floor == p.target { PlatformPhase::Opening } else { PlatformPhase::Moving };
                    }
                }
            }
            PlatformPhase::Moving => {
                p.floor = move_toward(p.floor, p.target, PLATFORM_SPEED);
                if p.floor == p.target {
                    p.phase = PlatformPhase::Opening;
                }
            }
            PlatformPhase::Opening => {
                p.door += 1;
                if p.door >= DOOR_TICKS {
                    p.door = DOOR_TICKS;
                    p.phase = PlatformPhase::Open;
                }
            }
            PlatformPhase::Open => {
                if p.serving < 0 {
                    p.phase = PlatformPhase::Closing;
                }
            }
            PlatformPhase::Closing => {
                p.door -= 1;
                if p.door <= 0 {
                    p.door = 0;
                    if p.loaded {
                        let v = vehicles.iter().find(|v| v.id == p.serving).expect("the loaded vehicle");
                        p.target = if v.outbound { STREET_FLOOR * SUB } else { v.lift_floor.unwrap_or(v.bay_floor * SUB) };
                        p.phase = PlatformPhase::Moving;
                    } else {
                        p.phase = PlatformPhase::Idle;
                    }
                }
            }
        }
    }

    // Each waiting car's place in its elevator's line (0 = at the shaft).
    let mut slot: BTreeMap<i64, i64> = BTreeMap::new();
    for e in car_elevators.iter() {
        let mut k = 0;
        for id in &e.queue {
            if let Some(v) = vehicles.iter().find(|v| v.id == *id) {
                if !v.outbound && matches!(v.phase, VehiclePhase::Approaching | VehiclePhase::Queued) {
                    slot.insert(*id, k);
                    k += 1;
                }
            }
        }
        // BOX RULES: a line past the street entrance is reported, once a day.
        if rules && k > STREET_QUEUE_LIMIT && e.backed_up_day != time.day_counter {
            pending_notifications.push(SimNotification {
                kind: SimNotificationKind::RouteFailure,
                message: Some(BACKED_UP_MESSAGE.to_string()),
            });
        }
    }
    if rules {
        for e in car_elevators.iter_mut() {
            let k = slot.iter().filter(|(id, _)| vehicles.iter().any(|v| v.id == **id && v.elevator == e.column)).count();
            if k as i64 > STREET_QUEUE_LIMIT {
                e.backed_up_day = time.day_counter;
            }
        }
    }

    // 3. Vehicles.
    let mut gone: Vec<i64> = Vec::new();
    for v in vehicles.iter_mut() {
        if v.elevator == RAMP {
            // along the street to the top of the ramps, down them to the
            // bay; out the same way, and off along the street
            match v.phase {
                VehiclePhase::Approaching => {
                    let top = v.route.first().map(|w| w[0]).unwrap_or(v.x);
                    v.x = move_toward(v.x, top, STREET_SPEED);
                    if v.x == top {
                        v.path = v.route.clone();
                        v.phase = VehiclePhase::ToBay;
                    }
                }
                VehiclePhase::ToBay => {
                    if follow(v) {
                        v.phase = VehiclePhase::Parked;
                        v.in_bay = true;
                    }
                }
                VehiclePhase::ToExit => {
                    if follow(v) {
                        v.phase = VehiclePhase::Leaving;
                        v.facing = v.side;
                    }
                }
                VehiclePhase::Leaving => {
                    let end = street_end_x(width, v.side);
                    v.x = move_toward(v.x, end, STREET_SPEED);
                    if v.x == end {
                        gone.push(v.id);
                    }
                }
                _ => {}
            }
            continue;
        }
        let e = car_elevators.iter_mut().find(|e| e.column == v.elevator).expect("checked above");
        // BOX RULES: waiting for the platform is waiting, as for a lift: a
        // tick a tick, two past the street entrance; it becomes the driver's
        // stress when the car boards.
        if rules {
            match v.phase {
                VehiclePhase::Queued => {
                    v.waited += if slot.get(&v.id).copied().unwrap_or(0) >= STREET_QUEUE_LIMIT { 2 } else { 1 };
                }
                VehiclePhase::AwaitingLift => v.waited += 1,
                _ => {}
            }
        }
        let p_ready_here = e.platform.serving == v.id
            && e.platform.phase == PlatformPhase::Open
            && e.platform.floor == v.floor;
        match v.phase {
            VehiclePhase::Approaching | VehiclePhase::Queued => {
                let k = slot.get(&v.id).copied().unwrap_or(0);
                let end = street_end_x(width, v.side);
                let mut target = wait_x(e.column, v.side) + v.side * k * (CAR_LENGTH + QUEUE_GAP) * SUB;
                target = if v.side < 0 { target.max(end) } else { target.min(end) };
                v.x = move_toward(v.x, target, STREET_SPEED);
                v.phase = if v.x == target { VehiclePhase::Queued } else { VehiclePhase::Approaching };
                if v.phase == VehiclePhase::Queued && k == 0 && p_ready_here {
                    v.phase = VehiclePhase::Boarding;
                    if rules {
                        charge_wait(sims, v, e);
                    }
                }
            }
            VehiclePhase::AwaitingLift => {
                if p_ready_here {
                    v.phase = VehiclePhase::Boarding;
                    v.in_bay = false;
                    v.facing = -v.bay_side;
                    if rules {
                        charge_wait(sims, v, e);
                    }
                }
            }
            VehiclePhase::Boarding => {
                let target = platform_x(e.column);
                v.x = move_toward(v.x, target, GARAGE_SPEED);
                if v.x == target {
                    v.phase = VehiclePhase::Riding;
                    e.platform.loaded = true;
                    e.platform.phase = PlatformPhase::Closing;
                }
            }
            VehiclePhase::Riding => {
                v.floor = e.platform.floor;
                if e.platform.phase == PlatformPhase::Open && e.platform.loaded {
                    e.platform.loaded = false;
                    if v.outbound {
                        v.phase = VehiclePhase::Leaving;
                        v.facing = v.side;
                    } else {
                        v.phase = VehiclePhase::ToBay;
                        v.facing = v.bay_side;
                        v.path = v.route.clone();
                    }
                }
            }
            VehiclePhase::ToBay => {
                if v.path.is_empty() {
                    // (a car saved before routes: straight to its bay)
                    v.path = vec![[parked_x(v.bay_x, v.bay_side, e.column), v.floor]];
                }
                let done = follow(v);
                if e.platform.serving == v.id && clear_of_shaft(v.x, e.column) {
                    release(e, v.id);
                }
                if done {
                    v.phase = VehiclePhase::Parked;
                    v.in_bay = true;
                }
            }
            VehiclePhase::ToExit => {
                if follow(v) {
                    v.phase = VehiclePhase::AwaitingLift;
                    v.facing = -v.bay_side;
                    e.queue.push(v.id);
                }
            }
            VehiclePhase::Leaving => {
                let end = street_end_x(width, v.side);
                v.x = move_toward(v.x, end, STREET_SPEED);
                if e.platform.serving == v.id && clear_of_shaft(v.x, e.column) {
                    release(e, v.id);
                }
                if v.x == end {
                    if e.platform.serving == v.id {
                        release(e, v.id);
                    }
                    gone.push(v.id);
                }
            }
            VehiclePhase::Parked => {}
        }
    }
    vehicles.retain(|v| !gone.contains(&v.id));
}

// ─── What the presenter and the properties pane read ─────────────────────────

/// A vehicle as the presenter draws it.  `x` is the car's left end in cells
/// (fractional), `floor` its floor index (fractional while riding; ground =
/// 10, B1 = 9), `row` the same as a grid row (y), `length` in cells.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VehicleView {
    pub id: i64,
    pub x: f64,
    pub floor: f64,
    pub row: f64,
    pub length: f64,
    pub facing: i64,
    pub phase: VehiclePhase,
    /// 0: the inbound lane (arriving and waiting), 1: the outbound lane
    /// (leaving), on the street; 0 below ground.
    pub lane: i64,
    /// A CAR_STYLES name ("saloon", ...).
    pub style: String,
    /// Parked in its bay (drawn nose out in the space); the space's left
    /// cell and its row.
    #[serde(default)]
    pub in_bay: bool,
    #[serde(default)]
    pub bay_x: f64,
    #[serde(default)]
    pub bay_row: f64,
    /// Its driver is going: its people are walking down to it.
    #[serde(default)]
    pub going: bool,
}

/// A car elevator's platform and doors as the presenter draws them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformView {
    pub column: i64,
    pub width: i64,
    pub bottom_floor: i64,
    pub floor: f64,
    pub row: f64,
    pub phase: PlatformPhase,
    /// 0 = shut .. 1 = open, the landing doors at `floor`.
    pub door_open: f64,
    pub loaded: bool,
    /// Cars waiting in the street line.
    pub line: i64,
    /// The line reaches past the street entrance (STREET_QUEUE_LIMIT).
    pub backed_up: bool,
}

fn sub(v: i64) -> f64 {
    v as f64 / SUB as f64
}

pub fn vehicle_views(state: &BoxExtState) -> Vec<VehicleView> {
    state
        .vehicles
        .iter()
        .map(|v| VehicleView {
            id: v.id,
            x: sub(v.x),
            floor: sub(v.floor),
            row: (GRID_HEIGHT - 1) as f64 - sub(v.floor),
            length: CAR_LENGTH as f64,
            facing: v.facing,
            phase: v.phase,
            lane: if v.phase == VehiclePhase::Leaving { 1 } else { 0 },
            style: CAR_STYLES[v.style as usize].to_string(),
            in_bay: v.in_bay,
            bay_x: v.bay_x as f64,
            bay_row: floor_to_y(v.bay_floor) as f64,
            going: v.going_since > 0,
        })
        .collect()
}

pub fn platform_views(state: &BoxExtState) -> Vec<PlatformView> {
    state
        .car_elevators
        .iter()
        .map(|e| PlatformView {
            column: e.column,
            width: CAR_ELEVATOR_WIDTH,
            bottom_floor: e.bottom_floor,
            floor: sub(e.platform.floor),
            row: (GRID_HEIGHT - 1) as f64 - sub(e.platform.floor),
            phase: e.platform.phase,
            door_open: e.platform.door as f64 / DOOR_TICKS as f64,
            loaded: e.platform.loaded,
            line: street_line(state, e.column),
            backed_up: street_line(state, e.column) > STREET_QUEUE_LIMIT,
        })
        .collect()
}

/// queryCell's extra for a car elevator's cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarElevatorInfo {
    pub column: i64,
    pub width: i64,
    /// Floor indices served, street first (10, 9, ...).
    pub served_floors: Vec<i64>,
    pub bottom_floor: i64,
    pub platform: PlatformView,
    pub queue_length: i64,
    pub vehicles: i64,
    pub bays_reached: i64,
    pub bays_taken: i64,
    /// What one more floor down would cost.
    pub extend_cost: i64,
    /// BOX RULES: cars that have boarded, and their average wait in ticks.
    pub rides: i64,
    pub average_wait: i64,
}

pub fn query(world: &WorldState, x: i64, y: i64) -> Option<CarElevatorInfo> {
    let i = elevator_at(&world.box_ext, x, y)?;
    let e = &world.box_ext.car_elevators[i];
    let bays = reachable_bays(world, e);
    let taken = bays.iter().filter(|(k, _, _)| world.box_ext.vehicles.iter().any(|v| v.bay == *k)).count();
    Some(CarElevatorInfo {
        column: e.column,
        width: CAR_ELEVATOR_WIDTH,
        served_floors: (e.bottom_floor..=STREET_FLOOR).rev().collect(),
        bottom_floor: e.bottom_floor,
        platform: platform_views(&world.box_ext).swap_remove(i),
        queue_length: e.queue.len() as i64,
        vehicles: world.box_ext.vehicles.iter().filter(|v| v.elevator == e.column).count() as i64,
        bays_reached: bays.len() as i64,
        bays_taken: taken as i64,
        extend_cost: CAR_ELEVATOR_FLOOR_COST,
        rides: e.rides,
        average_wait: if e.rides > 0 { e.wait_total / e.rides } else { 0 },
    })
}

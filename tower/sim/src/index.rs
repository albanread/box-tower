// ts: index.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::carriers::slot::floor_to_slot;
use crate::commands::{
    handle_add_elevator_car, handle_place_tile, handle_remove_elevator_car, handle_remove_tile,
    handle_set_cinema_movie_pool, handle_set_elevator_dwell_delay, handle_set_elevator_home_floor,
    handle_set_elevator_waiting_car_response, handle_set_rent_level, handle_toggle_elevator_floor_stop,
};
use crate::events::handle_prompt_response;
use crate::ledger::LedgerState;
use crate::resources::STARTING_CASH;
use crate::sims::scoring::{create_sim_state_record, create_sim_state_records};
use crate::snapshot::{create_initial_snapshot, hydrate_snapshot, serialize_sim_state};
use crate::tick::service_idle_tasks::{service_idle_tasks, ServiceIdleTasksContext};
use crate::time::TimeState;
use crate::world::{
    y_to_floor, GateFlags, LobbyMode, SidecarRecord, SimNotificationKind, SimPrompt, WorldState,
};

pub use crate::commands::{CellPatch, CommandResult, SimCommand};
pub use crate::sims::population::sim_key;
pub use crate::sims::scoring::SimStateRecord;
pub use crate::snapshot::SimSnapshot;
pub use crate::world::{CarrierPendingRoute, CarrierRecord, SimRecord};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierCarStateRecord {
    pub carrier_id: i64,
    pub car_index: i64,
    pub car_count: i64,
    pub column: i64,
    pub carrier_mode: i64,
    pub current_floor: i64,
    pub target_floor: i64,
    pub settle_counter: i64,
    pub direction_flag: i64,
    pub dwell_counter: i64,
    pub assigned_count: i64,
    pub prev_floor: i64,
    pub arrival_seen: i64,
    pub arrival_tick: i64,
    pub home_floor: i64,
    pub active: bool,
}

// ─── Step result ──────────────────────────────────────────────────────────────

/// An element of `StepResult.notifications` (upstream's inline
/// `{ kind: string; message: string }`; the kind is always a
/// `SimNotification` kind, which serialises as the same string).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepNotification {
    pub kind: SimNotificationKind,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub sim_time: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub economy_changed: Option<bool>,
    pub cell_patches: Vec<CellPatch>,
    pub notifications: Vec<StepNotification>,
    /// Upstream's inline prompt type is `SimPrompt`'s shape.
    pub prompts: Vec<SimPrompt>,
}

// ─── queryCell's result ───────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub object_type_code: i64,
    pub rent_level: i64,
    pub eval_level: i64,
    pub unit_status: i64,
    pub activation_tick_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue_availability: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub housekeeping_claimed_flag: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CinemaInfo {
    pub selector: i64,
    pub link_age_counter: i64,
    pub attendance_counter: i64,
    pub link_phase_state: i64,
}

/// An element of `CarrierInfo.carInfos`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarInfo {
    pub home_floor: i64,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierInfo {
    pub carrier_id: i64,
    pub column: i64,
    pub carrier_mode: i64,
    pub top_served_floor: i64,
    pub bottom_served_floor: i64,
    pub car_count: i64,
    pub max_cars: i64,
    pub served_floors: Vec<i64>,
    pub dwell_delay: i64,
    pub waiting_car_response_threshold: i64,
    pub stop_floor_enabled: Vec<bool>,
    pub car_infos: Vec<CarInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryCellResult {
    pub anchor_x: i64,
    pub tile_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_info: Option<ObjectInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cinema_info: Option<CinemaInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_info: Option<CarrierInfo>,
    // BOX: car elevator -- not in upstream; absent except on a car elevator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_elevator_info: Option<crate::box_ext::car_elevator::CarElevatorInfo>,
}

/// `Number(part)` of an "x,y" key.
fn key_part(key: &str, part: usize) -> i64 {
    key.split(',')
        .nth(part)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("key {key:?} is not \"x,y\""))
}

// ─── TowerSim ─────────────────────────────────────────────────────────────────

pub struct TowerSim {
    time: TimeState,
    world: WorldState,
    ledger: LedgerState,
    pub free_build: bool,
}

impl TowerSim {
    fn new(time: TimeState, world: WorldState, ledger: LedgerState) -> Self {
        TowerSim {
            time,
            world,
            ledger,
            free_build: false,
        }
    }

    // ── Factory methods ────────────────────────────────────────────────────────

    pub fn create(tower_id: &str, name: &str, lobby_mode: Option<LobbyMode>) -> TowerSim {
        TowerSim::from_snapshot(&create_initial_snapshot(tower_id, name, STARTING_CASH, lobby_mode))
    }

    /// BOX: BOX RULES -- a new tower made for the BOX: as `create`, with BOX
    /// RULES on.  (`create`, the fixtures and the harness keep them off.)
    pub fn create_box(tower_id: &str, name: &str, lobby_mode: Option<LobbyMode>) -> TowerSim {
        let mut sim = TowerSim::create(tower_id, name, lobby_mode);
        sim.set_box_rules(true);
        sim
    }

    /// BOX: BOX RULES -- turn them on or off (with them, the funds' cap).
    pub fn set_box_rules(&mut self, on: bool) {
        crate::box_ext::set_box_rules(&mut self.world, on);
        self.ledger.box_cash_cap = if on { crate::ledger::BOX_CASH_CAP } else { 0 };
    }

    /// BOX: BOX RULES -- on?
    pub fn box_rules(&self) -> bool {
        crate::box_ext::box_rules(&self.world)
    }

    /// BOX: BOX RULES -- make a medical incident happen now (a collapse, or
    /// with `admission` an outside patient brought in); true if one started.
    pub fn box_trigger_incident(&mut self, admission: bool) -> bool {
        crate::box_ext::emergency::trigger(&mut self.world, &self.time, admission)
    }

    /// BOX: BOX RULES -- sim `sim_index` collapses now (if in the tower).
    pub fn box_collapse_sim(&mut self, sim_index: i64) -> bool {
        crate::box_ext::emergency::collapse_sim(&mut self.world, &self.time, sim_index)
    }

    /// BOX: test hook -- a fire (`fire`) or a bomb threat started now,
    /// without the game's guards (events::test_start_event); true if one did.
    pub fn box_test_event(&mut self, fire: bool) -> bool {
        crate::events::test_start_event(&mut self.world, &self.time, fire)
    }

    /// BOX: front end -- the world, the ledger and the clock, read only, for
    /// the panes (the finances, the evaluation, where an event is).
    pub fn world(&self) -> &WorldState {
        &self.world
    }
    pub fn ledger(&self) -> &LedgerState {
        &self.ledger
    }
    pub fn time(&self) -> &TimeState {
        &self.time
    }

    /// BOX: what the presenter draws of the BOX additions.
    pub fn box_views(&self) -> crate::box_ext::BoxViews {
        crate::box_ext::views(&self.world)
    }

    pub fn from_snapshot(snap: &SimSnapshot) -> TowerSim {
        // structuredClone so subsequent step() mutations don't bleed back into
        // the caller's snapshot — caused client replays to diverge when
        // replayTo re-used the same baseSnapshot across calls.
        let hydrated = hydrate_snapshot(snap.clone());
        let mut sim = TowerSim::new(hydrated.time, hydrated.world, hydrated.ledger);
        // BOX: BOX RULES -- a tower saved before the funds' cap was raised
        if sim.box_rules() && sim.ledger.box_cash_cap == 0 {
            sim.ledger.box_cash_cap = crate::ledger::BOX_CASH_CAP;
        }
        sim
    }

    // ── Tick ──────────────────────────────────────────────────────────────────

    pub fn step(&mut self) -> StepResult {
        let balance_before = self.ledger.cash_balance;

        // Snapshot display-facing room fields before tick to detect changes.
        // (Upstream keeps four maps with the same keys: occupiedFlag,
        // unitStatus, evalLevel, evalScore; here one map of the four.)
        let mut before: IndexMap<String, (i64, i64, i64, i64)> = IndexMap::new();
        for (key, record) in self.world.placed_objects.iter() {
            // Display-facing scored bit: binary +0x14 (occupiedFlag).
            before.insert(
                key.to_string(),
                (record.occupied_flag, record.unit_status, record.eval_level, record.eval_score),
            );
        }

        // Binary 1268:01a6 service_idle_tasks: day scheduler + carrier tick.
        // `serviceIdleTasks` mutates ctx.time in place so that the inline
        // arrival/boarding paths (inside queue/dispatch-arrivals.ts and
        // queue/process-travel.ts) observe the advanced tick. Phase 7 removed
        // the `onArrival` / `onBoarding` callback plumbing — family dispatch
        // and stress accumulation now run inline, matching the binary.
        let mut ctx = ServiceIdleTasksContext {
            world: &mut self.world,
            ledger: &mut self.ledger,
            time: &mut self.time,
        };
        service_idle_tasks(&mut ctx);

        // Emit cell patches for display-facing room state changes.
        let mut cell_patches: Vec<CellPatch> = Vec::new();
        for (key, record) in self.world.placed_objects.iter() {
            let Some(&(prev, prev_unit_status, prev_eval_level, prev_eval_score)) = before.get(key) else {
                continue;
            };
            if prev != record.occupied_flag
                || prev_unit_status != record.unit_status
                || prev_eval_level != record.eval_level
                || prev_eval_score != record.eval_score
            {
                let mut p = CellPatch::new(
                    key_part(key, 0),
                    key_part(key, 1),
                    self.world.cells.get(key).map(String::as_str).unwrap_or(""),
                    true,
                );
                p.eval_active_flag = Some(record.occupied_flag);
                p.unit_status = Some(record.unit_status);
                p.eval_level = Some(record.eval_level);
                p.eval_score = Some(record.eval_score);
                cell_patches.push(p);
            }
        }

        // Drain pending notifications and prompts
        let notifications: Vec<_> = self.world.pending_notifications.drain(..).collect();
        let prompts: Vec<_> = self.world.pending_prompts.drain(..).collect();

        StepResult {
            sim_time: self.time.total_ticks,
            economy_changed: Some(self.ledger.cash_balance != balance_before),
            cell_patches,
            notifications: notifications
                .into_iter()
                .map(|n| StepNotification {
                    kind: n.kind,
                    message: n.message.unwrap_or_default(),
                })
                .collect(),
            prompts,
        }
    }

    // ── Commands ──────────────────────────────────────────────────────────────

    pub fn submit_command(&mut self, cmd: &SimCommand) -> CommandResult {
        match cmd {
            SimCommand::PlaceTile { x, y, tile_type } => handle_place_tile(
                *x,
                *y,
                tile_type,
                &mut self.world,
                &mut self.ledger,
                Some(self.free_build),
                Some(self.time.daypart_index),
            ),
            SimCommand::RemoveTile { x, y } => handle_remove_tile(*x, *y, &mut self.world, &mut self.ledger),
            SimCommand::PromptResponse { prompt_id, accepted } => {
                handle_prompt_response(&mut self.world, &mut self.ledger, &mut self.time, prompt_id, *accepted);
                CommandResult {
                    accepted: true,
                    patch: Some(Vec::new()),
                    reason: None,
                    economy_changed: Some(true),
                }
            }
            SimCommand::SetRentLevel { x, y, rent_level } => {
                handle_set_rent_level(*x, *y, *rent_level, &mut self.world, self.time.daypart_index)
            }
            SimCommand::AddElevatorCar { x, y } => {
                handle_add_elevator_car(*x, *y, &mut self.world, &mut self.ledger, self.free_build)
            }
            SimCommand::RemoveElevatorCar { x, y } => handle_remove_elevator_car(*x, *y, &mut self.world),
            SimCommand::SetElevatorDwellDelay { x, y, value } => {
                handle_set_elevator_dwell_delay(*x, *y, *value, &mut self.world)
            }
            SimCommand::SetElevatorWaitingCarResponse { x, y, value } => {
                handle_set_elevator_waiting_car_response(*x, *y, *value, &mut self.world)
            }
            SimCommand::SetElevatorHomeFloor { x, car_index, floor } => {
                handle_set_elevator_home_floor(*x, *car_index, *floor, &mut self.world)
            }
            SimCommand::ToggleElevatorFloorStop { x, floor } => {
                handle_toggle_elevator_floor_stop(*x, *floor, &mut self.world)
            }
            SimCommand::SetCinemaMoviePool { x, y, pool } => {
                handle_set_cinema_movie_pool(*x, *y, *pool, &mut self.world, &mut self.ledger, self.free_build)
            }
        }
    }

    // ── Cell inspection ──────────────────────────────────────────────────────────

    pub fn query_cell(&self, x: i64, y: i64) -> QueryCellResult {
        let world = &self.world;
        let key = format!("{x},{y}");
        let anchor_key = world.cell_to_anchor.get(&key).cloned().unwrap_or_else(|| key.clone());
        let tile_type = world.cells.get(&anchor_key).cloned().unwrap_or_else(|| "empty".to_string());

        let record = world.placed_objects.get(&anchor_key);
        let mut venue_availability: Option<i64> = None;
        let mut cinema_info: Option<CinemaInfo> = None;
        if let Some(record) = record {
            if record.linked_record_index >= 0 {
                match world.sidecars.get(record.linked_record_index as usize) {
                    Some(SidecarRecord::CommercialVenue(sidecar)) => {
                        venue_availability = Some(sidecar.availability_state);
                    }
                    Some(SidecarRecord::EntertainmentLink(sidecar))
                        if sidecar.family_selector_or_single_link_flag != 0xff =>
                    {
                        cinema_info = Some(CinemaInfo {
                            selector: sidecar.family_selector_or_single_link_flag,
                            link_age_counter: sidecar.link_age_counter,
                            attendance_counter: sidecar.attendance_counter,
                            link_phase_state: sidecar.link_phase_state,
                        });
                    }
                    _ => {}
                }
            }
        }
        let object_info = record.map(|record| ObjectInfo {
            object_type_code: record.object_type_code,
            rent_level: record.rent_level,
            eval_level: record.eval_level,
            unit_status: record.unit_status,
            activation_tick_count: record.activation_tick_count,
            venue_availability,
            housekeeping_claimed_flag: Some(record.housekeeping_claimed_flag),
        });

        // Check for carrier at this column (elevator overlays)
        let overlay_key = match world.overlay_to_anchor.get(&key) {
            Some(anchor) => Some(anchor.clone()),
            None => world.overlays.get(&key).filter(|t| !t.is_empty()).map(|_| key.clone()),
        };
        let mut carrier_info: Option<CarrierInfo> = None;

        if let Some(overlay_key) = overlay_key.filter(|k| !k.is_empty()) {
            let col = key_part(&overlay_key, 0);
            let queried_floor = y_to_floor(y);
            let carrier = world.carriers.iter().find(|c| {
                c.column == col && queried_floor >= c.bottom_served_floor && queried_floor <= c.top_served_floor
            });
            if let Some(carrier) = carrier {
                let mut served_floors: Vec<i64> = Vec::new();
                let mut stop_floor_enabled: Vec<bool> = Vec::new();
                for f in carrier.bottom_served_floor..=carrier.top_served_floor {
                    let slot = floor_to_slot(carrier, f);
                    if slot >= 0 {
                        served_floors.push(f);
                        stop_floor_enabled.push(crate::js::at_or(&carrier.stop_floor_enabled, slot, 1) != 0);
                    }
                }
                carrier_info = Some(CarrierInfo {
                    carrier_id: carrier.carrier_id,
                    column: col,
                    carrier_mode: carrier.carrier_mode,
                    top_served_floor: carrier.top_served_floor,
                    bottom_served_floor: carrier.bottom_served_floor,
                    car_count: carrier.cars.iter().filter(|c| c.active).count() as i64,
                    max_cars: 8,
                    served_floors,
                    dwell_delay: crate::js::at_or(&carrier.dwell_delay, 0, 0),
                    waiting_car_response_threshold: carrier.waiting_car_response_threshold,
                    stop_floor_enabled,
                    car_infos: carrier
                        .cars
                        .iter()
                        .map(|car| CarInfo {
                            home_floor: car.home_floor,
                            active: car.active,
                        })
                        .collect(),
                });
            }
        }

        QueryCellResult {
            anchor_x: key_part(&anchor_key, 0),
            tile_type,
            object_info,
            cinema_info,
            carrier_info,
            car_elevator_info: crate::box_ext::car_elevator::query(world, x, y), // BOX: car elevator
        }
    }

    // ── Serialization ──────────────────────────────────────────────────────────

    /// BOX: car elevator -- the BOX additions' state, for the presenter
    /// (box_ext::car_elevator::vehicle_views / platform_views).
    pub fn box_ext(&self) -> &crate::box_ext::BoxExtState {
        &self.world.box_ext
    }

    pub fn save_state(&self) -> SimSnapshot {
        serialize_sim_state(&self.time, &self.world, &self.ledger)
    }

    pub fn drain_notifications(&mut self) -> Vec<StepNotification> {
        self.world
            .pending_notifications
            .drain(..)
            .map(|n| StepNotification {
                kind: n.kind,
                message: n.message.unwrap_or_default(),
            })
            .collect()
    }

    pub fn drain_prompts(&mut self) -> Vec<SimPrompt> {
        self.world.pending_prompts.drain(..).collect()
    }

    // ── Accessors for TowerRoom ────────────────────────────────────────────────

    pub fn tower_id(&self) -> &str {
        &self.world.tower_id
    }
    pub fn name(&self) -> &str {
        &self.world.name
    }
    pub fn sim_time(&self) -> i64 {
        self.time.total_ticks
    }
    pub fn cash(&self) -> i64 {
        self.ledger.cash_balance
    }
    /// BOX: front end -- the funds set outright (the test game's budget)
    pub fn set_cash(&mut self, cash: i64) {
        self.ledger.cash_balance = cash;
    }

    pub fn population(&self) -> i64 {
        self.ledger.population_ledger.iter().sum()
    }

    pub fn current_population(&self) -> i64 {
        self.world.current_population
    }

    pub fn star_count(&self) -> i64 {
        self.world.star_count
    }
    /// Upstream's parameter is `1 | 2 | 3 | 4 | 5 | 6`.
    pub fn set_star_count(&mut self, star_count: i64) {
        self.world.star_count = star_count;
    }
    pub fn daypart_index(&self) -> i64 {
        self.time.daypart_index
    }
    pub fn gate_flags(&self) -> &GateFlags {
        &self.world.gate_flags
    }
    pub fn rng_call_count(&self) -> i64 {
        self.world.rng_call_count
    }
    pub fn width(&self) -> i64 {
        self.world.width
    }
    pub fn height(&self) -> i64 {
        self.world.height
    }

    pub fn cells_to_array(&self) -> Vec<CellPatch> {
        let world = &self.world;
        let mut result: Vec<CellPatch> = Vec::new();
        for (key, tile_type) in world.cells.iter() {
            let is_anchor = !world.cell_to_anchor.get(key).is_some_and(|a| !a.is_empty());
            let record = if is_anchor { world.placed_objects.get(key) } else { None };
            let mut coverage_flag: Option<i64> = None;
            if let Some(record) = record {
                if tile_type == "parking" && record.linked_record_index >= 0 {
                    if let Some(SidecarRecord::ServiceRequest(sidecar)) =
                        world.sidecars.get(record.linked_record_index as usize)
                    {
                        coverage_flag = Some(sidecar.coverage_flag.unwrap_or(0));
                    }
                }
            }
            let mut p = CellPatch::new(key_part(key, 0), key_part(key, 1), tile_type, is_anchor);
            if let Some(record) = record {
                p.eval_active_flag = Some(record.occupied_flag);
                p.unit_status = Some(record.unit_status);
                p.eval_level = Some(record.eval_level);
                p.eval_score = Some(record.eval_score);
            }
            p.coverage_flag = coverage_flag;
            result.push(p);
        }
        for (key, tile_type) in world.overlays.iter() {
            let mut p = CellPatch::new(key_part(key, 0), key_part(key, 1), tile_type, true);
            p.is_overlay = Some(true);
            result.push(p);
        }
        result
    }

    pub fn sims_to_array(&self) -> Vec<SimStateRecord> {
        create_sim_state_records(&self.world, &self.time)
    }

    /// Upstream takes the sim itself (one of `liveSims`); here its index in
    /// `live_sims()`.
    pub fn sim_to_record(&self, sim_index: i64) -> Option<SimStateRecord> {
        // { carrier, route } as (carrier index, route index).
        let mut pending_by_sim_id: crate::sims::scoring::PendingBySimId = IndexMap::new();
        for carrier in self.world.carriers.iter() {
            for route in carrier.pending_routes.iter() {
                pending_by_sim_id.insert(route.sim_id.clone(), (carrier, route));
            }
        }
        create_sim_state_record(&self.world, &self.time, sim_index, &pending_by_sim_id)
    }

    pub fn live_sims(&self) -> &[SimRecord] {
        &self.world.sims
    }

    pub fn live_carriers(&self) -> &[CarrierRecord] {
        &self.world.carriers
    }

    /// Per-floor active fire fronts (binary `g_fire_left_pos` / `g_fire_right_pos`,
    /// 120-entry uint16 arrays). Entries with sentinel 0xffff are inactive.
    /// Used by the client renderer to draw the on-fire overlay.
    ///
    /// Upstream's `{ fireLeftPos, fireRightPos }`, as a pair.
    pub fn fire_fronts(&self) -> (&[i64], &[i64]) {
        (
            &self.world.event_state.fire_left_pos,
            &self.world.event_state.fire_right_pos,
        )
    }

    pub fn carriers_to_array(&self) -> Vec<CarrierCarStateRecord> {
        self.world
            .carriers
            .iter()
            .flat_map(|carrier| {
                carrier.cars.iter().enumerate().map(move |(car_index, car)| CarrierCarStateRecord {
                    carrier_id: carrier.carrier_id,
                    car_index: car_index as i64,
                    car_count: carrier.cars.len() as i64,
                    column: carrier.column,
                    carrier_mode: carrier.carrier_mode,
                    current_floor: car.current_floor,
                    target_floor: car.target_floor,
                    settle_counter: car.settle_counter,
                    direction_flag: car.direction_flag,
                    dwell_counter: car.dwell_counter,
                    assigned_count: car.assigned_count,
                    prev_floor: car.prev_floor,
                    arrival_seen: car.arrival_seen,
                    arrival_tick: car.arrival_tick,
                    home_floor: car.home_floor,
                    active: car.active,
                })
            })
            .collect()
    }
}

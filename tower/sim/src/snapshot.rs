// ts: snapshot.ts
//
// Upstream's normalizeSnapshot and hydrateSnapshot take a "SimSnapshot" that
// is really whatever JSON a saved game held (legacy key names, snake_case,
// missing fields) and patch it in place.  A typed SimSnapshot cannot hold
// that, so here they work on the JSON (`serde_json::Value`) step for step,
// and the snapshot is deserialised once the JSON has upstream's current
// shape.  `hydrate_snapshot` takes a typed snapshot (as TowerSim.fromSnapshot
// does) and goes through the JSON; `hydrate_snapshot_json` takes a saved
// game.  Where upstream would throw (reading `.length` of a missing array)
// these return an error.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::carriers::init_carrier_state;
use crate::js;
use crate::ledger::{create_ledger_state, LedgerState};
use crate::queue::route_record::RouteRequestRing;
use crate::reachability::rebuild_tables::{rebuild_route_reachability_tables, rebuild_transfer_group_cache};
use crate::reachability::special_link_records::rebuild_special_link_route_records;
use crate::resources::{FAMILY_METRO_TOP, LEGACY_TILE_ALIASES, LEGACY_VIP_TILE_TO_STANDARD};
use crate::sims::population::rebuild_runtime_sims;
use crate::time::{create_new_game_time_state, TimeState};
use crate::world::{
    create_commercial_venue_buckets, create_event_state, create_gate_flags, create_medical_service_slots,
    LobbyMode, SpecialLinkRecord, SpecialLinkSegment, TransferGroupEntry, WorldState, DEFAULT_LOBBY_MODE,
    GRID_HEIGHT, GRID_WIDTH, MAX_SPECIAL_LINKS, MAX_SPECIAL_LINK_RECORDS, MAX_TRANSFER_GROUPS,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimSnapshot {
    pub time: TimeState,
    pub world: WorldState,
    pub ledger: LedgerState,
}

fn create_empty_special_links() -> Vec<SpecialLinkSegment> {
    (0..MAX_SPECIAL_LINKS)
        .map(|_| SpecialLinkSegment {
            active: false,
            flags: 0,
            height_metric: 0,
            entry_floor: 0,
            reserved_byte: 0,
            descending_load_counter: 0,
            ascending_load_counter: 0,
        })
        .collect()
}

fn create_empty_special_link_records() -> Vec<SpecialLinkRecord> {
    (0..MAX_SPECIAL_LINK_RECORDS)
        .map(|_| SpecialLinkRecord {
            active: false,
            lower_floor: 0,
            upper_floor: 0,
            reachability_masks_by_floor: vec![0; GRID_HEIGHT as usize],
        })
        .collect()
}

fn create_empty_transfer_group_entries() -> Vec<TransferGroupEntry> {
    (0..MAX_TRANSFER_GROUPS)
        .map(|_| TransferGroupEntry {
            active: false,
            tagged_floor: -1,
            carrier_mask: 0,
        })
        .collect()
}

/// `lobby_mode` is upstream's optional parameter (`None` for
/// DEFAULT_LOBBY_MODE).
pub fn create_initial_snapshot(
    tower_id: &str,
    name: &str,
    starting_cash: i64,
    lobby_mode: Option<LobbyMode>,
) -> SimSnapshot {
    let lobby_mode = lobby_mode.unwrap_or(DEFAULT_LOBBY_MODE);
    SimSnapshot {
        time: create_new_game_time_state(),
        world: WorldState {
            tower_id: tower_id.to_string(),
            name: name.to_string(),
            width: GRID_WIDTH,
            height: GRID_HEIGHT,
            lobby_height: 1,
            lobby_mode,
            gate_flags: create_gate_flags(),
            cells: js::JsObject::new(),
            cell_to_anchor: js::JsObject::new(),
            overlays: js::JsObject::new(),
            overlay_to_anchor: js::JsObject::new(),
            placed_objects: js::JsObject::new(),
            sidecars: Vec::new(),
            sims: Vec::new(),
            carriers: Vec::new(),
            special_links: create_empty_special_links(),
            special_link_records: create_empty_special_link_records(),
            floor_walkability_flags: vec![0; GRID_HEIGHT as usize],
            transfer_group_entries: create_empty_transfer_group_entries(),
            transfer_group_cache: vec![0; GRID_HEIGHT as usize],
            parking_demand_log: Vec::new(),
            medical_service_slots: create_medical_service_slots(),
            star_count: 1,
            current_population: 0,
            current_population_buckets: js::JsObject::new(),
            commercial_venue_buckets: create_commercial_venue_buckets(),
            rng_state: 1,
            rng_call_count: 0,
            event_state: create_event_state(),
            pending_notifications: Vec::new(),
            pending_prompts: Vec::new(),
            box_ext: Default::default(), // BOX: car elevator
            sim_owner_cache: Default::default(),
            detached_objects: Default::default(),
        },
        ledger: create_ledger_state(starting_cash),
    }
}

// ─── JSON helpers (JavaScript's view of a parsed object) ────────────────────

type Obj = Map<String, Value>;
type R<T> = Result<T, String>;

/// `x` is `null` or `undefined`.
fn nullish(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// `!x` for the values a snapshot holds (no NaN in JSON).
fn falsy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => !b,
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        Some(Value::String(s)) => s.is_empty(),
        Some(_) => false,
    }
}

/// `o.k ??= f()`.
fn nullish_assign(o: &mut Obj, k: &str, f: impl FnOnce() -> Value) {
    if nullish(o.get(k)) {
        o.insert(k.to_string(), f());
    }
}

/// `x as number` where upstream relies on it being a number.
fn num(v: Option<&Value>) -> Option<i64> {
    v.and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
}

/// `o.k` where upstream goes on to use it as an object (and would throw on
/// `undefined`).
fn field_obj<'a>(o: &'a mut Obj, k: &str, ctx: &str) -> R<&'a mut Obj> {
    o.get_mut(k)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{ctx}.{k} is not an object"))
}

/// `o.k` where upstream goes on to use it as an array.
fn field_arr<'a>(o: &'a mut Obj, k: &str, ctx: &str) -> R<&'a mut Vec<Value>> {
    o.get_mut(k)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("{ctx}.{k} is not an array"))
}

/// `o.k.length` (a missing array throws in upstream).
fn arr_len(o: &Obj, k: &str, ctx: &str) -> R<usize> {
    o.get(k)
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| format!("{ctx}.{k}.length of a non-array"))
}

fn to_json<T: Serialize>(t: T) -> Value {
    serde_json::to_value(t).expect("serialising a snapshot part")
}

/// `delete o.k` (keeping the order of the others).
fn delete(o: &mut Obj, k: &str) {
    o.shift_remove(k);
}

// ─── Migration ──────────────────────────────────────────────────────────────

fn snake_to_camel(s: &str) -> String {
    // s.replace(/_([a-z])/g, (_, c) => c.toUpperCase())
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == '_' && i + 1 < b.len() && b[i + 1].is_ascii_lowercase() {
            out.push(b[i + 1].to_ascii_uppercase());
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn rename_keys_shallow(obj: &mut Value) {
    let Some(record) = obj.as_object_mut() else {
        return;
    };
    let keys: Vec<String> = record.keys().cloned().collect();
    for key in keys {
        if !key.contains('_') {
            continue;
        }
        let camel_key = snake_to_camel(&key);
        if !record.contains_key(&camel_key) {
            let v = record[&key].clone();
            record.insert(camel_key, v);
        }
        // (When the key has no `_x` to change, camelKey is the key itself,
        // and upstream deletes it: so does this.)
        delete(record, &key);
    }
}

fn rename_keys_deep(obj: &mut Value) {
    match obj {
        Value::Array(items) => {
            for item in items {
                rename_keys_deep(item);
            }
        }
        Value::Object(_) => {
            rename_keys_shallow(obj);
            for value in obj.as_object_mut().unwrap().values_mut() {
                rename_keys_deep(value);
            }
        }
        _ => {}
    }
}

fn migrate_snake_to_camel(snapshot: &mut Obj) {
    if !falsy(snapshot.get("time")) {
        rename_keys_shallow(snapshot.get_mut("time").unwrap());
    }
    if !falsy(snapshot.get("ledger")) {
        rename_keys_shallow(snapshot.get_mut("ledger").unwrap());
    }
    if falsy(snapshot.get("world")) {
        return;
    }

    let world = snapshot.get_mut("world").unwrap();
    let skip_keys = ["cells", "overlays", "cellToAnchor", "overlayToAnchor"];
    rename_keys_shallow(world);
    if let Some(world) = world.as_object_mut() {
        for (key, value) in world.iter_mut() {
            if !skip_keys.contains(&key.as_str()) {
                rename_keys_deep(value);
            }
        }
    }
}

fn normalize_legacy_tile_names(world: &mut Obj) -> R<()> {
    let cells = field_obj(world, "cells", "world")?;
    for (_, tile) in cells.iter_mut() {
        if let Some(t) = tile.as_str() {
            if let Some(alias) = js::record_get(LEGACY_TILE_ALIASES, t) {
                *tile = Value::String(alias.to_string());
            }
        }
    }
    Ok(())
}

/// The y of a "x,y" cell key (`key.split(",").map(Number)[1]`).
fn key_y(key: &str) -> R<i64> {
    key.split(',')
        .nth(1)
        .and_then(|y| y.parse().ok())
        .ok_or_else(|| format!("cell key {key:?} is not \"x,y\""))
}

/// `RouteRequestRing.from(data)` on the JSON of `data`.
fn ring_from(items: Option<&Value>, head: i64, count: i64) -> R<Value> {
    let items: Vec<Option<String>> = items
        .and_then(Value::as_array)
        .ok_or("RouteRequestRing.from: items is not an array")?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect();
    Ok(to_json(RouteRequestRing::from(&items, head, count)))
}

/// Upstream's normalizeSnapshot, on the JSON: brings a saved snapshot of
/// any age to the current shape, in place.
pub fn normalize_snapshot(raw: &mut Value) -> R<()> {
    let snapshot = raw.as_object_mut().ok_or("a snapshot is a JSON object")?;
    // `old` is the snapshot itself, read for the oldest (flat) layout.

    if falsy(snapshot.get("world")) {
        let old = snapshot.clone();
        let or = |k: &str, d: Value| match old.get(k) {
            None | Some(Value::Null) => d,
            Some(v) => v.clone(),
        };
        let mut w = Map::new();
        w.insert("towerId".into(), or("towerId", Value::Null));
        w.insert("name".into(), or("name", Value::Null));
        w.insert("width".into(), or("width", json!(GRID_WIDTH)));
        w.insert("height".into(), or("height", json!(GRID_HEIGHT)));
        w.insert("lobbyHeight".into(), or("lobbyHeight", json!(1)));
        w.insert("lobbyMode".into(), or("lobbyMode", json!("perfect-parity")));
        w.insert("gateFlags".into(), to_json(create_gate_flags()));
        w.insert("cells".into(), or("cells", json!({})));
        w.insert("cellToAnchor".into(), or("cellToAnchor", json!({})));
        w.insert("overlays".into(), or("overlays", json!({})));
        w.insert("overlayToAnchor".into(), or("overlayToAnchor", json!({})));
        w.insert("placedObjects".into(), json!({}));
        w.insert("sidecars".into(), json!([]));
        w.insert("sims".into(), json!([]));
        w.insert("carriers".into(), json!([]));
        w.insert("specialLinks".into(), json!([]));
        w.insert("specialLinkRecords".into(), json!([]));
        w.insert("floorWalkabilityFlags".into(), json!([]));
        w.insert("transferGroupEntries".into(), json!([]));
        w.insert("transferGroupCache".into(), json!([]));
        w.insert("parkingDemandLog".into(), json!([]));
        w.insert("medicalServiceSlots".into(), to_json(create_medical_service_slots()));
        w.insert("starCount".into(), json!(1));
        w.insert("currentPopulation".into(), json!(0));
        w.insert("currentPopulationBuckets".into(), json!({}));
        w.insert("commercialVenueBuckets".into(), to_json(create_commercial_venue_buckets()));
        w.insert("rngState".into(), json!(1));
        w.insert("rngCallCount".into(), json!(0));
        w.insert("eventState".into(), to_json(create_event_state()));
        w.insert("pendingNotifications".into(), json!([]));
        w.insert("pendingPrompts".into(), json!([]));
        snapshot.insert("world".into(), Value::Object(w));
    }
    {
        let world = field_obj(snapshot, "world", "snapshot")?;
        if falsy(world.get("commercialVenueBuckets")) {
            world.insert("commercialVenueBuckets".into(), to_json(create_commercial_venue_buckets()));
        }
    }

    if falsy(snapshot.get("ledger")) {
        let cash = num(snapshot.get("cash")).unwrap_or(2_000_000);
        snapshot.insert("ledger".into(), to_json(create_ledger_state(cash)));
    }

    if falsy(snapshot.get("time")) {
        let total_ticks = num(snapshot.get("simTime")).unwrap_or(0);
        snapshot.insert(
            "time".into(),
            json!({
                "dayTick": 0,
                "daypartIndex": 0,
                "dayCounter": 0,
                "weekendFlag": 0,
                "totalTicks": total_ticks,
            }),
        );
    }

    migrate_snake_to_camel(snapshot);

    // Migrate starCount from time → world (legacy snapshots stored it in time)
    let legacy_time = field_obj(snapshot, "time", "snapshot")?;
    if let Some(star_count) = legacy_time.get("starCount").cloned() {
        delete(legacy_time, "starCount");
        let world = field_obj(snapshot, "world", "snapshot")?;
        nullish_assign(world, "starCount", || star_count);
    }

    normalize_legacy_tile_names(field_obj(snapshot, "world", "snapshot")?)?;

    let legacy_ledger = field_obj(snapshot, "ledger", "snapshot")?;
    if !legacy_ledger.contains_key("populationLedger") && legacy_ledger.contains_key("primaryLedger") {
        let v = legacy_ledger["primaryLedger"].clone();
        legacy_ledger.insert("populationLedger".into(), v);
    }
    if !legacy_ledger.contains_key("incomeLedger") && legacy_ledger.contains_key("secondaryLedger") {
        let v = legacy_ledger["secondaryLedger"].clone();
        legacy_ledger.insert("incomeLedger".into(), v);
    }
    if !legacy_ledger.contains_key("expenseLedger") && legacy_ledger.contains_key("tertiaryLedger") {
        let v = legacy_ledger["tertiaryLedger"].clone();
        legacy_ledger.insert("expenseLedger".into(), v);
    }

    let world = field_obj(snapshot, "world", "snapshot")?;
    for (_, record) in field_obj(world, "placedObjects", "world")?.iter_mut() {
        let Some(legacy_record) = record.as_object_mut() else {
            continue;
        };
        if !legacy_record.contains_key("unitStatus") && legacy_record.contains_key("stayPhase") {
            let v = legacy_record["stayPhase"].clone();
            legacy_record.insert("unitStatus".into(), v);
        }
        if !legacy_record.contains_key("occupiableFlag") && legacy_record.contains_key("pairingActiveFlag") {
            let v = legacy_record["pairingActiveFlag"].clone();
            legacy_record.insert("occupiableFlag".into(), v);
        }
        if !legacy_record.contains_key("occupiableFlag") && legacy_record.contains_key("evalActiveFlag") {
            let v = legacy_record["evalActiveFlag"].clone();
            legacy_record.insert("occupiableFlag".into(), v);
        }
        if legacy_record.contains_key("occupiableFlag") {
            // Legacy snapshots used a single occupiableFlag that conflated
            // binary +0x13 (dirty / cashflow) and +0x14 (occupied / scored).
            // Default both to the legacy value so existing saves keep behavior.
            let v = legacy_record["occupiableFlag"].clone();
            if !legacy_record.contains_key("dirtyFlag") {
                legacy_record.insert("dirtyFlag".into(), v.clone());
            }
            if !legacy_record.contains_key("occupiedFlag") {
                legacy_record.insert("occupiedFlag".into(), v);
            }
            delete(legacy_record, "occupiableFlag");
        }
        if !legacy_record.contains_key("dirtyFlag") {
            legacy_record.insert("dirtyFlag".into(), json!(1));
        }
        if !legacy_record.contains_key("occupiedFlag") {
            legacy_record.insert("occupiedFlag".into(), json!(1));
        }
        if !legacy_record.contains_key("evalLevel") && legacy_record.contains_key("pairingStatus") {
            let v = legacy_record["pairingStatus"].clone();
            legacy_record.insert("evalLevel".into(), v);
        }
        if !legacy_record.contains_key("rentLevel") && legacy_record.contains_key("variantIndex") {
            let v = legacy_record["variantIndex"].clone();
            legacy_record.insert("rentLevel".into(), v);
        }
        if !legacy_record.contains_key("evalScore") {
            legacy_record.insert("evalScore".into(), json!(-1));
        }
    }

    if num(world.get("height")).is_some_and(|h| h < GRID_HEIGHT) {
        world.insert("height".into(), json!(GRID_HEIGHT));
    }
    if falsy(world.get("width")) || num(world.get("width")).is_some_and(|w| w < GRID_WIDTH) {
        world.insert("width".into(), json!(GRID_WIDTH));
    }
    nullish_assign(world, "lobbyHeight", || json!(1));
    // Pre-existing snapshots predate the lobbyMode flag; preserve their
    // floor-14 sky-lobby positions by defaulting to perfect-parity.
    nullish_assign(world, "lobbyMode", || json!("perfect-parity"));
    nullish_assign(world, "placedObjects", || json!({}));
    nullish_assign(world, "sidecars", || json!([]));
    if !world.contains_key("sims") && world.get("entities").is_some_and(Value::is_array) {
        let v = world["entities"].clone();
        world.insert("sims".into(), v);
    }
    nullish_assign(world, "sims", || json!([]));
    nullish_assign(world, "gateFlags", || to_json(create_gate_flags()));
    {
        let gate_flags = field_obj(world, "gateFlags", "world")?;
        nullish_assign(gate_flags, "metroStationFloorIndex", || json!(-1));
        if !gate_flags.contains_key("evalSimIndex") && gate_flags.contains_key("evalEntityIndex") {
            let v = gate_flags["evalEntityIndex"].clone();
            gate_flags.insert("evalSimIndex".into(), v);
        }
        if !gate_flags.contains_key("recyclingAdequate") && gate_flags.contains_key("securityAdequate") {
            let v = gate_flags["securityAdequate"].clone();
            gate_flags.insert("recyclingAdequate".into(), v);
        }
        if !gate_flags.contains_key("recyclingCenterCount") && gate_flags.contains_key("securityLedgerScale") {
            let v = gate_flags["securityLedgerScale"].clone();
            gate_flags.insert("recyclingCenterCount".into(), v);
        }
    }
    nullish_assign(world, "carriers", || json!([]));
    if arr_len(world, "specialLinks", "world")? == 0 {
        world.insert("specialLinks".into(), to_json(create_empty_special_links()));
    }
    if arr_len(world, "specialLinkRecords", "world")? == 0 {
        world.insert("specialLinkRecords".into(), to_json(create_empty_special_link_records()));
    }
    if arr_len(world, "floorWalkabilityFlags", "world")? != GRID_HEIGHT as usize {
        world.insert("floorWalkabilityFlags".into(), json!(vec![0; GRID_HEIGHT as usize]));
    }
    if arr_len(world, "transferGroupEntries", "world")? == 0 {
        world.insert("transferGroupEntries".into(), to_json(create_empty_transfer_group_entries()));
    }
    if arr_len(world, "transferGroupCache", "world")? != GRID_HEIGHT as usize {
        world.insert("transferGroupCache".into(), json!(vec![0; GRID_HEIGHT as usize]));
    }
    nullish_assign(world, "starCount", || json!(1));
    nullish_assign(world, "currentPopulation", || json!(0));
    nullish_assign(world, "currentPopulationBuckets", || json!({}));
    nullish_assign(world, "rngState", || json!(1));
    nullish_assign(world, "rngCallCount", || json!(0));
    nullish_assign(world, "eventState", || to_json(create_event_state()));
    {
        let event_state = field_obj(world, "eventState", "world")?;
        nullish_assign(event_state, "bombSearchLowerBound", || json!(-1));
        nullish_assign(event_state, "bombSearchUpperBound", || json!(-1));
        nullish_assign(event_state, "bombSearchCurrentFloor", || json!(-1));
        nullish_assign(event_state, "bombSearchScanTile", || json!(-1));
        nullish_assign(event_state, "pendingCarrierEditColumn", || json!(-1));
        nullish_assign(event_state, "pendingCarrierEditY", || json!(-1));
        nullish_assign(event_state, "fireRescueHelpers", || json!([]));
    }
    {
        let ledger = field_obj(snapshot, "ledger", "snapshot")?;
        nullish_assign(ledger, "populationLedger", || json!(vec![0; 256]));
        nullish_assign(ledger, "incomeLedger", || json!(vec![0; 256]));
        nullish_assign(ledger, "expenseLedger", || json!(vec![0; 256]));
    }
    let world = field_obj(snapshot, "world", "snapshot")?;
    {
        let gate_flags = field_obj(world, "gateFlags", "world")?;
        nullish_assign(gate_flags, "family345SaleCount", || json!(0));
        nullish_assign(gate_flags, "newspaperTrigger", || json!(0));
        nullish_assign(gate_flags, "officeServiceOkMedical", || json!(0));
        nullish_assign(gate_flags, "securityPlaced", || json!(0));
    }
    if !world
        .get("medicalServiceSlots")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        world.insert("medicalServiceSlots".into(), to_json(create_medical_service_slots()));
    }

    for sidecar in field_arr(world, "sidecars", "world")?.iter_mut() {
        let Some(sidecar) = sidecar.as_object_mut() else {
            return Err("a sidecar is not an object".into());
        };
        match sidecar.get("kind").and_then(Value::as_str) {
            Some("commercial_venue") => {
                for k in [
                    "currentPopulation",
                    "lastAcquireTick",
                    "eligibilityThreshold",
                    "remainingCapacity",
                    "phaseASeed",
                    "phaseBSeed",
                    "overrideSeed",
                    "acquireCount",
                ] {
                    nullish_assign(sidecar, k, || json!(0));
                }
                continue;
            }
            Some("medical_center") => {
                nullish_assign(sidecar, "pendingVisitorsCount", || json!(0));
                continue;
            }
            Some("entertainment_link") => {}
            _ => continue,
        }
        nullish_assign(sidecar, "familySelectorOrSingleLinkFlag", || json!(0xff));
        nullish_assign(sidecar, "linkPhaseState", || json!(0));
        nullish_assign(sidecar, "pendingTransitionFlag", || json!(0));
        // Migrate forwardBudget/reverseBudget → upperBudget/lowerBudget
        if let Some(v) = sidecar.get("forwardBudget").cloned() {
            sidecar.insert("upperBudget".into(), v);
            delete(sidecar, "forwardBudget");
        }
        if let Some(v) = sidecar.get("reverseBudget").cloned() {
            sidecar.insert("lowerBudget".into(), v);
            delete(sidecar, "reverseBudget");
        }
    }

    let mut vip_anchors: Vec<String> = Vec::new(); // a Set: membership only
    let cell_to_anchor = world.get("cellToAnchor").cloned();
    for (key, tile_type) in field_obj(world, "cells", "world")?.iter_mut() {
        let Some(standard_tile) = tile_type.as_str().and_then(|t| js::record_get(LEGACY_VIP_TILE_TO_STANDARD, t))
        else {
            continue;
        };
        *tile_type = Value::String(standard_tile.to_string());
        let anchor_key = cell_to_anchor
            .as_ref()
            .ok_or("world.cellToAnchor is undefined")?
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_string();
        if !vip_anchors.contains(&anchor_key) {
            vip_anchors.push(anchor_key);
        }
    }

    for (anchor_key, record) in field_obj(world, "placedObjects", "world")?.iter_mut() {
        if vip_anchors.contains(anchor_key) {
            let record = record.as_object_mut().ok_or("a placed object is not an object")?;
            match num(record.get("objectTypeCode")) {
                Some(31) => {
                    record.insert("objectTypeCode".into(), json!(3));
                }
                Some(32) => {
                    record.insert("objectTypeCode".into(), json!(4));
                }
                Some(33) => {
                    record.insert("objectTypeCode".into(), json!(5));
                }
                _ => {}
            }
            record.insert("vipFlag".into(), json!(true));
        }
    }
    let mut metro_station_floor_index: Option<i64> = None;
    for (key, record) in field_obj(world, "placedObjects", "world")?.iter() {
        if num(record.get("objectTypeCode")) != Some(FAMILY_METRO_TOP) {
            continue;
        }
        let y = key_y(key)?;
        metro_station_floor_index = Some(GRID_HEIGHT - 1 - y);
        break;
    }
    {
        let gate_flags = field_obj(world, "gateFlags", "world")?;
        gate_flags.insert("metroPlaced".into(), json!(0));
        gate_flags.insert("metroStationFloorIndex".into(), json!(-1));
        if let Some(floor) = metro_station_floor_index {
            gate_flags.insert("metroPlaced".into(), json!(1));
            gate_flags.insert("metroStationFloorIndex".into(), json!(floor));
        }
    }

    // Migrate carrier floorQueues from old flat format to RouteRequestRing instances
    for carrier in field_arr(world, "carriers", "world")?.iter_mut() {
        let carrier = carrier.as_object_mut().ok_or("a carrier is not an object")?;
        for car in field_arr(carrier, "cars", "carrier")?.iter_mut() {
            let car = car.as_object_mut().ok_or("a car is not an object")?;
            nullish_assign(car, "dwellStartPendingAssignmentCount", || json!(0));
            nullish_assign(car, "arrivalDispatchThisTick", || json!(false));
            nullish_assign(car, "arrivalDispatchStartingAssignedCount", || json!(0));
            nullish_assign(car, "suppressDwellOppositeDirectionFlip", || json!(false));
        }
        for q in field_arr(carrier, "floorQueues", "carrier")?.iter_mut() {
            // A parsed queue is never a RouteRequestRing instance (nor is one
            // that structuredClone copied), so every queue is rebuilt.
            if falsy(Some(q)) {
                continue;
            }
            let Some(qo) = q.as_object() else {
                continue;
            };
            if qo.contains_key("upQueueRouteIds") {
                // Old format: {upCount, upHeadIndex, downCount, downHeadIndex, upQueueRouteIds, downQueueRouteIds}
                // New format: {up: RouteRequestRing, down: RouteRequestRing}
                let up_buf = ring_from(
                    qo.get("upQueueRouteIds"),
                    num(qo.get("upHeadIndex")).unwrap_or(0),
                    num(qo.get("upCount")).unwrap_or(0),
                )?;
                let down_buf = ring_from(
                    qo.get("downQueueRouteIds"),
                    num(qo.get("downHeadIndex")).unwrap_or(0),
                    num(qo.get("downCount")).unwrap_or(0),
                )?;
                *q = json!({ "up": up_buf, "down": down_buf });
            } else if qo.contains_key("up") && qo.contains_key("down") {
                // Already new shape but plain objects from JSON deserialization
                let ring = |r: &Value| -> R<Value> {
                    let head = num(r.get("head")).ok_or("RouteRequestRing.from: head is not a number")?;
                    let count = num(r.get("count")).ok_or("RouteRequestRing.from: count is not a number")?;
                    ring_from(r.get("items"), head, count)
                };
                let up = ring(&qo["up"])?;
                let down = ring(&qo["down"])?;
                *q = json!({ "up": up, "down": down });
            }
        }
    }

    Ok(())
}

/// Upstream's hydrateSnapshot for a saved game's JSON.
pub fn hydrate_snapshot_json(mut raw: Value) -> R<SimSnapshot> {
    normalize_snapshot(&mut raw)?;
    let snapshot = raw.as_object_mut().unwrap();

    {
        let world = field_obj(snapshot, "world", "snapshot")?;
        if num(world.get("height")).is_some_and(|h| h < GRID_HEIGHT) {
            world.insert("height".into(), json!(GRID_HEIGHT));
        }
        nullish_assign(world, "placedObjects", || json!({}));
        nullish_assign(world, "sidecars", || json!([]));
        nullish_assign(world, "sims", || json!([]));
    }

    // (normalize_snapshot has always made a ledger by now; upstream's
    // fallback is kept as it is.)
    if falsy(snapshot.get("ledger")) {
        let world = field_obj(snapshot, "world", "snapshot")?;
        let cash = num(world.get("cash")).unwrap_or(2_000_000);
        delete(world, "cash");
        snapshot.insert("ledger".into(), to_json(create_ledger_state(cash)));
    }

    // Upstream calls initCarrierState(world) here.  It only fills in
    // world.carriers, floorWalkabilityFlags and transferGroupCache when
    // absent, which normalize_snapshot has made sure they are not, so it is
    // called below on the typed world instead, with the same effect.
    let world = field_obj(snapshot, "world", "snapshot")?;
    for carrier in field_arr(world, "carriers", "world")?.iter_mut() {
        let carrier = carrier.as_object_mut().ok_or("a carrier is not an object")?;
        nullish_assign(carrier, "completedRouteIds", || json!([]));
        nullish_assign(carrier, "suppressedFloorAssignments", || json!([]));
        let top = num(carrier.get("topServedFloor"));
        let bottom = num(carrier.get("bottomServedFloor"));
        // Math.max(0, top - bottom + 1); NaN (a missing floor) gives NaN,
        // and `new Array(NaN)` throws.
        let span = match (top, bottom) {
            (Some(t), Some(b)) => 0.max(t - b + 1),
            _ => return Err("carrier served floors are not numbers".into()),
        };
        let num_slots = span as usize;
        if carrier
            .get("stopFloorEnabled")
            .and_then(Value::as_array)
            .is_none_or(|a| a.len() != num_slots)
        {
            carrier.insert("stopFloorEnabled".into(), json!(vec![1; num_slots]));
        }
        if carrier
            .get("dwellDelay")
            .and_then(Value::as_array)
            .is_none_or(|a| a.len() != 14)
        {
            carrier.insert("dwellDelay".into(), json!(vec![0; 14]));
        }
        if let Some(routes) = carrier.get_mut("pendingRoutes").and_then(Value::as_array_mut) {
            for route in routes {
                if let Some(route) = route.as_object_mut() {
                    nullish_assign(route, "assignedCarIndex", || json!(-1));
                }
            }
        }
        if let Some(cars) = carrier.get_mut("cars").and_then(Value::as_array_mut) {
            for car in cars {
                let Some(car) = car.as_object_mut() else {
                    continue;
                };
                nullish_assign(car, "active", || json!(true));
                nullish_assign(car, "pendingAssignmentCount", || json!(0));
                if nullish(car.get("homeFloor")) {
                    let home = match car.get("currentFloor") {
                        None | Some(Value::Null) => json!(bottom),
                        Some(v) => v.clone(),
                    };
                    car.insert("homeFloor".into(), home);
                }
                if nullish(car.get("nearestWorkFloor")) {
                    let home = car["homeFloor"].clone();
                    car.insert("nearestWorkFloor".into(), home);
                }
                nullish_assign(car, "destinationCountByFloor", || json!(vec![0; num_slots]));
                nullish_assign(car, "activeRouteSlots", || json!([]));
            }
        }
    }

    nullish_assign(world, "specialLinks", || to_json(create_empty_special_links()));
    nullish_assign(world, "specialLinkRecords", || to_json(create_empty_special_link_records()));
    nullish_assign(world, "transferGroupEntries", || to_json(create_empty_transfer_group_entries()));
    nullish_assign(world, "parkingDemandLog", || json!([]));
    for sim in field_arr(world, "sims", "world")?.iter_mut() {
        let sim = sim.as_object_mut().ok_or("a sim is not an object")?;
        nullish_assign(sim, "elapsedTicks", || json!(0));
        nullish_assign(sim, "targetRoomFloor", || json!(-1));
        nullish_assign(sim, "targetRoomColumn", || json!(-1));
        if nullish(sim.get("spawnFloor")) {
            let anchor = sim.get("floorAnchor").cloned().unwrap_or(Value::Null);
            sim.insert("spawnFloor".into(), anchor);
        }
        nullish_assign(sim, "postClaimCountdown", || json!(0));
        nullish_assign(sim, "encodedTargetFloor", || json!(0));
        // Migrate old fields away
        delete(sim, "stressCounter");
        delete(sim, "visitCounter");
    }
    for (_, obj) in field_obj(world, "placedObjects", "world")?.iter_mut() {
        let Some(raw) = obj.as_object_mut() else {
            continue;
        };
        if let Some(v) = raw.get("pairingPendingFlag").cloned() {
            nullish_assign(raw, "housekeepingClaimedFlag", || v);
            delete(raw, "pairingPendingFlag");
        }
        nullish_assign(raw, "housekeepingClaimedFlag", || json!(0));
        nullish_assign(raw, "evalScore", || json!(-1));
    }
    nullish_assign(world, "starCount", || json!(1));
    nullish_assign(world, "currentPopulation", || json!(0));
    nullish_assign(world, "currentPopulationBuckets", || json!({}));
    nullish_assign(world, "rngState", || json!(1));
    nullish_assign(world, "rngCallCount", || json!(0));
    nullish_assign(world, "eventState", || to_json(create_event_state()));

    let mut snapshot: SimSnapshot = serde_json::from_value(raw).map_err(|e| format!("snapshot: {e}"))?;

    init_carrier_state(&mut snapshot.world);
    rebuild_special_link_route_records(&mut snapshot.world);
    rebuild_route_reachability_tables(&mut snapshot.world);
    rebuild_transfer_group_cache(&mut snapshot.world);
    rebuild_runtime_sims(&mut snapshot.world);

    Ok(snapshot)
}

/// Upstream's hydrateSnapshot as TowerSim.fromSnapshot calls it, on a
/// structuredClone of a snapshot.  structuredClone keeps what JSON would
/// not: the income ledger's holes and NaNs, which are put back after the
/// trip through the JSON (nothing in hydration touches that ledger when it
/// is present).
pub fn hydrate_snapshot(raw: SimSnapshot) -> SimSnapshot {
    let income_ledger = raw.ledger.income_ledger.clone();
    let mut snapshot =
        hydrate_snapshot_json(to_json(&raw)).expect("a typed snapshot always has every field hydration reads");
    snapshot.ledger.income_ledger = income_ledger;
    snapshot
}

pub fn serialize_sim_state(time: &TimeState, world: &WorldState, ledger: &LedgerState) -> SimSnapshot {
    SimSnapshot {
        time: time.clone(),
        world: WorldState {
            tower_id: world.tower_id.clone(),
            name: world.name.clone(),
            width: world.width,
            height: world.height,
            lobby_height: world.lobby_height,
            lobby_mode: world.lobby_mode,
            gate_flags: world.gate_flags.clone(),
            cells: world.cells.clone(),
            cell_to_anchor: world.cell_to_anchor.clone(),
            overlays: world.overlays.clone(),
            overlay_to_anchor: world.overlay_to_anchor.clone(),
            placed_objects: world.placed_objects.clone(),
            sidecars: world.sidecars.clone(),
            sims: world.sims.clone(),
            carriers: world.carriers.clone(),
            special_links: world.special_links.clone(),
            special_link_records: world.special_link_records.clone(),
            floor_walkability_flags: world.floor_walkability_flags.clone(),
            transfer_group_entries: world.transfer_group_entries.clone(),
            transfer_group_cache: world.transfer_group_cache.clone(),
            parking_demand_log: world.parking_demand_log.clone(),
            medical_service_slots: world.medical_service_slots.clone(),
            star_count: world.star_count,
            current_population: world.current_population,
            current_population_buckets: world.current_population_buckets.clone(),
            commercial_venue_buckets: world.commercial_venue_buckets.clone(),
            rng_state: world.rng_state,
            rng_call_count: world.rng_call_count,
            event_state: world.event_state.clone(),
            pending_notifications: Vec::new(),
            pending_prompts: Vec::new(),
            box_ext: world.box_ext.clone(), // BOX: car elevator
            sim_owner_cache: Default::default(),
            detached_objects: Default::default(),
        },
        ledger: LedgerState {
            cash_balance: ledger.cash_balance,
            population_ledger: ledger.population_ledger.clone(),
            income_ledger: ledger.income_ledger.clone(),
            expense_ledger: ledger.expense_ledger.clone(),
            cash_balance_cycle_base: ledger.cash_balance_cycle_base,
            box_cash_cap: ledger.box_cash_cap,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_to_camel_matches_the_regex() {
        assert_eq!(snake_to_camel("tower_id"), "towerId");
        assert_eq!(snake_to_camel("a__b"), "a_B");
        assert_eq!(snake_to_camel("phase_a_seed"), "phaseASeed");
        assert_eq!(snake_to_camel("x_1"), "x_1");
        assert_eq!(snake_to_camel("_x"), "X");
    }

    #[test]
    fn normalize_leaves_a_current_snapshot_alone() {
        let snap = create_initial_snapshot("t", "Tower", 2_000_000, Some(LobbyMode::PerfectParity));
        let mut v = to_json(&snap);
        normalize_snapshot(&mut v).unwrap();
        let back: SimSnapshot = serde_json::from_value(v).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn normalize_migrates_a_legacy_snapshot() {
        let snap = create_initial_snapshot("t", "Tower", 5, None);
        let mut v = to_json(&snap);
        {
            let o = v.as_object_mut().unwrap();
            // snake_case keys, star count in time, an old ledger name.
            let w = o["world"].as_object_mut().unwrap();
            let id = w.shift_remove("towerId").unwrap();
            w.insert("tower_id".into(), id);
            w.shift_remove("lobbyMode");
            w["placedObjects"] = json!({
                "10,100": {"leftTileIndex": 10, "rightTileIndex": 39, "objectTypeCode": 31,
                           "stay_phase": 2, "auxValueOrTimer": 0, "linkedRecordIndex": -1,
                           "pairingStatus": 1, "variantIndex": 2, "activationTickCount": 0,
                           "pairingActiveFlag": 0, "housekeepingClaimedFlag": 0},
            });
            w["cells"] = json!({"10,100": "metro", "11,100": "vipSingle"});
            w["cellToAnchor"] = json!({"11,100": "10,100"});
            w["carriers"] = json!([{
                "carrierId": 0, "column": 4, "carrierMode": 1, "topServedFloor": 12,
                "bottomServedFloor": 10, "servedFloorFlags": [], "primaryRouteStatusByFloor": [],
                "secondaryRouteStatusByFloor": [], "serviceScheduleFlags": [], "dwellDelay": [],
                "expressDirectionFlags": [], "waitingCarResponseThreshold": 0, "assignmentCapacity": 0,
                "floorQueues": [{"upCount": 1, "upHeadIndex": 39, "upQueueRouteIds": ["a", null],
                                 "downCount": 0, "downQueueRouteIds": []}],
                "pendingRoutes": [], "stopFloorEnabled": [], "cars": [],
            }]);
            let t = o["time"].as_object_mut().unwrap();
            t.insert("starCount".into(), json!(3));
            o["world"].as_object_mut().unwrap().shift_remove("starCount");
            let l = o["ledger"].as_object_mut().unwrap();
            let p = l.shift_remove("populationLedger").unwrap();
            l.insert("primaryLedger".into(), p);
        }
        normalize_snapshot(&mut v).unwrap();
        let w = &v["world"];
        assert_eq!(w["towerId"], "t");
        assert_eq!(w["lobbyMode"], "perfect-parity");
        assert_eq!(w["starCount"], 3);
        assert!(v["time"].get("starCount").is_none());
        assert_eq!(v["ledger"]["populationLedger"], json!(vec![0; 10]));
        let r = &w["placedObjects"]["10,100"];
        assert_eq!(r["unitStatus"], 2);
        assert_eq!((r["dirtyFlag"].clone(), r["occupiedFlag"].clone()), (json!(0), json!(0)));
        assert!(r.get("occupiableFlag").is_none());
        assert_eq!((r["evalLevel"].clone(), r["rentLevel"].clone(), r["evalScore"].clone()), (json!(1), json!(2), json!(-1)));
        // The VIP cell's anchor is the record: hotel single, flagged VIP.
        assert_eq!(w["cells"]["11,100"], "hotelSingle");
        assert_eq!((r["objectTypeCode"].clone(), r["vipFlag"].clone()), (json!(3), json!(true)));
        // The old flat queue became two rings.
        let q = &w["carriers"][0]["floorQueues"][0];
        assert_eq!((q["up"]["head"].clone(), q["up"]["count"].clone()), (json!(39), json!(1)));
        assert_eq!(q["up"]["items"][0], "a");
        assert_eq!(q["up"]["items"][1], "");
        assert_eq!(q["down"]["capacity"], 40);
    }

    #[test]
    fn normalize_finds_the_metro() {
        let mut v = to_json(create_initial_snapshot("t", "t", 0, None));
        v["world"]["placedObjects"] = json!({"3,105": {"objectTypeCode": FAMILY_METRO_TOP}});
        normalize_snapshot(&mut v).unwrap();
        assert_eq!(v["world"]["gateFlags"]["metroPlaced"], 1);
        assert_eq!(v["world"]["gateFlags"]["metroStationFloorIndex"], GRID_HEIGHT - 1 - 105);
    }

    #[test]
    fn serialize_drops_pending_messages() {
        let mut s = create_initial_snapshot("t", "t", 0, None);
        s.world.pending_notifications.push(crate::world::SimNotification {
            kind: crate::world::SimNotificationKind::Event,
            message: None,
        });
        let out = serialize_sim_state(&s.time, &s.world, &s.ledger);
        assert!(out.world.pending_notifications.is_empty());
        s.world.pending_notifications.clear();
        assert_eq!(out, s);
    }
}

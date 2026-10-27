// ts: sims/population.ts

use std::collections::{HashMap, VecDeque};

use indexmap::IndexSet;

use crate::js;
use crate::resources::{
    FAMILY_CATHEDRAL_BASE, FAMILY_CINEMA, FAMILY_CONDO, FAMILY_HOUSEKEEPING, FAMILY_MEDICAL,
    FAMILY_OFFICE, FAMILY_PARTY_HALL, FAMILY_RECYCLING_CENTER_UPPER, FAMILY_SECURITY,
};
use crate::sims::states::{
    CATHEDRAL_FAMILIES, COMMERCIAL_FAMILIES, ENTITY_POPULATION_BY_TYPE, HK_STATE_SEARCH,
    HOTEL_FAMILIES, ROUTE_IDLE, STATE_ACTIVE, STATE_HOTEL_PARKED, STATE_MORNING_GATE,
    STATE_PARKED,
};
use crate::sims::trip_counters::reset_sim_trip_counters;
use crate::world::{y_to_floor, CarrierRecord, SimRecord, WorldState, GRID_HEIGHT, GROUND_Y};

/// The x and y of an "x,y" key (`key.split(",").map(Number)`).
fn key_xy(key: &str) -> (i64, i64) {
    let mut parts = key.split(',').map(|p| {
        p.parse::<i64>()
            .unwrap_or_else(|_| panic!("key {key:?} is not \"x,y\""))
    });
    let x = parts.next().unwrap();
    let y = parts.next().unwrap_or_else(|| panic!("key {key:?} is not \"x,y\""));
    (x, y)
}

fn make_sim(
    floor_anchor: i64,
    home_column: i64,
    base_offset: i64,
    family_code: i64,
    population: i64,
    facility_slot: i64,
) -> SimRecord {
    SimRecord {
        floor_anchor,
        home_column,
        base_offset,
        facility_slot,
        family_code,
        state_code: initial_state_for_family(family_code, base_offset, population),
        route: ROUTE_IDLE,
        selected_floor: floor_anchor,
        origin_floor: floor_anchor,
        destination_floor: -1,
        venue_return_state: 0,
        queue_tick: 0,
        elapsed_ticks: 0,
        transit_ticks_remaining: 0,
        last_demand_tick: -1,
        trip_count: 0,
        accumulated_ticks: 0,
        target_room_floor: -1,
        target_room_column: -1,
        spawn_floor: floor_anchor,
        post_claim_countdown: 0,
        encoded_target_floor: 0,
        commercial_venue_slot: -1,
    }
}

fn initial_state_for_family(family_code: i64, base_offset: i64, _population: i64) -> i64 {
    if HOTEL_FAMILIES.contains(&family_code) {
        // Binary NIGHT_B handler: base_offset == 0 → HOTEL_PARKED, others → MORNING_GATE.
        return if base_offset == 0 { STATE_HOTEL_PARKED } else { STATE_MORNING_GATE };
    }
    if CATHEDRAL_FAMILIES.contains(&family_code) {
        return STATE_PARKED;
    }
    if family_code == FAMILY_OFFICE {
        return STATE_MORNING_GATE;
    }
    if family_code == FAMILY_CONDO {
        return STATE_MORNING_GATE;
    }
    if COMMERCIAL_FAMILIES.contains(&family_code) {
        return STATE_MORNING_GATE;
    }
    if family_code == FAMILY_RECYCLING_CENTER_UPPER || family_code == FAMILY_SECURITY {
        return STATE_ACTIVE;
    }
    if family_code == FAMILY_HOUSEKEEPING {
        return HK_STATE_SEARCH;
    }
    STATE_PARKED
}

pub fn sim_key(sim: &SimRecord) -> String {
    format!(
        "{}:{}:{}:{}",
        sim.floor_anchor, sim.home_column, sim.family_code, sim.base_offset
    )
}

fn object_key(sim: &SimRecord) -> String {
    let y = GRID_HEIGHT - 1 - sim.floor_anchor;
    format!("{},{}", sim.home_column, y)
}

// Upstream keeps `const simOwnerCache = new WeakMap<SimRecord,
// PlacedObjectRecord>()` here: the first successful lookup for a sim object
// is remembered for as long as that sim object lives, and later lookups
// return the remembered PlacedObjectRecord even if it has since been removed
// from (or replaced in) `world.placedObjects`.  Sim objects are recreated by
// rebuildRuntimeSims (every placement and demolition runs it through
// runGlobalRebuilds, and hydrateSnapshot runs it on load), so the cache and
// a fresh lookup agree everywhere except after events.ts deletes a placed
// object (fire spread, bomb) without a rebuild: there upstream's sims keep
// working on the detached record and this lookup finds nothing.  That case
// cannot be expressed without a runtime-only side table on the world (the
// integrator's decision; see the report), so this is the plain lookup and
// returns the object's key.
//
// Returns the key of the sim's home object in `world.placed_objects`.
/// Upstream returns the object; here its key, which `world.object(key)`
/// reaches.  Upstream caches each sim's object in a WeakMap
/// (`simOwnerCache`), so a sim keeps the object it first found even after
/// that object is deleted or replaced: `world.sim_owner_cache` and
/// `world.detached_objects` (world.rs) carry that.
pub fn find_object_for_sim(world: &WorldState, sim_index: i64) -> Option<String> {
    if let Some(cached) = world.sim_owner_cache.get(sim_index as usize) {
        return Some(cached);
    }
    let key = object_key(&world.sims[sim_index as usize]);
    if world.placed_objects.contains_key(&key) {
        world.sim_owner_cache.set(sim_index as usize, key.clone());
        Some(key)
    } else {
        None
    }
}

/// Upstream returns the sibling sim objects themselves (the sim included);
/// here their indices in `world.sims`, in the same order.
pub fn find_sibling_sims(world: &WorldState, sim_index: i64) -> Vec<i64> {
    let sim = &world.sims[sim_index as usize];
    world
        .sims
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            candidate.floor_anchor == sim.floor_anchor
                && candidate.home_column == sim.home_column
                && candidate.family_code == sim.family_code
        })
        .map(|(i, _)| i as i64)
        .collect()
}

pub fn clear_sim_route(sim: &mut SimRecord) {
    sim.route = ROUTE_IDLE;
    // Phase 5b: `sim.stateCode` bits are NOT cleared here. The binary's
    // `dispatch_destination_queue_entries` sets state_code bits via the
    // family handler it dispatches into (e.g. hotel state-0x60 → 0x01 on
    // arrival), so the family arrival handler — called right after
    // clearSimRoute — is responsible for any state-bit changes. Stripping
    // 0x40 unconditionally here corrupts the _TRANSIT phase byte that the
    // handler still needs to read to identify the arrival branch (e.g.
    // `sim.stateCode === STATE_MORNING_TRANSIT` in hotel.ts#handleHotelSimArrival).
    // Explicit state-bit clears are paired at the specific sites that park
    // the sim (resetSimRuntimeState, cleanupSimsForRemovedTile,
    // maybe-dispatch-after-wait, and the idle transitions inside family
    // state handlers).
}

fn clear_carrier_slots_for_removed_sims(carrier: &mut CarrierRecord, removed_ids: &IndexSet<String>) {
    carrier
        .pending_routes
        .retain(|route| !removed_ids.contains(&route.sim_id));
    for car in carrier.cars.iter_mut() {
        for slot in car.active_route_slots.iter_mut() {
            if !slot.active {
                continue;
            }
            if !removed_ids.contains(&slot.route_id) {
                continue;
            }
            if slot.boarded {
                car.assigned_count = 0.max(car.assigned_count - 1);
            }
            slot.active = false;
            slot.route_id = String::new();
            slot.source_floor = 0xff;
            slot.destination_floor = 0xff;
            slot.boarded = false;
        }
        car.pending_route_ids.retain(|id| !removed_ids.contains(id));
    }
}

// Binary place_object_on_floor maintains a 10-entry FIFO of pending sim-slot
// reservations. Each placement (a) first-fit allocates a contiguous block of
// sim-record slots sized to its population (6 for medical, which reserves
// slots but fills none), (b) writes placeholder records tagged with the
// facility's (floor, facilitySlot), (c) enqueues the block. When the queue
// is about to exceed 10 entries, the oldest is finalized: occupant placeholders
// promote to live sims in place; medical placeholders are zeroed, freeing
// their slots for a later placement's first-fit to reclaim. This is what
// causes ~one office per medical to end up with sim indices earlier than its
// nominal placement order. See medical-allocation.md for the empirical
// derivation.
const PLACEMENT_QUEUE_SIZE: usize = 10;
const MEDICAL_SLOT_WIDTH: i64 = 6;

struct QueueEntry {
    slot_indices: Vec<i64>,
    zero_on_finalize: bool,
}

fn first_fit_slot(occupied: &[bool], width: i64) -> i64 {
    let mut run = 0;
    let mut run_start = -1;
    for (i, &taken) in occupied.iter().enumerate() {
        if !taken {
            if run == 0 {
                run_start = i as i64;
            }
            run += 1;
            if run == width {
                return run_start;
            }
        } else {
            run = 0;
        }
    }
    occupied.len() as i64
}

pub fn rebuild_runtime_sims(world: &mut WorldState) {
    // `new Map(world.sims.map(...))`: a repeated key keeps the last sim.
    // Only lookups are made, so a HashMap serves.
    let mut previous: HashMap<String, SimRecord> = HashMap::new();
    for sim in &world.sims {
        previous.insert(sim_key(sim), sim.clone());
    }

    // Sort by binary floor-by-floor allocation order: above-grade floors
    // ascending (y descending), then below-grade floors ascending (y ascending),
    // then x ascending within the same floor. This matches the binary's
    // entity table layout from _place_build_objects.
    let mut sorted_entries: Vec<(&str, i64)> = world
        .placed_objects
        .iter()
        .map(|(key, object)| (key, object.object_type_code))
        .collect();
    sorted_entries.sort_by(|(a, _), (b, _)| {
        let (ax, ay) = key_xy(a);
        let (bx, by) = key_xy(b);
        let a_below = if ay > GROUND_Y { 1 } else { 0 };
        let b_below = if by > GROUND_Y { 1 } else { 0 };
        if a_below != b_below {
            return a_below.cmp(&b_below);
        }
        if a_below == 0 {
            if ay != by {
                return by.cmp(&ay);
            }
        } else if ay != by {
            return ay.cmp(&by);
        }
        ax.cmp(&bx)
    });

    let mut slot_by_floor: HashMap<i64, i64> = HashMap::new();
    let mut slots: Vec<Option<SimRecord>> = Vec::new();
    let mut occupied: Vec<bool> = Vec::new();
    let mut queue: VecDeque<QueueEntry> = VecDeque::new();

    for &(key, family_code) in &sorted_entries {
        let runtime_family_code = if CATHEDRAL_FAMILIES.contains(&family_code) {
            FAMILY_CATHEDRAL_BASE
        } else {
            family_code
        };
        let is_medical = family_code == FAMILY_MEDICAL;
        let population = js::record_get(ENTITY_POPULATION_BY_TYPE, family_code)
            .copied()
            .unwrap_or(0);
        if population == 0 && !is_medical {
            continue;
        }

        let (x, y) = key_xy(key);
        let floor_anchor = y_to_floor(y);
        let facility_slot = slot_by_floor.get(&floor_anchor).copied().unwrap_or(0);
        slot_by_floor.insert(floor_anchor, facility_slot + 1);

        let slot_width = if is_medical { MEDICAL_SLOT_WIDTH } else { population };
        let start = first_fit_slot(&occupied, slot_width);
        let mut slot_indices: Vec<i64> = Vec::new();
        for j in 0..slot_width {
            let idx = start + j;
            while slots.len() as i64 <= idx {
                slots.push(None);
                occupied.push(false);
            }
            occupied[idx as usize] = true;
            slot_indices.push(idx);
            if is_medical {
                slots[idx as usize] = None;
                continue;
            }
            let mut fresh = make_sim(floor_anchor, x, j, runtime_family_code, population, facility_slot);
            if let Some(prior) = previous.get(&sim_key(&fresh)) {
                // `{ ...fresh, ...prior, floorAnchor, homeColumn: x, facilitySlot }`:
                // every field of prior wins, then the three are overwritten.
                let mut merged = prior.clone();
                merged.floor_anchor = floor_anchor;
                merged.home_column = x;
                merged.facility_slot = facility_slot;
                slots[idx as usize] = Some(merged);
            } else {
                fresh.trip_count = 0;
                fresh.accumulated_ticks = 0;
                slots[idx as usize] = Some(fresh);
            }
        }

        queue.push_back(QueueEntry {
            slot_indices,
            zero_on_finalize: is_medical,
        });
        if queue.len() > PLACEMENT_QUEUE_SIZE {
            let oldest = queue.pop_front();
            if let Some(oldest) = oldest {
                if oldest.zero_on_finalize {
                    for &i in &oldest.slot_indices {
                        occupied[i as usize] = false;
                        slots[i as usize] = None;
                    }
                }
            }
        }
    }

    world.sims = slots.into_iter().flatten().collect();
    // New sim objects: upstream's WeakMap holds none of them.
    world.forget_sim_owners();
}

pub fn cleanup_sims_for_removed_tile(world: &mut WorldState, anchor_x: i64, y: i64) {
    let floor_anchor = y_to_floor(y);
    let mut removed_ids: IndexSet<String> = IndexSet::new();

    let mut keep: Vec<bool> = Vec::with_capacity(world.sims.len());
    world.sims.retain_mut(|sim| {
        if sim.home_column != anchor_x || sim.floor_anchor != floor_anchor {
            keep.push(true);
            return true;
        }
        clear_sim_route(sim);
        sim.destination_floor = -1;
        removed_ids.insert(sim_key(sim));
        keep.push(false);
        false
    });
    // The surviving sim objects are the same objects: their cache entries stay.
    world.sim_owner_cache.retain(&keep);

    if removed_ids.is_empty() {
        return;
    }

    for carrier in world.carriers.iter_mut() {
        clear_carrier_slots_for_removed_sims(carrier, &removed_ids);
    }
}

pub fn reset_sim_runtime_state(world: &mut WorldState) {
    for sim_index in 0..world.sims.len() as i64 {
        if find_object_for_sim(world, sim_index).is_none() {
            continue;
        }
        let sim = &mut world.sims[sim_index as usize];
        let clears_daily_stress = COMMERCIAL_FAMILIES.contains(&sim.family_code)
            || sim.family_code == FAMILY_SECURITY
            || sim.family_code == FAMILY_HOUSEKEEPING
            || sim.family_code == FAMILY_CINEMA
            || sim.family_code == FAMILY_PARTY_HALL
            || sim.family_code == 0x21
            || sim.family_code == FAMILY_CATHEDRAL_BASE;
        if clears_daily_stress {
            reset_sim_trip_counters(sim);
        }

        if HOTEL_FAMILIES.contains(&sim.family_code) {
            // Binary 1228:0000 reset_sim_runtime_state, family 3/4/5 case:
            // if get_current_sim_state_word(sim) == 0 → stateCode = 0x24
            // (STATE_HOTEL_PARKED). Other branches (room+0xb gate → 0x20/0x10)
            // are left to the existing day-boundary state machine.
            if sim.base_offset == 0 {
                sim.state_code = STATE_HOTEL_PARKED;
            }
            continue;
        } else if sim.family_code == FAMILY_CONDO {
            // Condos persist across day boundaries via CHECKOUT_QUEUE→TRANSITION.
            continue;
        } else if sim.family_code == FAMILY_HOUSEKEEPING {
            sim.state_code = HK_STATE_SEARCH;
            sim.target_room_floor = -1;
            sim.spawn_floor = sim.floor_anchor;
            sim.post_claim_countdown = 0;
        } else if sim.family_code == FAMILY_OFFICE || COMMERCIAL_FAMILIES.contains(&sim.family_code) {
            // Spec TIME.md checkpoint 2500: family 6/7/10/12 → 0x20 (MORNING_GATE).
            sim.state_code = STATE_MORNING_GATE;
        } else if sim.family_code == FAMILY_RECYCLING_CENTER_UPPER || sim.family_code == FAMILY_SECURITY {
            // Stationary sims keep their current state across day boundaries.
            continue;
        } else {
            sim.state_code = STATE_PARKED;
        }

        sim.selected_floor = sim.floor_anchor;
        sim.origin_floor = sim.floor_anchor;
        sim.route = ROUTE_IDLE;
        // Phase 5b note: state_code bit cleanup is NOT done here. The reset
        // block above already wrote a fresh stateCode (MORNING_GATE / PARKED
        // / HK_SEARCH). In the TS encoding MORNING_GATE (0x20) overlaps with
        // the binary's "waiting" bit — stripping 0x20 here would corrupt the
        // post-reset phase byte. The explicit byte-write is the authoritative
        // source; the bit helpers are reserved for transitions that keep the
        // base phase constant (e.g. `finalizeRuntimeRouteState`).
        sim.destination_floor = -1;
        sim.venue_return_state = 0;
        sim.queue_tick = 0;
        sim.elapsed_ticks = 0;
        sim.transit_ticks_remaining = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{FAMILY_HOTEL_SINGLE, FAMILY_OFFICE};
    use crate::world::{floor_to_y, PlacedObjectRecord};

    fn object(x: i64, width: i64, code: i64) -> PlacedObjectRecord {
        PlacedObjectRecord {
            left_tile_index: x,
            right_tile_index: x + width - 1,
            object_type_code: code,
            unit_status: 0,
            aux_value_or_timer: 0,
            linked_record_index: -1,
            dirty_flag: 0,
            occupied_flag: 0,
            eval_level: 0,
            eval_score: -1,
            rent_level: 1,
            activation_tick_count: 0,
            housekeeping_claimed_flag: 0,
            vip_flag: None,
        }
    }

    #[test]
    fn rebuild_orders_sims_and_keeps_prior_state() {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        w.placed_objects.clear();
        w.sims.clear();
        // Placed out of order: below grade, then two above grade.
        w.placed_objects.insert(format!("5,{}", floor_to_y(8)), object(5, 9, FAMILY_OFFICE));
        w.placed_objects.insert(format!("30,{}", floor_to_y(12)), object(30, 4, FAMILY_HOTEL_SINGLE));
        w.placed_objects.insert(format!("20,{}", floor_to_y(12)), object(20, 9, FAMILY_OFFICE));
        rebuild_runtime_sims(&mut w);
        let order: Vec<(i64, i64, i64)> =
            w.sims.iter().map(|s| (s.floor_anchor, s.home_column, s.base_offset)).collect();
        // Above grade first (floor 12: x 20 then 30), then below grade.
        assert_eq!(order.len(), 6 + 2 + 6);
        assert_eq!(order[0], (12, 20, 0));
        assert_eq!(order[6], (12, 30, 0));
        assert_eq!(order[8], (8, 5, 0));
        assert_eq!(w.sims[6].state_code, STATE_HOTEL_PARKED);
        assert_eq!(w.sims[7].state_code, STATE_MORNING_GATE);
        assert_eq!(w.sims[6].facility_slot, 1);
        // Prior runtime state survives a rebuild.
        w.sims[1].trip_count = 7;
        w.sims[1].state_code = 0x05;
        rebuild_runtime_sims(&mut w);
        assert_eq!((w.sims[1].trip_count, w.sims[1].state_code), (7, 0x05));
        assert_eq!(sim_key(&w.sims[1]), "12:20:7:1");
        assert_eq!(find_object_for_sim(&w, 1), Some(format!("20,{}", floor_to_y(12))));
        assert_eq!(find_sibling_sims(&w, 7), vec![6, 7]);
        cleanup_sims_for_removed_tile(&mut w, 20, floor_to_y(12));
        assert_eq!(w.sims.len(), 8);
    }

    #[test]
    fn medical_reserves_slots_until_finalised() {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        w.placed_objects.clear();
        // A medical centre first on floor 20, then eleven offices above it:
        // the medical block is freed when it leaves the ten-entry queue, and
        // the next office's first-fit takes its slots.
        w.placed_objects.insert(format!("0,{}", floor_to_y(20)), object(0, 26, FAMILY_MEDICAL));
        for i in 0..11 {
            w.placed_objects.insert(format!("0,{}", floor_to_y(21 + i)), object(0, 9, FAMILY_OFFICE));
        }
        rebuild_runtime_sims(&mut w);
        assert_eq!(w.sims.len(), 66);
        // Slots 0..6 were the medical block; the 11th office (floor 31) lands there.
        assert_eq!(w.sims[0].floor_anchor, 31);
        assert_eq!(w.sims[6].floor_anchor, 21);
    }

    #[test]
    fn first_fit() {
        assert_eq!(first_fit_slot(&[true, false, false, true, false, false, false], 3), 4);
        assert_eq!(first_fit_slot(&[true, false], 2), 2);
        assert_eq!(first_fit_slot(&[], 6), 0);
    }
}

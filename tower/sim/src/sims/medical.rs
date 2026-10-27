// ts: sims/medical.ts

use indexmap::IndexMap;

use crate::js;
use crate::queue::resolve::resolve_sim_route_between_floors;
use crate::resources::FAMILY_MEDICAL;
use crate::sims::population::{clear_sim_route, sim_key};
use crate::sims::release_service_request;
use crate::sims::states::{LOBBY_FLOOR, STATE_DEPARTURE, STATE_DEPARTURE_TRANSIT, STATE_NIGHT_B, STATE_PARKED};
use crate::stress::add_delay::add_delay_to_current_sim;
use crate::stress::trip_counters::advance_sim_trip_counters;
use crate::time::TimeState;
use crate::world::{
    sample_rng, y_to_floor, MedicalCenterRecord, SidecarRecord, SimNotification, SimNotificationKind,
    WorldState, MAX_MEDICAL_SERVICE_SLOTS, MEDICAL_RETRY_OVERFLOW,
};

pub const MEDICAL_NOTIFICATION_MESSAGE: &str = "Medical Center demanded near Lobby";

/// Office worker state codes used during a medical trip.
pub const STATE_MEDICAL_TRIP: i64 = 0x30;
pub const STATE_MEDICAL_TRIP_TRANSIT: i64 = 0x70;
pub const STATE_MEDICAL_DWELL: i64 = 0x31;

/// Upstream's candidate also holds `record`, the sidecar at `sidecarIndex`;
/// here it is reached through the index.
#[derive(Clone, Debug)]
struct MedicalCenterCandidate {
    sidecar_index: i64,
    floor: i64,
}

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

/// `world.sidecars[index]` when it is a medical center record; `None` where
/// JavaScript reads `undefined` (a negative or too-large index) or another kind.
fn medical_center(world: &WorldState, index: i64) -> Option<&MedicalCenterRecord> {
    if index < 0 {
        return None;
    }
    match world.sidecars.get(index as usize) {
        Some(SidecarRecord::MedicalCenter(m)) => Some(m),
        _ => None,
    }
}

fn medical_center_mut(world: &mut WorldState, index: i64) -> Option<&mut MedicalCenterRecord> {
    if index < 0 {
        return None;
    }
    match world.sidecars.get_mut(index as usize) {
        Some(SidecarRecord::MedicalCenter(m)) => Some(m),
        _ => None,
    }
}

/// Enumerate placed medical centers, bucketed by zone key `(floor - 9) / 15`.
///
/// Upstream returns `{ byZone, all }`; here the pair.
fn collect_medical_centers(
    world: &WorldState,
) -> (IndexMap<i64, Vec<MedicalCenterCandidate>>, Vec<MedicalCenterCandidate>) {
    let mut by_zone: IndexMap<i64, Vec<MedicalCenterCandidate>> = IndexMap::new();
    let mut all: Vec<MedicalCenterCandidate> = Vec::new();
    for (key, object) in world.placed_objects.iter() {
        if object.object_type_code != FAMILY_MEDICAL {
            continue;
        }
        if object.linked_record_index < 0 {
            continue;
        }
        let Some(sidecar) = medical_center(world, object.linked_record_index) else {
            continue;
        };
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        let (_, y) = key_xy(key);
        let floor = y_to_floor(y);
        let zone_key = js::floor_div(floor - 9, 15);
        let candidate = MedicalCenterCandidate {
            sidecar_index: object.linked_record_index,
            floor,
        };
        by_zone.entry(zone_key).or_default().push(candidate.clone());
        all.push(candidate);
    }
    (by_zone, all)
}

fn pick_medical_center(world: &mut WorldState, source_floor: i64) -> Option<MedicalCenterCandidate> {
    let (by_zone, all) = collect_medical_centers(world);
    if all.is_empty() {
        return None;
    }
    let zone_key = js::floor_div(source_floor - 9, 15);
    let zone_bucket = by_zone.get(&zone_key);
    let bucket = match zone_bucket {
        Some(b) if !b.is_empty() => b,
        _ => &all,
    };
    let pick = sample_rng(world) % bucket.len() as i64;
    Some(bucket[pick as usize].clone())
}

fn alloc_medical_slot(world: &mut WorldState, sim_index: i64, target_sidecar_index: i64, source_floor: i64) -> i64 {
    let sim_id = sim_key(&world.sims[sim_index as usize]);
    for i in 0..MAX_MEDICAL_SERVICE_SLOTS {
        let slot = &mut world.medical_service_slots[i as usize];
        if slot.active {
            continue;
        }
        slot.active = true;
        slot.sim_id = sim_id;
        slot.source_floor = source_floor;
        slot.target_sidecar_index = target_sidecar_index;
        slot.retry_counter = 0;
        return i;
    }
    -1
}

fn find_medical_slot_for_sim(world: &WorldState, sim_index: i64) -> i64 {
    let key = sim_key(&world.sims[sim_index as usize]);
    for (i, slot) in world.medical_service_slots.iter().enumerate() {
        if slot.active && slot.sim_id == key {
            return i as i64;
        }
    }
    -1
}

fn free_medical_slot(world: &mut WorldState, slot_index: i64) {
    // `slot?.active`: a negative or too-large index reads undefined.
    let Some(slot) = (if slot_index < 0 {
        None
    } else {
        world.medical_service_slots.get(slot_index as usize)
    }) else {
        return;
    };
    if !slot.active {
        return;
    }
    let target = slot.target_sidecar_index;
    if let Some(sidecar) = medical_center_mut(world, target) {
        sidecar.pending_visitors_count = 0.max(sidecar.pending_visitors_count - 1);
    }
    let slot = &mut world.medical_service_slots[slot_index as usize];
    slot.active = false;
    slot.sim_id = String::new();
    slot.source_floor = -1;
    slot.target_sidecar_index = -1;
    slot.retry_counter = 0;
}

fn fire_medical_failure_banner(world: &mut WorldState) {
    world.gate_flags.office_service_ok_medical = 0;
    world.pending_notifications.push(SimNotification {
        kind: SimNotificationKind::RouteFailure,
        message: Some(MEDICAL_NOTIFICATION_MESSAGE.to_string()),
    });
}

fn route_office_worker_home(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    let selected_floor = world.sims[si].selected_floor;
    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        selected_floor,
        LOBBY_FLOOR,
        if LOBBY_FLOOR > selected_floor { 1 } else { 0 },
        Some(time),
        None,
    );
    if result == -1 {
        let sim = &mut world.sims[si];
        sim.state_code = STATE_NIGHT_B;
        sim.destination_floor = -1;
        clear_sim_route(sim);
        release_service_request(world, sim_index);
        return;
    }
    if result == 3 {
        let sim = &mut world.sims[si];
        sim.destination_floor = -1;
        sim.selected_floor = LOBBY_FLOOR;
        sim.state_code = STATE_PARKED;
        release_service_request(world, sim_index);
        return;
    }
    let sim = &mut world.sims[si];
    sim.destination_floor = LOBBY_FLOOR;
    sim.state_code = STATE_DEPARTURE_TRANSIT;
}

/// Attempt a medical trip for `sim` (office worker at end of workday).
/// Returns true iff the sim has been routed onto a medical trip; the caller
/// should fall through to its normal departure path when false. The 1-in-10
/// RNG sample is only consumed at `starCount >= 3`, matching the binary's
/// gated check — pre-3 towers never enter this branch.
pub fn try_start_medical_trip(world: &mut WorldState, time: &TimeState, sim_index: i64) -> bool {
    let si = sim_index as usize;
    if world.star_count < 3 {
        return false;
    }
    let roll = sample_rng(world) % 10;
    if roll != 0 {
        return false;
    }

    let floor_anchor = world.sims[si].floor_anchor;
    let Some(target) = pick_medical_center(world, floor_anchor) else {
        fire_medical_failure_banner(world);
        return false;
    };

    let slot_index = alloc_medical_slot(world, sim_index, target.sidecar_index, floor_anchor);
    if slot_index < 0 {
        fire_medical_failure_banner(world);
        return false;
    }

    medical_center_mut(world, target.sidecar_index)
        .expect("tryStartMedicalTrip: target is a medical center")
        .pending_visitors_count += 1;

    if target.floor == floor_anchor {
        let sim = &mut world.sims[si];
        sim.state_code = STATE_MEDICAL_DWELL;
        sim.selected_floor = target.floor;
        sim.destination_floor = -1;
        clear_sim_route(sim);
        return true;
    }

    let result = resolve_sim_route_between_floors(
        world,
        sim_index,
        floor_anchor,
        target.floor,
        if target.floor > floor_anchor { 1 } else { 0 },
        Some(time),
        None,
    );
    if result == -1 {
        free_medical_slot(world, slot_index);
        fire_medical_failure_banner(world);
        return false;
    }
    let sim = &mut world.sims[si];
    sim.selected_floor = sim.floor_anchor;
    sim.destination_floor = target.floor;
    if result == 3 {
        sim.selected_floor = target.floor;
        sim.destination_floor = -1;
        sim.state_code = STATE_MEDICAL_DWELL;
        clear_sim_route(sim);
        return true;
    }
    sim.state_code = STATE_MEDICAL_TRIP_TRANSIT;
    true
}

/// Per-tick state-machine advance for an office worker on a medical trip.
/// Handles dwell timeout, demolished-target detection, and home routing.
pub fn process_medical_sim(world: &mut WorldState, time: &TimeState, sim_index: i64) {
    let si = sim_index as usize;
    if world.sims[si].state_code == STATE_MEDICAL_TRIP_TRANSIT {
        return;
    }

    if world.sims[si].state_code != STATE_MEDICAL_DWELL {
        return;
    }

    let slot_index = find_medical_slot_for_sim(world, sim_index);
    if slot_index < 0 {
        world.sims[si].state_code = STATE_DEPARTURE;
        return;
    }
    let target = world.medical_service_slots[slot_index as usize].target_sidecar_index;

    let target_gone = match medical_center(world, target) {
        None => true,
        Some(sidecar) => sidecar.owner_subtype_index == 0xff,
    };
    if target_gone {
        // Binary 1178:02e1 (site 6 of 6 add_or_update_sim_trip_counters callers):
        // office_sim_check_medical_service_slot fires
        // add_delay_to_current_sim(g_venue_unavailable_delay) followed by
        // advance_sim_trip_counters in the target-gone (slot.target == -1) branch.
        let sim = &mut world.sims[si];
        add_delay_to_current_sim(sim, 300);
        advance_sim_trip_counters(sim);
        free_medical_slot(world, slot_index);
        fire_medical_failure_banner(world);
        route_office_worker_home(world, time, sim_index);
        return;
    }

    let slot = &mut world.medical_service_slots[slot_index as usize];
    slot.retry_counter += 1;
    if slot.retry_counter >= MEDICAL_RETRY_OVERFLOW {
        free_medical_slot(world, slot_index);
        route_office_worker_home(world, time, sim_index);
    }
}

/// Arrival handler for office workers en route to a medical center.
pub fn handle_medical_sim_arrival(world: &mut WorldState, sim_index: i64, arrival_floor: i64) {
    let si = sim_index as usize;
    if world.sims[si].state_code != STATE_MEDICAL_TRIP_TRANSIT {
        return;
    }
    let slot_index = find_medical_slot_for_sim(world, sim_index);
    if slot_index < 0 {
        let sim = &mut world.sims[si];
        sim.state_code = STATE_DEPARTURE;
        sim.destination_floor = -1;
        return;
    }
    let target = world.medical_service_slots[slot_index as usize].target_sidecar_index;
    let gone = match medical_center(world, target) {
        None => true,
        Some(sidecar) => sidecar.owner_subtype_index == 0xff,
    };
    if gone {
        free_medical_slot(world, slot_index);
        fire_medical_failure_banner(world);
        let sim = &mut world.sims[si];
        sim.state_code = STATE_DEPARTURE;
        sim.selected_floor = arrival_floor;
        sim.destination_floor = -1;
        clear_sim_route(sim);
        return;
    }
    let sim = &mut world.sims[si];
    sim.selected_floor = arrival_floor;
    sim.destination_floor = -1;
    sim.state_code = STATE_MEDICAL_DWELL;
    clear_sim_route(sim);
}

/// Invalidate any in-flight medical slots whose target is `removedSidecarIndex`.
/// Called from the placement command layer after freeSidecar marks the target
/// as demolished. Each invalidated slot fires the failure banner and clears the
/// daily flag. Attached sims are put back into STATE_DEPARTURE so they route
/// home on the next refresh stride.
pub fn invalidate_medical_slots_for_sidecar(world: &mut WorldState, removed_sidecar_index: i64) {
    for i in 0..world.medical_service_slots.len() {
        let slot = &world.medical_service_slots[i];
        if !slot.active {
            continue;
        }
        if slot.target_sidecar_index != removed_sidecar_index {
            continue;
        }
        let sim = world.sims.iter().position(|s| sim_key(s) == slot.sim_id);
        free_medical_slot(world, i as i64);
        fire_medical_failure_banner(world);
        let Some(sim) = sim else {
            continue;
        };
        let sim = &mut world.sims[sim];
        sim.state_code = STATE_DEPARTURE;
        sim.destination_floor = -1;
        clear_sim_route(sim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{floor_to_y, PlacedObjectRecord};

    fn world_with_centres(floors: &[i64]) -> WorldState {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        w.placed_objects.clear();
        w.sidecars.clear();
        for (i, &floor) in floors.iter().enumerate() {
            w.sidecars.push(SidecarRecord::MedicalCenter(MedicalCenterRecord {
                owner_subtype_index: 0,
                pending_visitors_count: 0,
            }));
            w.placed_objects.insert(
                format!("{},{}", 10 + 30 * i as i64, floor_to_y(floor)),
                PlacedObjectRecord {
                    left_tile_index: 10,
                    right_tile_index: 35,
                    object_type_code: FAMILY_MEDICAL,
                    unit_status: 0,
                    aux_value_or_timer: 0,
                    linked_record_index: i as i64,
                    dirty_flag: 0,
                    occupied_flag: 0,
                    eval_level: 0,
                    eval_score: -1,
                    rent_level: 4,
                    activation_tick_count: 0,
                    housekeeping_claimed_flag: 0,
                    vip_flag: None,
                },
            );
        }
        w
    }

    #[test]
    fn zones_are_floored_fifteen_floor_bands() {
        // Floors 5 and 8 are zone -1 ((5-9)/15 floored); 9 and 23 zone 0; 24 zone 1.
        let w = world_with_centres(&[5, 9, 24, 23, 8]);
        let (by_zone, all) = collect_medical_centers(&w);
        assert_eq!(all.len(), 5);
        let zones: Vec<(i64, Vec<i64>)> = by_zone
            .iter()
            .map(|(z, c)| (*z, c.iter().map(|c| c.floor).collect()))
            .collect();
        assert_eq!(zones, vec![(-1, vec![5, 8]), (0, vec![9, 23]), (1, vec![24])]);
    }

    #[test]
    fn pick_uses_the_zone_bucket_or_all() {
        let mut w = world_with_centres(&[30, 40]);
        // Zone 0 is empty: the pick is over all, consuming one sample.
        let before = w.rng_call_count;
        let c = pick_medical_center(&mut w, 12).unwrap();
        assert_eq!(w.rng_call_count, before + 1);
        assert!(c.floor == 30 || c.floor == 40);
        // Zone 2 (floors 39..53) holds only floor 40.
        assert_eq!(pick_medical_center(&mut w, 45).unwrap().floor, 40);
    }

    #[test]
    fn free_slot_decrements_pending_and_ignores_inactive() {
        let mut w = world_with_centres(&[30]);
        free_medical_slot(&mut w, 99);
        free_medical_slot(&mut w, -1);
        let slot = &mut w.medical_service_slots[2];
        slot.active = true;
        slot.target_sidecar_index = 0;
        slot.sim_id = "x".into();
        if let SidecarRecord::MedicalCenter(m) = &mut w.sidecars[0] {
            m.pending_visitors_count = 1;
        }
        free_medical_slot(&mut w, 2);
        free_medical_slot(&mut w, 2);
        let SidecarRecord::MedicalCenter(m) = &w.sidecars[0] else { panic!() };
        assert_eq!(m.pending_visitors_count, 0);
        assert!(!w.medical_service_slots[2].active);
        assert_eq!(w.medical_service_slots[2].target_sidecar_index, -1);
    }
}

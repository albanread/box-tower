// ts: sims/hotel-facilities.ts

use crate::resources::{FAMILY_CONDO, FAMILY_OFFICE};
use crate::time::TimeState;
use crate::world::{PlacedObjectRecord, WorldState};

use super::population::find_object_for_sim;
use super::scoring::{recompute_object_operational_status, refresh_occupied_flag_and_trip_counters};
use super::states::{HOTEL_FAMILIES, UNIT_STATUS_CONDO_VACANT, UNIT_STATUS_CONDO_VACANT_EVENING};

/// `key.split(",").map(Number)` for a placed-object key "x,y".
fn key_xy(key: &str) -> (i64, i64) {
    let mut parts = key.split(',').map(|p| {
        p.parse::<i64>()
            .unwrap_or_else(|_| panic!("placed-object key {key:?} is not \"x,y\""))
    });
    let x = parts.next().unwrap();
    let y = parts.next().unwrap_or_else(|| panic!("placed-object key {key:?} is not \"x,y\""));
    (x, y)
}

/// End-of-day object-state floor pass.
/// Called after resetSimRuntimeState to normalize unit status bands:
/// - Occupied hotels (< 0x18) clamped to sync sentinel 0x10
/// - Sold condos (< 0x18) clamped to 0x10
/// - Non-occupied hotel bands toggle: 0x18 <-> 0x20, 0x28 <-> 0x30, 0x38 <-> 0x40
/// - Unsold condo bands toggle: 0x18 <-> 0x20
pub fn normalize_unit_status_end_of_day(world: &mut WorldState) {
    for object in world.placed_objects.values_mut() {
        if HOTEL_FAMILIES.contains(&object.object_type_code) {
            if object.unit_status < 0x18 {
                // Occupied: clamp to sync sentinel
                object.unit_status = 0x10;
            } else if object.unit_status >= 0x18 && object.unit_status < 0x28 {
                // Vacant band toggle: 0x18 <-> 0x20
                object.unit_status = if object.unit_status < 0x20 { 0x20 } else { 0x18 };
            } else if object.unit_status >= 0x28 && object.unit_status < 0x38 {
                // Checkout/turnover band toggle: 0x28 <-> 0x30
                object.unit_status = if object.unit_status < 0x30 { 0x30 } else { 0x28 };
            } else if object.unit_status >= 0x38 && object.unit_status <= 0x40 {
                // Extended band toggle: 0x38 <-> 0x40
                object.unit_status = if object.unit_status < 0x40 { 0x40 } else { 0x38 };
            }
        } else if object.object_type_code == FAMILY_CONDO {
            if object.unit_status < 0x18 {
                // Sold: clamp to sync sentinel
                object.unit_status = 0x10;
            } else {
                // Unsold band toggle: 0x18 <-> 0x20
                object.unit_status = if object.unit_status < 0x20 {
                    UNIT_STATUS_CONDO_VACANT_EVENING
                } else {
                    UNIT_STATUS_CONDO_VACANT
                };
            }
        }
    }
}

/// Spread existing cockroach infestations to adjacent hotel rooms on the same floor.
/// Spec: runs at checkpoint 0x640 before the expiry check.
///
/// Upstream takes `Object.entries` once and walks it twice, reading each
/// object live: a room infected earlier in the outer walk spreads in turn
/// when the outer walk reaches it.  Here the key list is taken once and
/// every object is read through its key as it is reached; `neighbor ===
/// object` is a comparison of keys (unique in the record).
pub fn spread_cockroach_infestation(world: &mut WorldState, time: &TimeState) {
    let objects_by_key = world.placed_objects.key_list();
    for key in &objects_by_key {
        let object = &world.object(key.as_str()).unwrap();
        if !HOTEL_FAMILIES.contains(&object.object_type_code) {
            continue;
        }
        if object.unit_status < 0x38 {
            continue;
        }

        let (x, y) = key_xy(key);
        for n_key in &objects_by_key {
            if n_key == key {
                continue;
            }
            let neighbor = &world.object(n_key.as_str()).unwrap();
            if !HOTEL_FAMILIES.contains(&neighbor.object_type_code) {
                continue;
            }
            let (nx, ny) = key_xy(n_key);
            if ny != y {
                continue;
            }
            let object_left = world.object(key.as_str()).unwrap().left_tile_index;
            if nx < x && world.object(n_key.as_str()).unwrap().right_tile_index >= object_left - 1 {
                infect_hotel_room(world.object_mut(n_key).unwrap(), time);
            }
            let object_right = world.object(key.as_str()).unwrap().right_tile_index;
            let neighbor = &world.object(n_key.as_str()).unwrap();
            if nx > x && neighbor.left_tile_index <= object_right + 1 && neighbor.unit_status < 0x38 {
                infect_hotel_room(world.object_mut(n_key).unwrap(), time);
            }
        }
    }
}

fn infect_hotel_room(neighbor: &mut PlacedObjectRecord, time: &TimeState) {
    neighbor.unit_status = if time.daypart_index < 4 { 0x40 } else { 0x38 };
    neighbor.eval_level = 0xff;
    neighbor.housekeeping_claimed_flag = 0;
}

/// Three-strikes expiry: hotel rooms in checkout/turnover band without housekeeping
/// become infested after 3 consecutive checkpoint passes.
/// Spec: handle_extended_vacancy_expiry, runs at checkpoint 0x640.
pub fn handle_extended_vacancy_expiry(world: &mut WorldState, time: &TimeState) {
    for object in world.placed_objects.values_mut() {
        if !HOTEL_FAMILIES.contains(&object.object_type_code) {
            continue;
        }
        if object.unit_status <= 0x27 {
            continue;
        }
        if object.unit_status >= 0x38 {
            continue;
        }

        if object.housekeeping_claimed_flag != 0 {
            object.eval_level = 0xff;
            object.activation_tick_count = 0;
            object.housekeeping_claimed_flag = 0;
        } else {
            object.activation_tick_count += 1;
            if object.activation_tick_count >= 3 {
                object.unit_status = if time.daypart_index < 4 { 0x40 } else { 0x38 };
            }
        }
    }
}

/// Binary `advance_object_stay_phase_tiers` @ 1230:0b5f, invoked at checkpoint
/// 0x640 (dayTick 1600). Iterates all placed objects and raises unit_status
/// bands by +8 in specific residue classes. Critically, hotels at unit_status
/// 0x18 → 0x20 here triggers `refresh_occupied_flag_and_trip_counters` to
/// actually reset sim trip counters when the later midday pass runs.
pub fn advance_object_stay_phase_tiers(world: &mut WorldState) {
    for object in world.placed_objects.values_mut() {
        let status = object.unit_status;
        if HOTEL_FAMILIES.contains(&object.object_type_code) {
            if status == 0x18 {
                object.unit_status = 0x20;
            } else if status == 0x28 {
                object.unit_status = 0x30;
            } else if status == 0x38 {
                object.unit_status = 0x40;
            }
        } else if object.object_type_code == FAMILY_OFFICE {
            if status == 0x00 {
                object.unit_status = 0x08;
            } else if status == 0x10 {
                object.unit_status = 0x18;
            }
        } else if object.object_type_code == FAMILY_CONDO {
            if status == 0x18 {
                object.unit_status = 0x20;
            } else if (status & 0xf8) == 0 {
                object.unit_status = (status & 7) + 0x08;
            }
        }
    }
}

pub fn update_hotel_operational_and_occupancy(world: &mut WorldState, time: &TimeState) {
    // `for (const sim of world.sims)`: by index, the length read each time
    // round as JavaScript's array iterator does.
    let mut sim_index = 0;
    while sim_index < world.sims.len() {
        let index = sim_index;
        sim_index += 1;
        let Some(object) = find_object_for_sim(world, index as i64) else {
            continue;
        };
        if !HOTEL_FAMILIES.contains(&world.object(object.as_str()).unwrap().object_type_code) {
            continue;
        }
        if world.sims[index].base_offset != 0 {
            continue;
        }
        recompute_object_operational_status(world, index as i64, &object);
    }
    handle_extended_vacancy_expiry(world, time);
    let mut sim_index = 0;
    while sim_index < world.sims.len() {
        let index = sim_index;
        sim_index += 1;
        let Some(object) = find_object_for_sim(world, index as i64) else {
            continue;
        };
        if !HOTEL_FAMILIES.contains(&world.object(object.as_str()).unwrap().object_type_code) {
            continue;
        }
        if world.sims[index].base_offset != 0 {
            continue;
        }
        // Binary update_hotel_operational_and_pairings pass 2: rooms with
        // unitStatus >= 0x28 (dormant / post-checkout) keep their current
        // occupiedFlag (+0x14). Fresh rooms run through the donor/flag update
        // path unconditionally — the refresh function handles the evalLevel
        // branches.
        if world.object(object.as_str()).unwrap().unit_status >= 0x28 {
            continue;
        }
        refresh_occupied_flag_and_trip_counters(world, index as i64, &object);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::FAMILY_HOTEL_SINGLE;
    use crate::world::PlacedObjectRecord;

    fn room(left: i64, width: i64, code: i64, status: i64) -> PlacedObjectRecord {
        PlacedObjectRecord {
            left_tile_index: left,
            right_tile_index: left + width - 1,
            object_type_code: code,
            unit_status: status,
            aux_value_or_timer: 0,
            linked_record_index: -1,
            dirty_flag: 0,
            occupied_flag: 0,
            eval_level: -1,
            eval_score: -1,
            rent_level: 1,
            activation_tick_count: 0,
            housekeeping_claimed_flag: 1,
            vip_flag: None,
        }
    }

    fn world_with(rooms: &[(&str, PlacedObjectRecord)]) -> WorldState {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        for (k, r) in rooms {
            w.placed_objects.insert(*k, r.clone());
        }
        w
    }

    #[test]
    fn infestation_chains_within_one_pass() {
        // Three adjacent rooms, the leftmost infested: the middle one is
        // infected while the outer walk is on the first, and then spreads to
        // the third (and back to the first, leftward) when the outer walk
        // reaches it, as upstream's live reads do.
        let mut w = world_with(&[
            ("10,50", room(10, 4, FAMILY_HOTEL_SINGLE, 0x38)),
            ("14,50", room(14, 4, FAMILY_HOTEL_SINGLE, 0x10)),
            ("18,50", room(18, 4, FAMILY_HOTEL_SINGLE, 0x10)),
            ("30,50", room(30, 4, FAMILY_HOTEL_SINGLE, 0x10)), // not adjacent
            ("14,51", room(14, 4, FAMILY_HOTEL_SINGLE, 0x10)), // other floor
        ]);
        let mut t = crate::time::create_time_state();
        t.daypart_index = 2;
        spread_cockroach_infestation(&mut w, &t);
        let s = |k: &str| w.placed_objects[k].unit_status;
        assert_eq!((s("10,50"), s("14,50"), s("18,50"), s("30,50"), s("14,51")), (0x40, 0x40, 0x40, 0x10, 0x10));
        assert_eq!(w.placed_objects["14,50"].eval_level, 0xff);
        assert_eq!(w.placed_objects["14,50"].housekeeping_claimed_flag, 0);
    }

    #[test]
    fn infestation_leftward_ignores_neighbour_status() {
        // Leftward spread has no `< 0x38` check: an already-infested left
        // neighbour is infected again (its status rewritten by daypart).
        let mut w = world_with(&[
            ("10,50", room(10, 4, FAMILY_HOTEL_SINGLE, 0x40)),
            ("14,50", room(14, 4, FAMILY_HOTEL_SINGLE, 0x40)),
        ]);
        let mut t = crate::time::create_time_state();
        t.daypart_index = 5;
        spread_cockroach_infestation(&mut w, &t);
        // 14 infects 10 leftward (0x38); 10 does not reinfect 14 rightward
        // (14 is already >= 0x38).
        assert_eq!(w.placed_objects["10,50"].unit_status, 0x38);
        assert_eq!(w.placed_objects["14,50"].unit_status, 0x40);
    }

    #[test]
    fn end_of_day_bands() {
        let mut w = world_with(&[
            ("1,1", room(1, 1, FAMILY_HOTEL_SINGLE, 0x05)),
            ("2,1", room(2, 1, FAMILY_HOTEL_SINGLE, 0x18)),
            ("3,1", room(3, 1, FAMILY_HOTEL_SINGLE, 0x30)),
            ("4,1", room(4, 1, FAMILY_HOTEL_SINGLE, 0x40)),
            ("5,1", room(5, 1, FAMILY_HOTEL_SINGLE, 0x41)),
            ("6,1", room(6, 1, FAMILY_CONDO, 0x18)),
            ("7,1", room(7, 1, FAMILY_CONDO, 0x20)),
            ("8,1", room(8, 1, FAMILY_CONDO, 0x03)),
        ]);
        normalize_unit_status_end_of_day(&mut w);
        let got: Vec<i64> = w.placed_objects.values().map(|o| o.unit_status).collect();
        assert_eq!(got, [0x10, 0x20, 0x28, 0x38, 0x41, 0x20, 0x18, 0x10]);
        advance_object_stay_phase_tiers(&mut w);
        let got: Vec<i64> = w.placed_objects.values().map(|o| o.unit_status).collect();
        assert_eq!(got, [0x10, 0x20, 0x30, 0x40, 0x41, 0x20, 0x20, 0x10]);
    }
}

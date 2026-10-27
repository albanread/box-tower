// ts: sims/parking.ts

use indexmap::IndexMap;

use crate::js::JsObject;
use crate::resources::{FAMILY_PARKING, FAMILY_PARKING_RAMP};
use crate::stress::rebase_elapsed::rebase_sim_elapsed_from_clock;
use crate::time::TimeState;
use crate::world::{sample_rng, SidecarRecord, WorldState};

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

#[allow(dead_code)]
struct ParkingSpace {
    x: i64,
    y: i64,
    sidecar_index: i64,
}

struct RampAnchor {
    x: i64,
    y: i64,
    right: i64,
}

/// `world.sidecars[index]`'s coverageFlag, written if it is a service
/// request record (`rec?.kind === "service_request"`).
fn set_coverage(sidecars: &mut [SidecarRecord], index: i64, flag: i64) {
    if index < 0 {
        return;
    }
    if let Some(SidecarRecord::ServiceRequest(rec)) = sidecars.get_mut(index as usize) {
        rec.coverage_flag = Some(flag);
    }
}

/// JavaScript truthiness of `o[k]` for a string-valued object.
fn truthy(o: &JsObject<String>, k: &str) -> bool {
    o.get(k).is_some_and(|v| !v.is_empty())
}

/// Rebuild parking-space coverage flags from parking-ramp anchors.
///
/// Per spec/PARKING.md "Coverage Propagation": for each floor, walk left/right
/// from each ramp anchor across same-floor tiles, crossing empty runs of at
/// most 3 tiles. Stop at any wider gap or any non-empty/non-parking object.
/// Reachable parking-space tiles are marked covered (state 1); all others
/// default to uncovered (state 0).
pub fn rebuild_parking_coverage(world: &mut WorldState) {
    let mut parking_by_key: IndexMap<String, ParkingSpace> = IndexMap::new();
    let mut ramp_anchors: Vec<RampAnchor> = Vec::new();

    for (key, obj) in world.placed_objects.iter() {
        let (x, y) = key_xy(key);
        if obj.object_type_code == FAMILY_PARKING {
            parking_by_key.insert(
                key.to_string(),
                ParkingSpace {
                    x,
                    y,
                    sidecar_index: obj.linked_record_index,
                },
            );
        } else if obj.object_type_code == FAMILY_PARKING_RAMP {
            ramp_anchors.push(RampAnchor {
                x,
                y,
                right: obj.right_tile_index,
            });
        }
    }

    // BOX: car elevator -- each car elevator landing on a basement floor is
    // an anchor as a ramp is (none while no car elevator is built).
    for (x, y, right) in crate::box_ext::car_elevator::coverage_anchors(world) {
        ramp_anchors.push(RampAnchor { x, y, right });
    }

    for space in parking_by_key.values() {
        set_coverage(&mut world.sidecars, space.sidecar_index, 0);
    }

    // BOX: car elevator -- under BOX RULES bare basement floor is a
    // driveway: cars cross it to the spaces (classic coverage stops at it).
    let driveways = crate::box_ext::box_rules(world);
    let WorldState {
        cells,
        cell_to_anchor,
        sidecars,
        width,
        ..
    } = world;

    // `world.cellToAnchor[k] ?? k`.
    let anchor_key = |k: String| -> String { cell_to_anchor.get(&k).cloned().unwrap_or(k) };
    let is_parking_tile = |x: i64, y: i64| -> bool {
        let k = format!("{x},{y}");
        parking_by_key.contains_key(&anchor_key(k))
    };
    let is_empty = |x: i64, y: i64| -> bool {
        let k = format!("{x},{y}");
        !truthy(cells, &k) && !truthy(cell_to_anchor, &k)
    };
    let cover = |sidecars: &mut Vec<SidecarRecord>, x: i64, y: i64| {
        let anchor = anchor_key(format!("{x},{y}"));
        let Some(space) = parking_by_key.get(&anchor) else {
            return;
        };
        set_coverage(sidecars, space.sidecar_index, 1);
    };

    const MAX_EMPTY_GAP: i64 = 3;

    // `direction` is upstream's `-1 | 1`.
    let walk = |sidecars: &mut Vec<SidecarRecord>, start_x: i64, y: i64, direction: i64, bound: i64| {
        let in_bounds = |x: i64| if direction == 1 { x <= bound } else { x >= bound };
        let mut x = start_x;
        while in_bounds(x) {
            if is_parking_tile(x, y) {
                cover(sidecars, x, y);
                x += direction;
                continue;
            }
            if driveways && cells.get(&format!("{x},{y}")).is_some_and(|t| t == "floor") {
                x += direction;
                continue;
            }
            if is_empty(x, y) {
                let mut run = 0;
                while in_bounds(x) && is_empty(x, y) && run <= MAX_EMPTY_GAP {
                    x += direction;
                    run += 1;
                }
                if run > MAX_EMPTY_GAP {
                    return;
                }
                continue;
            }
            return;
        }
    };

    for ramp in &ramp_anchors {
        // Cover the ramp footprint's parking neighbors immediately to the sides.
        walk(sidecars, ramp.x - 1, ramp.y, -1, 0);
        walk(sidecars, ramp.right + 1, ramp.y, 1, *width - 1);
    }
}

pub fn rebuild_parking_demand_log(world: &mut WorldState) {
    world.parking_demand_log = Vec::new();
    for i in 0..world.sidecars.len() {
        let SidecarRecord::ServiceRequest(rec) = &world.sidecars[i] else {
            continue;
        };
        if rec.owner_subtype_index == 0xff {
            continue;
        }
        if rec.floor_index.is_none() {
            continue;
        }
        if rec.coverage_flag == Some(1) {
            continue;
        }

        let mut is_parking = false;
        for obj in world.placed_objects.values() {
            if obj.object_type_code == FAMILY_PARKING && obj.linked_record_index == i as i64 {
                is_parking = true;
                break;
            }
        }
        if is_parking {
            world.parking_demand_log.push(i as i64);
        }
    }
}

pub fn try_assign_parking_service(world: &mut WorldState, time: &TimeState, sim_index: i64) -> bool {
    // BOX: BOX RULES -- parking that makes sense: a reachable free bay
    // serves the driver (box_ext/car_elevator.rs).  Off: upstream's body.
    if crate::box_ext::box_rules(world) {
        return crate::box_ext::car_elevator::assign_parking_box_rules(world, time, sim_index);
    }
    if world.parking_demand_log.is_empty() {
        return false;
    }
    let pick = sample_rng(world) % world.parking_demand_log.len() as i64;
    let idx = world.parking_demand_log[pick as usize];
    let is_service_request = idx >= 0
        && matches!(world.sidecars.get(idx as usize), Some(SidecarRecord::ServiceRequest(_)));
    if !is_service_request {
        return false;
    }
    // Binary parity: process_family_parking_destination_arrival (1048:00f0)
    // is NOT one of the 6 advance_sim_trip_counters call sites. The previous
    // advanceSimTripCounters call here did not correspond to any binary site
    // and has been removed.
    rebase_sim_elapsed_from_clock(&mut world.sims[sim_index as usize], time);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{PlacedObjectRecord, ServiceRequestEntry};

    fn object(x: i64, width: i64, code: i64, linked: i64) -> PlacedObjectRecord {
        PlacedObjectRecord {
            left_tile_index: x,
            right_tile_index: x + width - 1,
            object_type_code: code,
            unit_status: 0,
            aux_value_or_timer: 0,
            linked_record_index: linked,
            dirty_flag: 0,
            occupied_flag: 0,
            eval_level: 0,
            eval_score: -1,
            rent_level: 4,
            activation_tick_count: 0,
            housekeeping_claimed_flag: 0,
            vip_flag: None,
        }
    }

    fn place(w: &mut WorldState, x: i64, y: i64, width: i64, code: i64, linked: i64, tile: &str) {
        w.cells.insert(format!("{x},{y}"), tile.to_string());
        for dx in 1..width {
            w.cells.insert(format!("{},{y}", x + dx), tile.to_string());
            w.cell_to_anchor.insert(format!("{},{y}", x + dx), format!("{x},{y}"));
        }
        w.placed_objects.insert(format!("{x},{y}"), object(x, width, code, linked));
    }

    fn coverage(w: &WorldState, i: usize) -> Option<i64> {
        let SidecarRecord::ServiceRequest(r) = &w.sidecars[i] else { panic!() };
        r.coverage_flag
    }

    #[test]
    fn coverage_crosses_gaps_of_three_and_demand_logs_the_rest() {
        let mut w = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        w.placed_objects.clear();
        w.cells.clear();
        w.cell_to_anchor.clear();
        w.sidecars.clear();
        for _ in 0..3 {
            w.sidecars.push(SidecarRecord::ServiceRequest(ServiceRequestEntry {
                owner_subtype_index: 0,
                floor_index: Some(5),
                coverage_flag: None,
            }));
        }
        let y = 114;
        place(&mut w, 100, y, 4, FAMILY_PARKING_RAMP, -1, "parkingRamp");
        // Right of the ramp (103): a space at 104..107, a gap of 3 (108..110),
        // a space at 111..114, a gap of 4 (115..118), a space at 119.
        place(&mut w, 104, y, 4, FAMILY_PARKING, 0, "parking");
        place(&mut w, 111, y, 4, FAMILY_PARKING, 1, "parking");
        place(&mut w, 119, y, 4, FAMILY_PARKING, 2, "parking");
        rebuild_parking_coverage(&mut w);
        assert_eq!(coverage(&w, 0), Some(1));
        assert_eq!(coverage(&w, 1), Some(1));
        assert_eq!(coverage(&w, 2), Some(0));
        rebuild_parking_demand_log(&mut w);
        assert_eq!(w.parking_demand_log, vec![2]);
    }
}

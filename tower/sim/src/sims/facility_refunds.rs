// ts: sims/facility-refunds.ts

use crate::families::entertainment::{
    recompute_commercial_venue_buckets, recompute_restaurant_venue_buckets,
};
use crate::js;
use crate::ledger::{add_cashflow_from_family_resource, remove_cashflow_from_family_resource, LedgerState};
use crate::progression::{add_to_population_bucket, clear_population_bucket};
use crate::resources::{
    COMMERCIAL_CAPACITY_CAPS, COMMERCIAL_CLOSURE_BANDS, COMMERCIAL_CLOSURE_PAYOUTS,
    FAMILY_CODE_TO_TILE, FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_RESTAURANT, FAMILY_RETAIL,
    OP_SCORE_THRESHOLDS,
};
use crate::sims::population::clear_sim_route;
use crate::sims::states::{STATE_PARKED, UNIT_STATUS_CONDO_VACANT, UNIT_STATUS_CONDO_VACANT_EVENING};
use crate::time::TimeState;
use crate::world::{
    y_to_floor, CommercialVenueRecord, SidecarRecord, WorldState, VENUE_AVAILABLE, VENUE_CLOSED,
    VENUE_DORMANT, VENUE_PARTIAL,
};

/// `world.sidecars[index]` when it is a commercial venue record (upstream's
/// `record?.kind === "commercial_venue"` after the lookup); `None` where
/// JavaScript reads `undefined` or another kind.
fn venue_mut(sidecars: &mut [SidecarRecord], index: i64) -> Option<&mut CommercialVenueRecord> {
    if index < 0 {
        return None;
    }
    match sidecars.get_mut(index as usize) {
        Some(SidecarRecord::CommercialVenue(v)) => Some(v),
        _ => None,
    }
}

/// The y of an "x,y" key.
fn key_xy(key: &str) -> (i64, i64) {
    let mut parts = key.split(',').map(|p| {
        p.parse::<i64>()
            .unwrap_or_else(|_| panic!("key {key:?} is not \"x,y\""))
    });
    let x = parts.next().unwrap();
    let y = parts.next().unwrap_or_else(|| panic!("key {key:?} is not \"x,y\""));
    (x, y)
}

/// Mirrors binary `select_facility_progress_slot` (11b0:1870). Selects
/// which seed slot to use: override (5) when facilityProgressOverride is
/// active, phaseB (4) when calendarPhaseFlag is set (weekendFlag, i.e.
/// dayCounter % 3 === 2), otherwise phaseA (3).
///
/// (Upstream's return type is `3 | 4 | 5`.)
pub fn select_facility_progress_slot(world: &WorldState, time: &TimeState) -> i64 {
    if world.gate_flags.facility_progress_override != 0 {
        return 5;
    }
    if time.weekend_flag != 0 {
        return 4;
    }
    3
}

fn read_seed_for_slot(record: &CommercialVenueRecord, slot: i64) -> i64 {
    if slot == 5 {
        return record.override_seed;
    }
    if slot == 4 {
        return record.phase_b_seed;
    }
    record.phase_a_seed
}

fn write_seed_for_slot(record: &mut CommercialVenueRecord, slot: i64, value: i64) {
    if slot == 5 {
        record.override_seed = value;
    } else if slot == 4 {
        record.phase_b_seed = value;
    } else {
        record.phase_a_seed = value;
    }
}

/// The (objectTypeCode, linkedRecordIndex) of every placed object, in
/// `Object.values` order.  The loops below change sidecars and population
/// buckets but never `placedObjects`, so walking this list is walking
/// upstream's.
fn object_links(world: &WorldState) -> Vec<(i64, i64)> {
    world
        .placed_objects
        .values()
        .map(|obj| (obj.object_type_code, obj.linked_record_index))
        .collect()
}

/// Mirrors binary `rebuild_linked_facility_records` (11b0:0184) →
/// `recompute_facility_runtime_state` (11b0:02f2), invoked at checkpoint
/// 0x0f0 (dayTick 240 / daypart 0). For each valid fast-food or retail
/// venue, computes the daily capacity from the active phase seed, caps it
/// at the type-specific tuning limit, floors at 10, and writes the
/// eligibility threshold. Restaurants are excluded — they use a separate
/// per-cycle mechanism at tick 1600.
pub fn rebuild_commercial_venue_runtime(world: &mut WorldState, time: &TimeState) {
    let slot = select_facility_progress_slot(world, time);
    // Binary `rebuild_linked_facility_records` (11b0:0184) opens by clearing
    // the per-family ledger buckets for non-restaurant commercial families
    // before iterating records and re-adding each record's yesterday-visit-
    // count. The clear-then-add yields a NET delta of (new - old) on the
    // running total, matching the binary's daily population accounting.
    clear_population_bucket(world, FAMILY_FAST_FOOD);
    clear_population_bucket(world, FAMILY_RETAIL);
    for (code, linked) in object_links(world) {
        if code != FAMILY_FAST_FOOD && code != FAMILY_RETAIL {
            continue;
        }
        if linked < 0 {
            continue;
        }
        let Some(record) = venue_mut(&mut world.sidecars, linked) else {
            continue;
        };
        if record.availability_state != VENUE_DORMANT {
            record.availability_state = VENUE_AVAILABLE;
        }

        // Binary recompute_facility_runtime_state: capacity always comes from
        // phaseASeed (offset +3), regardless of the active calendar slot.
        // The active slot determines which seed is CLEARED (and which is
        // incremented by visits via clamp_object_type_limit).
        let caps = js::record_get(COMMERCIAL_CAPACITY_CAPS, code);
        let mut cap = record.phase_a_seed;
        if let Some(caps) = caps {
            if cap > caps[0] {
                cap = caps[0];
            }
        }
        if cap < 10 {
            cap = 10;
        }

        record.remaining_capacity = cap;
        record.eligibility_threshold = -(cap + 1);

        // Roll visit counters (binary: record[8] = record[7], then clear).
        // Binary recompute_facility_runtime_state @ 11b0:0461 calls
        // add_to_primary_family_ledger_bucket(family_code, record[+8]) AFTER
        // the roll, so yesterday's daily visit count contributes to the
        // running primary-family-ledger total used by star advancement.
        record.yesterday_visit_count = record.today_visit_count;
        let yesterday = record.yesterday_visit_count;
        add_to_population_bucket(world, code, yesterday);
        let record = venue_mut(&mut world.sidecars, linked).unwrap();
        record.today_visit_count = 0;
        record.acquire_count = 0;
        record.current_population = 0;
        record.visit_count = 0;

        // Clear the active calendar slot (not necessarily phaseA).
        write_seed_for_slot(record, slot, 0);
    }
    recompute_commercial_venue_buckets(world);
}

/// Mirrors binary `rebuild_type6_facility_records` (11b0:0250) — type 6 is
/// the restaurant family. Invoked at checkpoint 0x640 (dayTick 1600 / midday):
/// restaurants use a per-cycle seeding that fires at midday instead of daypart
/// 0. For each restaurant venue, reopens it (availabilityState → AVAILABLE if
/// not DORMANT), refills remainingCapacity (floor 10), resets eligibility
/// threshold to -(cap+1), and rolls visit counters.
pub fn rebuild_restaurant_facility_records(world: &mut WorldState, time: &TimeState) {
    let slot = select_facility_progress_slot(world, time);
    // Binary `rebuild_type6_facility_records` (11b0:0250) clears the
    // per-family bucket then iterates restaurants through
    // `recompute_facility_runtime_state`, which calls
    // `add_to_primary_family_ledger_bucket(6, record[+8])` after rolling the
    // day counter. Mirrors the fast-food/retail clear+add pattern.
    clear_population_bucket(world, FAMILY_RESTAURANT);
    for (code, linked) in object_links(world) {
        if code != FAMILY_RESTAURANT {
            continue;
        }
        if linked < 0 {
            continue;
        }
        let Some(record) = venue_mut(&mut world.sidecars, linked) else {
            continue;
        };
        if record.availability_state != VENUE_DORMANT {
            record.availability_state = VENUE_AVAILABLE;
        }

        // Capacity always from phaseASeed; active slot is cleared.
        let caps = js::record_get(COMMERCIAL_CAPACITY_CAPS, FAMILY_RESTAURANT);
        let mut cap = record.phase_a_seed;
        if let Some(caps) = caps {
            if cap > caps[0] {
                cap = caps[0];
            }
        }
        if cap < 10 {
            cap = 10;
        }

        record.remaining_capacity = cap;
        record.eligibility_threshold = -(cap + 1);
        record.yesterday_visit_count = record.today_visit_count;
        let yesterday = record.yesterday_visit_count;
        add_to_population_bucket(world, FAMILY_RESTAURANT, yesterday);
        let record = venue_mut(&mut world.sidecars, linked).unwrap();
        record.today_visit_count = 0;
        record.acquire_count = 0;
        record.current_population = 0;
        record.visit_count = 0;
        write_seed_for_slot(record, slot, 0);
    }
    recompute_restaurant_venue_buckets(world);
}

pub fn close_commercial_venues(world: &mut WorldState) {
    for record in world.sidecars.iter_mut() {
        let SidecarRecord::CommercialVenue(record) = record else {
            continue;
        };
        record.availability_state = VENUE_CLOSED;
    }
}

/// Close commercial venues of a single family and accrue closure income.
/// Mirrors `seed_facility_runtime_link_state` for non-type-6 (tick 2000)
/// and type-6 (tick 2200) sweeps. Restaurant/fast-food payouts come from
/// `derive_commercial_venue_state_code`; retail returns 0.
pub fn close_commercial_venues_by_family(world: &mut WorldState, ledger: &mut LedgerState, family_code: i64) {
    let tile_name = js::record_get(FAMILY_CODE_TO_TILE, family_code).copied();
    // `tileName ? ... : undefined`: every tile name is a non-empty string.
    let payouts = match tile_name {
        Some(name) if !name.is_empty() => js::record_get(COMMERCIAL_CLOSURE_PAYOUTS, name).copied(),
        _ => None,
    };

    for (code, linked) in object_links(world) {
        if code != family_code {
            continue;
        }
        if linked < 0 {
            continue;
        }
        let Some(record) = venue_mut(&mut world.sidecars, linked) else {
            continue;
        };
        if record.availability_state == VENUE_DORMANT {
            continue;
        }

        if let Some(payouts) = payouts {
            // Binary `derive_commercial_venue_state_code` (11b0:1731) is called
            // from `seed_facility_runtime_link_state` with `record+0x10`
            // (acquireCount, the visitor-acquisition word incremented by both
            // try_consume_commercial_venue_capacity at MORNING_GATE AND
            // acquire_commercial_venue_slot on visitor arrivals). Read
            // `acquireCount` here (NOT `todayVisitCount`, which is the
            // staff-emit byte at +0x7 that feeds the population/star ledger
            // instead) so closure cash bands match the binary's input.
            let visits = record.acquire_count;
            let mut band = 0;
            for threshold in COMMERCIAL_CLOSURE_BANDS {
                if visits >= threshold {
                    band += 1;
                }
            }
            let yen_k = js::at_or(&payouts, band, 0);
            if yen_k != 0 {
                apply_closure_cash(ledger, family_code, yen_k);
            }
        }

        record.availability_state = VENUE_CLOSED;
    }
}

fn apply_closure_cash(ledger: &mut LedgerState, family_code: i64, yen_k: i64) {
    let amount = yen_k * 1_000;
    if amount > 0 {
        ledger.cash_balance = ledger.cap().min(ledger.cash_balance + amount);
        if (0..256).contains(&family_code) {
            // Writes past the ten-element array grow it with holes
            // (TRANSLATION.md, "Settled while translating the foundation").
            js::slot_mut(&mut ledger.income_ledger, family_code).add_assign(amount);
        }
    } else {
        let debit = -amount;
        ledger.cash_balance = 0.max(ledger.cash_balance - debit);
        if (0..256).contains(&family_code) {
            ledger.expense_ledger[family_code as usize] += debit;
        }
    }
}

/// Upstream takes the placed object and its venue record; here their key in
/// `world.placed_objects` and index in `world.sidecars`.
pub fn activate_retail_shop(world: &mut WorldState, object_key: &str, record_index: i64, ledger: &mut LedgerState) {
    venue_mut(&mut world.sidecars, record_index)
        .expect("activateRetailShop: not a commercial venue record")
        .availability_state = VENUE_PARTIAL;

    let object = &world.object(object_key).unwrap();
    add_cashflow_from_family_resource(ledger, "retail", object.rent_level, object.object_type_code);
    add_to_population_bucket(world, FAMILY_RETAIL, 10);
}

fn deactivate_retail_shop(world: &mut WorldState, object_key: &str, record_index: i64, ledger: &mut LedgerState) {
    venue_mut(&mut world.sidecars, record_index)
        .expect("deactivateRetailShop: not a commercial venue record")
        .availability_state = VENUE_DORMANT;
    // Binary deactivate_retail_shop_cashflow (1180:1255):
    //   1180:1296: MOV byte ptr ES:[BX+0x13],1  (dirty)
    //   1180:12a6: MOV byte ptr ES:[BX+0x14],0  (occupied cleared)
    let object = world.object_mut(object_key).unwrap();
    object.dirty_flag = 1;
    object.occupied_flag = 0;
    object.activation_tick_count = 0;

    remove_cashflow_from_family_resource(ledger, "retail", object.rent_level, object.object_type_code);
    add_to_population_bucket(world, FAMILY_RETAIL, -10);
}

pub fn refund_unhappy_facilities(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    // Object.entries: no object is added or removed in the loop.
    for key in world.placed_objects.key_list() {
        let object = &world.object(key.as_str()).unwrap();
        if object.object_type_code == FAMILY_CONDO {
            if object.eval_level != 0 {
                continue;
            }
            if object.unit_status >= UNIT_STATUS_CONDO_VACANT {
                continue;
            }
            remove_cashflow_from_family_resource(ledger, "condo", object.rent_level, object.object_type_code);
            add_to_population_bucket(world, FAMILY_CONDO, -3);
            let object = world.object_mut(&key).unwrap();
            object.unit_status = if time.daypart_index < 4 {
                UNIT_STATUS_CONDO_VACANT
            } else {
                UNIT_STATUS_CONDO_VACANT_EVENING
            };
            // Binary revert_condo_to_unsold (1180:1102):
            //   1180:115b: MOV byte ptr ES:[BX+0x13],1  (dirty)
            //   1180:116c: MOV byte ptr ES:[BX+0x14],0  (occupied cleared)
            object.dirty_flag = 1;
            object.occupied_flag = 0;
            object.activation_tick_count = 0;

            let (x, y) = key_xy(&key);
            for sim in world.sims.iter_mut() {
                if sim.home_column == x && sim.floor_anchor == y_to_floor(y) {
                    sim.state_code = STATE_PARKED;
                    sim.selected_floor = sim.floor_anchor;
                    sim.destination_floor = -1;
                    sim.venue_return_state = 0;
                    clear_sim_route(sim);
                }
            }
            continue;
        }

        if object.object_type_code == FAMILY_RETAIL {
            if object.eval_level != 0 {
                continue;
            }
            if object.linked_record_index < 0 {
                continue;
            }
            let linked = object.linked_record_index;
            let Some(record) = venue_mut(&mut world.sidecars, linked) else {
                continue;
            };
            if record.availability_state == VENUE_DORMANT {
                continue;
            }
            deactivate_retail_shop(world, &key, linked, ledger);
        }
    }
}

/// Mirrors binary `clamp_object_type_limit` (11b0:121c). After each sim
/// visit acquisition, increments the active phase seed by +2 (low stress)
/// or +1 (medium stress), capped at the type-specific tuning limit.
/// The active slot is selected by `selectFacilityProgressSlot`.
///
/// Stress is `compute_runtime_tile_stress_average` (1138:037b): average
/// elapsed ticks per trip = accumulatedTicks / tripCount (0 when no trips).
/// Thresholds from DS:0xe5ea/0xe5ec match OP_SCORE_THRESHOLDS: [80, 150]
/// for stars 1–3, [80, 200] for stars 4–5.
///
/// Upstream takes the venue record and the sim; here their indices in
/// `world.sidecars` and `world.sims`.
pub fn increment_venue_seed(
    record_index: i64,
    family_code: i64,
    sim_index: i64,
    world: &mut WorldState,
    time: &TimeState,
) {
    let Some(caps) = js::record_get(COMMERCIAL_CAPACITY_CAPS, family_code).copied() else {
        return;
    };
    let slot = select_facility_progress_slot(world, time);
    let current = read_seed_for_slot(
        venue_mut(&mut world.sidecars, record_index).expect("incrementVenueSeed: not a commercial venue record"),
        slot,
    );

    // Binary: stress = accumulated_elapsed / sample_count, or 0 if no trips.
    let sim = &world.sims[sim_index as usize];
    let stress = if sim.trip_count > 0 {
        js::trunc_div(sim.accumulated_ticks, sim.trip_count)
    } else {
        0
    };
    let [lower, upper] = js::record_get(OP_SCORE_THRESHOLDS, world.star_count.min(5))
        .copied()
        .unwrap_or([80, 150]);
    let increment = if stress < lower {
        2
    } else if stress < upper {
        1
    } else {
        0
    };

    write_seed_for_slot(
        venue_mut(&mut world.sidecars, record_index).unwrap(),
        slot,
        (current + increment).min(caps[0]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn venue() -> CommercialVenueRecord {
        CommercialVenueRecord {
            owner_subtype_index: 0,
            capacity: 0,
            visit_count: 0,
            today_visit_count: 0,
            yesterday_visit_count: 0,
            acquire_count: 0,
            availability_state: VENUE_AVAILABLE,
            current_population: 0,
            last_acquire_tick: 0,
            eligibility_threshold: 0,
            remaining_capacity: 0,
            phase_a_seed: 0,
            phase_b_seed: 0,
            override_seed: 0,
        }
    }

    #[test]
    fn closure_cash_bands_and_ledger_holes() {
        let mut world = crate::snapshot::create_initial_snapshot("t", "t", 0, None).world;
        world.placed_objects.clear();
        world.sidecars.clear();
        let mut ledger = crate::ledger::create_ledger_state(10_000);
        let mut v = venue();
        v.acquire_count = 36; // bands 25, 35 passed: payouts[2]
        world.sidecars.push(SidecarRecord::CommercialVenue(v));
        let mut v = venue();
        v.acquire_count = 3; // band 0: a debit
        world.sidecars.push(SidecarRecord::CommercialVenue(v));
        for (i, x) in [(0, 10), (1, 40)] {
            world.placed_objects.insert(
                format!("{x},100"),
                crate::world::PlacedObjectRecord {
                    left_tile_index: x,
                    right_tile_index: x + 15,
                    object_type_code: FAMILY_FAST_FOOD,
                    unit_status: 0,
                    aux_value_or_timer: 0,
                    linked_record_index: i,
                    dirty_flag: 0,
                    occupied_flag: 0,
                    eval_level: 0,
                    eval_score: -1,
                    rent_level: 1,
                    activation_tick_count: 0,
                    housekeeping_claimed_flag: 0,
                    vip_flag: None,
                },
            );
        }
        close_commercial_venues_by_family(&mut world, &mut ledger, FAMILY_FAST_FOOD);
        // fastFood payouts [-3, 2, 3, 5]: +3000 then -3000.
        assert_eq!(ledger.cash_balance, 10_000);
        assert_eq!(js::slot_at(&ledger.income_ledger, FAMILY_FAST_FOOD), js::JsSlot::NaN);
        assert_eq!(ledger.expense_ledger[FAMILY_FAST_FOOD as usize], 3_000);
        for s in &world.sidecars {
            let SidecarRecord::CommercialVenue(v) = s else { panic!() };
            assert_eq!(v.availability_state, VENUE_CLOSED);
        }
    }

    #[test]
    fn seed_slots() {
        let mut v = venue();
        write_seed_for_slot(&mut v, 4, 7);
        write_seed_for_slot(&mut v, 5, 9);
        write_seed_for_slot(&mut v, 3, 1);
        assert_eq!((v.phase_a_seed, v.phase_b_seed, v.override_seed), (1, 7, 9));
        assert_eq!(read_seed_for_slot(&v, 4), 7);
    }
}

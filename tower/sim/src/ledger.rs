// ts: ledger.ts

use serde::{Deserialize, Serialize};

use crate::js::{self, JsSlot};
use crate::resources::{
    FAMILY_CINEMA, FAMILY_CINEMA_LOWER, FAMILY_CINEMA_STAIRS_LOWER, FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CONDO, FAMILY_FAST_FOOD, FAMILY_HOTEL_SINGLE, FAMILY_HOTEL_SUITE, FAMILY_HOTEL_TWIN,
    FAMILY_OFFICE, FAMILY_PARTY_HALL, FAMILY_PARTY_HALL_LOWER, FAMILY_RESTAURANT, FAMILY_RETAIL,
    PARKING_EXPENSE_RATE_BY_STAR, QUARTERLY_EXPENSES, YEN_1001,
};
use crate::sims::states::ENTITY_POPULATION_BY_TYPE;
use crate::world::{y_to_floor, SidecarRecord, WorldState, UNDERGROUND_FLOORS, VENUE_DORMANT};

// ─── Three-ledger money model ─────────────────────────────────────────────────
//
// cashBalance          — live balance, capped at 99,999,999
// populationLedger[f]  — live per-family active-unit counts
// incomeLedger[f]      — income accumulated since last 3-day rollover
// expenseLedger[f]     — expenses accumulated since last 3-day rollover
// cashBalanceCycleBase — balance saved at each rollover (net delta reporting)
//
// YEN #1001 / #1002 values are in units of ¥1,000.

const YEN_UNIT: i64 = 1_000;
const CASH_CAP: i64 = 99_999_999;

const LEDGER_OFFICE: i64 = 0;
const LEDGER_HOTEL_SINGLE: i64 = 1;
const LEDGER_HOTEL_TWIN: i64 = 2;
const LEDGER_HOTEL_SUITE: i64 = 3;
const LEDGER_RETAIL: i64 = 4;
const LEDGER_FAST_FOOD: i64 = 5;
const LEDGER_RESTAURANT: i64 = 6;
const LEDGER_PARTY_HALL: i64 = 7;
const LEDGER_CINEMA: i64 = 8;
const LEDGER_CONDO: i64 = 9;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerState {
    pub cash_balance: i64,
    /// Live per-family active-unit counts indexed by objectTypeCode.
    pub population_ledger: Vec<i64>,
    /// Income since last 3-day rollover, indexed by objectTypeCode.
    ///
    /// Ten elements to begin with, but upstream also writes it at family
    /// codes (sims/facility-refunds.ts `incomeLedger[familyCode] += amount`,
    /// entertainment.ts at FAMILY_PARTY_HALL and FAMILY_CINEMA), which grows
    /// the JavaScript array with holes, and `hole += x` is NaN; a snapshot
    /// shows both as null.  So its elements are `JsSlot`s: write them
    /// through `js::slot_mut`.
    pub income_ledger: Vec<JsSlot>,
    /// Expenses since last 3-day rollover, indexed by objectTypeCode.
    pub expense_ledger: Vec<i64>,
    /// Balance saved at last rollover.
    pub cash_balance_cycle_base: i64,
    /// BOX: BOX RULES -- the most the funds may be (0: upstream's
    /// CASH_CAP).  Saved only when set, so a classic snapshot is unchanged.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub box_cash_cap: i64,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// BOX: BOX RULES' cap on the funds, for the billion-dollar tower and any
/// tower that grows rich: $9,999,999,999.
pub const BOX_CASH_CAP: i64 = 9_999_999_999;

impl LedgerState {
    /// The most the funds may be: upstream's $99,999,999, or BOX RULES'.
    pub fn cap(&self) -> i64 {
        if self.box_cash_cap > 0 {
            self.box_cash_cap
        } else {
            CASH_CAP
        }
    }
}

pub fn create_ledger_state(starting_cash: i64) -> LedgerState {
    LedgerState {
        cash_balance: starting_cash,
        population_ledger: vec![0; 10],
        income_ledger: vec![JsSlot::Number(0); 10],
        expense_ledger: vec![0; 256],
        cash_balance_cycle_base: starting_cash,
        box_cash_cap: 0,
    }
}

pub fn family_to_ledger_index(family_code: i64) -> i64 {
    match family_code {
        FAMILY_OFFICE => LEDGER_OFFICE,
        FAMILY_HOTEL_SINGLE => LEDGER_HOTEL_SINGLE,
        FAMILY_HOTEL_TWIN => LEDGER_HOTEL_TWIN,
        FAMILY_HOTEL_SUITE => LEDGER_HOTEL_SUITE,
        FAMILY_RETAIL => LEDGER_RETAIL,
        FAMILY_FAST_FOOD => LEDGER_FAST_FOOD,
        FAMILY_RESTAURANT => LEDGER_RESTAURANT,
        FAMILY_PARTY_HALL | FAMILY_PARTY_HALL_LOWER => LEDGER_PARTY_HALL,
        FAMILY_CINEMA | FAMILY_CINEMA_LOWER | FAMILY_CINEMA_STAIRS_UPPER | FAMILY_CINEMA_STAIRS_LOWER => {
            LEDGER_CINEMA
        }
        FAMILY_CONDO => LEDGER_CONDO,
        _ => -1,
    }
}

// ─── Income ───────────────────────────────────────────────────────────────────

/// The YEN #1001 payout for `rent_level` in yen: `payouts[Math.min(rentLevel,
/// 3)] * YEN_UNIT`.  Every payout times 1,000 is an exact integer.  (A
/// negative rent level would read `undefined` and make NaN in upstream; it
/// never happens, and panics here.)
fn yen_1001_amount(payouts: &[f64; 4], rent_level: i64) -> i64 {
    (payouts[rent_level.min(3) as usize] * YEN_UNIT as f64) as i64
}

/// Credit checkout/activation income for a placed object using YEN #1001.
/// Called by sim checkout handlers (Phase 4). tileName is the canonical
/// string key (e.g. "hotelSingle", "office").
pub fn add_cashflow_from_family_resource(
    ledger: &mut LedgerState,
    tile_name: &str,
    rent_level: i64,
    family_code: i64,
) {
    let Some(payouts) = js::record_get(YEN_1001, tile_name) else {
        return;
    };
    let amount = yen_1001_amount(payouts, rent_level);
    ledger.cash_balance = ledger.cap().min(ledger.cash_balance + amount);
    if (0..256).contains(&family_code) {
        let index = family_to_ledger_index(family_code);
        if index == -1 {
            return;
        }
        js::slot_mut(&mut ledger.income_ledger, index).add_assign(amount);
    }
}

pub fn remove_cashflow_from_family_resource(
    ledger: &mut LedgerState,
    tile_name: &str,
    rent_level: i64,
    family_code: i64,
) {
    let Some(payouts) = js::record_get(YEN_1001, tile_name) else {
        return;
    };
    let amount = yen_1001_amount(payouts, rent_level);
    ledger.cash_balance = 0.max(ledger.cash_balance - amount);
    if (0..256).contains(&family_code) {
        let index = family_to_ledger_index(family_code);
        if index == -1 {
            return;
        }
        // Math.max(0, x - amount): NaN stays NaN.
        let slot = js::slot_mut(&mut ledger.income_ledger, index);
        *slot = JsSlot::from_number(slot.to_number().map(|v| 0.max(v - amount)));
    }
}

// ─── Expense sweep ────────────────────────────────────────────────────────────

/// The y of a "x,y" cell key (`key.split(",").map(Number)[1]`).
fn key_y(key: &str) -> i64 {
    key.split(',')
        .nth(1)
        .and_then(|y| y.parse().ok())
        .unwrap_or_else(|| panic!("cell key {key:?} is not \"x,y\""))
}

/// Charge operating expenses for all placed tiles (YEN #1002).
/// Called at checkpoint 0x09e5 every 3 days.
pub fn do_expense_sweep(ledger: &mut LedgerState, world: &WorldState) {
    let parking_rate = js::record_get(PARKING_EXPENSE_RATE_BY_STAR, world.star_count.min(5))
        .copied()
        .unwrap_or(0);

    let lobby_floor = UNDERGROUND_FLOORS; // internal floor index of ground lobby
    let lobby_height = 1.max(world.lobby_height);

    for (key, obj) in world.placed_objects.iter() {
        let code = obj.object_type_code;

        // Parking: star-dependent rate × width / 10
        if code == 0x18 {
            if parking_rate > 0 {
                // Skip upper floors of multi-floor lobby
                let y = key_y(key);
                let floor = y_to_floor(y);
                if floor >= lobby_floor + 1 && floor < lobby_floor + lobby_height {
                    continue;
                }
                let width = obj.right_tile_index - obj.left_tile_index + 1;
                // BOX: car elevator -- under BOX RULES a parking space's
                // upkeep is what its cars' fees can bear (lobby cells, the
                // same family, keep upstream's rate).
                let amount = crate::box_ext::car_elevator::space_upkeep(world, key)
                    .unwrap_or(js::trunc_div(width * parking_rate, 10) * YEN_UNIT);
                ledger.cash_balance = 0.max(ledger.cash_balance - amount);
                ledger.expense_ledger[code as usize] += amount;
            }
            continue;
        }

        let rate = js::record_get(QUARTERLY_EXPENSES, code).copied().unwrap_or(0);
        if rate == 0 {
            continue;
        }
        let amount = rate * YEN_UNIT;
        ledger.cash_balance = 0.max(ledger.cash_balance - amount);
        if (0..256).contains(&code) {
            ledger.expense_ledger[code as usize] += amount;
        }
    }

    for carrier in &world.carriers {
        let active_car_count = carrier.cars.iter().filter(|car| car.active).count() as i64;
        if active_car_count == 0 {
            continue;
        }
        let family_code = if carrier.carrier_mode == 0 {
            42
        } else if carrier.carrier_mode == 2 {
            43
        } else {
            1
        };
        let rate = js::record_get(QUARTERLY_EXPENSES, family_code).copied().unwrap_or(0);
        if rate == 0 {
            continue;
        }
        let amount = rate * YEN_UNIT * active_car_count;
        ledger.cash_balance = 0.max(ledger.cash_balance - amount);
        ledger.expense_ledger[family_code as usize] += amount;
    }

    // BOX: car elevator -- under BOX RULES a car elevator costs to run as a
    // lift's car does (nothing with BOX RULES off).
    crate::box_ext::car_elevator::upkeep(ledger, world);

    for segment in &world.special_links {
        if !segment.active {
            continue;
        }
        let units = 1.max(js::sar(segment.flags, 1));
        // Binary: bit 0 clear → escalator (family 27); bit 0 set → stairs (family 22, rate 0).
        let family_code = if (segment.flags & 1) == 1 { 22 } else { 27 };
        let rate = js::record_get(QUARTERLY_EXPENSES, family_code).copied().unwrap_or(0);
        if rate == 0 {
            continue;
        }
        let amount = rate * YEN_UNIT * units;
        ledger.cash_balance = 0.max(ledger.cash_balance - amount);
        ledger.expense_ledger[family_code as usize] += amount;
    }
}

// ─── Facility ledger rebuild ──────────────────────────────────────────────────

/// Rebuild populationLedger count by sweeping all placedObjects.
/// Maps to rebuild_facility_ledger in the binary.
/// Called at checkpoint 0x00f0 (start of day).
pub fn rebuild_facility_ledger(ledger: &mut LedgerState, world: &WorldState) {
    ledger.population_ledger.fill(0);
    for obj in world.placed_objects.values() {
        let code = obj.object_type_code;
        if (0..256).contains(&code) {
            let index = family_to_ledger_index(code);
            if index == -1 {
                continue;
            }
            let pop = js::record_get(ENTITY_POPULATION_BY_TYPE, code).copied().unwrap_or(0);
            ledger.population_ledger[index as usize] += pop;
        }
    }
}

// ─── 3-day rollover ───────────────────────────────────────────────────────────

/// Called at checkpoint 0x09e5.
/// If this is a 3-day boundary (dayCounter % 3 === 0), run the full expense
/// sweep, save the cycle base, and reset rolling ledgers.
pub fn do_ledger_rollover(ledger: &mut LedgerState, _world: &WorldState, day_counter: i64) {
    if day_counter % 3 != 0 {
        return;
    }
    ledger.cash_balance_cycle_base = ledger.cash_balance;
    ledger.income_ledger.fill(JsSlot::Number(0));
    ledger.expense_ledger.fill(0);
}

/// Mirrors binary `recompute_all_operational_status_and_cashflow` at the 3-day
/// checkpoint. Offices activate unconditionally; retail activates only once its
/// occupied flag (+0x14) has been set (by the scoring sweep's first
/// eval_level>0 pass). Both share auxValueOrTimer with the per-sim handlers as
/// a once-per-cycle guard.
///
/// Offices here only re-pay if they're currently operational
/// (`occupiedFlag === 1`, binary +0x14) AND have been rented at least once
/// (`auxValueOrTimer !== 0`, flipped by a prior per-sim morning-gate dispatch).
/// Never-rented offices (unreachable from the lobby, e.g. above a sky lobby
/// without a transfer) stay at the initial `auxValueOrTimer === 0`, and
/// stress-vacated offices get `occupiedFlag === 0` via
/// `refreshOccupiedFlagAndTripCounters` branch C. Both skip here, matching the
/// binary where office rent only credits through `activate_office_cashflow`
/// (1180:0d2e) from the per-sim state-0x20 path.
pub fn activate_three_day_cashflow(world: &mut WorldState, ledger: &mut LedgerState, day_counter: i64) {
    let guard = day_counter + 1;
    let WorldState {
        placed_objects,
        sidecars,
        ..
    } = world;
    for obj in placed_objects.values_mut() {
        if obj.object_type_code == FAMILY_OFFICE {
            if obj.aux_value_or_timer == guard {
                continue;
            }
            if obj.aux_value_or_timer == 0 {
                continue;
            }
            if obj.occupied_flag != 1 {
                continue;
            }
            obj.aux_value_or_timer = guard;
            add_cashflow_from_family_resource(ledger, "office", obj.rent_level, FAMILY_OFFICE);
            continue;
        }
        if obj.object_type_code == FAMILY_RETAIL {
            if obj.aux_value_or_timer == guard {
                continue;
            }
            if obj.occupied_flag != 1 {
                continue;
            }
            // Binary activate_family_cashflow_if_operational (1138:0bad) gates
            // family 10 on the linked CommercialVenueRecord.availability_state
            // being >= 0 (signed). VENUE_DORMANT (0xff → -1) is excluded.
            let sidecar = if obj.linked_record_index >= 0 {
                sidecars.get(obj.linked_record_index as usize)
            } else {
                None
            };
            match sidecar {
                Some(SidecarRecord::CommercialVenue(v)) if v.availability_state != VENUE_DORMANT => {}
                _ => continue,
            }
            obj.aux_value_or_timer = guard;
            add_cashflow_from_family_resource(ledger, "retail", obj.rent_level, FAMILY_RETAIL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cashflow_and_holes() {
        let mut l = create_ledger_state(1_000);
        add_cashflow_from_family_resource(&mut l, "hotelTwin", 3, FAMILY_HOTEL_TWIN);
        assert_eq!(l.cash_balance, 1_800);
        assert_eq!(l.income_ledger[2], JsSlot::Number(800));
        add_cashflow_from_family_resource(&mut l, "hotelSingle", 9, FAMILY_HOTEL_SINGLE); // tier 3
        assert_eq!(l.cash_balance, 2_300);
        remove_cashflow_from_family_resource(&mut l, "condo", 0, FAMILY_CONDO);
        assert_eq!(l.cash_balance, 0);
        assert_eq!(l.income_ledger[9], JsSlot::Number(0));
        add_cashflow_from_family_resource(&mut l, "nothing", 0, FAMILY_CONDO);
        assert_eq!(l.cash_balance, 0);
        // What facility-refunds.ts does to it: a hole and then NaN.
        js::slot_mut(&mut l.income_ledger, FAMILY_FAST_FOOD).add_assign(3_000);
        assert_eq!(
            serde_json::to_string(&l.income_ledger).unwrap(),
            "[0,500,800,0,0,0,0,0,0,0,null,null,null]"
        );
        do_ledger_rollover(&mut l, &crate::snapshot::create_initial_snapshot("t", "t", 0, None).world, 3);
        assert!(l.income_ledger.iter().all(|s| *s == JsSlot::Number(0)));
        assert_eq!(l.income_ledger.len(), 13);
    }

    #[test]
    fn ledger_indices() {
        assert_eq!(family_to_ledger_index(FAMILY_CINEMA_STAIRS_LOWER), LEDGER_CINEMA);
        assert_eq!(family_to_ledger_index(FAMILY_PARTY_HALL_LOWER), LEDGER_PARTY_HALL);
        assert_eq!(family_to_ledger_index(24), -1);
    }
}

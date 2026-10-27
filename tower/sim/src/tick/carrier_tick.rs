// ts: tick/carrier-tick.ts

// 1098:03ab carrier_tick (FUN_1098_03ab)
//
// Per-tick driver for the elevator + sim subsystem. Order matches the binary:
//   check_and_advance_star_rating              (1148:002d)
//   refresh_runtime_entities_for_tick_stride   (1228:0d64)
//   for each carrier:
//     for each active car: advance_carrier_car_state     (1098:06fb)
//     for each active car: dispatch_carrier_car_arrivals (1218:07a6)
//     for each active car: process_unit_travel_queue     (1218:0351)
//

use crate::carriers::{
    advance_carrier_car_state, dispatch_carrier_car_arrivals, process_unit_travel_queue,
    reset_carrier_tick_bookkeeping,
};
use crate::ledger::LedgerState;
use crate::progression::try_advance_star_count;
use crate::sims::{reconcile_sim_transport, refresh_runtime_entities_for_tick_stride};
use crate::time::TimeState;
use crate::world::WorldState;

/// The loops reach each carrier and car by index, reading the array's length
/// afresh on every step as JavaScript's array iterator does.  (Nothing in a
/// carrier tick adds or removes carriers or cars.)
pub fn carrier_tick(world: &mut WorldState, ledger: &mut LedgerState, time: &TimeState) {
    // Binary `check_and_advance_star_rating` runs at the top of FUN_1098_03ab,
    // before refresh_runtime_entities_for_tick_stride. It compares the
    // per-tick-updated `g_primary_family_ledger_total` against the tier
    // thresholds and may bump `starCount` mid-tick.
    try_advance_star_count(world, time);

    refresh_runtime_entities_for_tick_stride(world, ledger, time);

    let mut carrier_index = 0;
    while carrier_index < world.carriers.len() {
        reset_carrier_tick_bookkeeping(&mut world.carriers[carrier_index]);
        let mut car_index = 0;
        while car_index < world.carriers[carrier_index].cars.len() {
            let lobby_mode = world.lobby_mode;
            advance_carrier_car_state(&mut world.carriers[carrier_index], car_index as i64, time, lobby_mode);
            car_index += 1;
        }
        let mut car_index = 0;
        while car_index < world.carriers[carrier_index].cars.len() {
            dispatch_carrier_car_arrivals(world, ledger, time, carrier_index as i64, car_index as i64);
            car_index += 1;
        }
        let mut car_index = 0;
        while car_index < world.carriers[carrier_index].cars.len() {
            process_unit_travel_queue(world, carrier_index as i64, car_index as i64, time);
            car_index += 1;
        }
        carrier_index += 1;
    }

    reconcile_sim_transport(world, ledger, time);
}

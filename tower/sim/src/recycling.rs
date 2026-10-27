// ts: recycling.ts

use crate::ledger::LedgerState;
use crate::resources::{FAMILY_RECYCLING_CENTER_LOWER, FAMILY_RECYCLING_CENTER_UPPER};
use crate::world::WorldState;

pub fn reset_recycling_center_duty_tier(world: &mut WorldState) {
    for object in world.placed_objects.values_mut() {
        if object.object_type_code == FAMILY_RECYCLING_CENTER_LOWER && object.unit_status == 6 {
            object.unit_status = 0;
        }
    }
}

pub fn update_recycling_center_state(world: &mut WorldState, ledger: &LedgerState, param: i64) {
    if world.star_count <= 2 {
        world.gate_flags.recycling_adequate = 0;
        return;
    }

    if param == 0 {
        world.gate_flags.recycling_adequate = 0;
        if world.gate_flags.recycling_center_count == 0 {
            return;
        }
        for object in world.placed_objects.values_mut() {
            if object.object_type_code == FAMILY_RECYCLING_CENTER_UPPER
                || object.object_type_code == FAMILY_RECYCLING_CENTER_LOWER
            {
                if object.unit_status == 5 {
                    continue;
                }
                object.unit_status = 0;
            }
        }
        return;
    }

    let scale = 0.max(world.gate_flags.recycling_center_count);
    if scale == 0 {
        world.gate_flags.recycling_adequate = 0;
        return;
    }

    let primary_total: i64 = ledger.population_ledger.iter().sum();
    let scaled = crate::js::trunc_div(primary_total, scale);
    let required_tier = if scaled < 500 {
        1
    } else if scaled < 1000 {
        2
    } else if scaled < 1500 {
        3
    } else if scaled < 2000 {
        4
    } else if scaled < 2500 {
        5
    } else {
        6
    };
    let adequate = if param >= required_tier { 1 } else { 0 };
    let duty_tier = if adequate != 0 { required_tier.min(param) } else { param };

    world.gate_flags.recycling_adequate = adequate;
    for object in world.placed_objects.values_mut() {
        if object.object_type_code == FAMILY_RECYCLING_CENTER_UPPER
            || object.object_type_code == FAMILY_RECYCLING_CENTER_LOWER
        {
            if adequate == 0 && object.unit_status == 5 {
                continue;
            }
            object.unit_status = duty_tier;
        }
    }
}

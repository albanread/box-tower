// ts: sim-access/selectors.ts

// 1228:681d..688c + 1228:6700 + 1228:65c1 + 1228:6757 + 1228:662a + 1228:640c + 1228:67d7
//
// Binary sim-level selector / accessor helpers.

use crate::families::housekeeping::activate_object_family_housekeeping_connection_state;
use crate::world::{SidecarRecord, SimRecord, WorldState};

/// 1228:681d get_current_sim_type — byte at sim+4 (family_code).
pub fn get_current_sim_type(sim: &SimRecord) -> i64 {
    sim.family_code
}

/// 1228:6854 get_current_sim_variant — byte at sim+1 (object_subtype_index).
pub fn get_current_sim_variant(sim: &SimRecord) -> i64 {
    sim.facility_slot
}

/// 1228:688c get_current_sim_state_word — word at sim+2 (object_base_offset in
///  the binary; TS uses `baseOffset` in the general case).
pub fn get_current_sim_state_word(sim: &SimRecord) -> i64 {
    sim.base_offset
}

/// 1228:6700 resolve_family_parking_selector_value.
///
/// (These three selectors take `(world, sim)`; `sim` is
/// `world.sims[sim_index]`.)
pub fn resolve_family_parking_selector_value(world: &WorldState, sim_index: i64) -> i64 {
    let sim = &world.sims[sim_index as usize];
    match sim.state_code {
        0x45 => 10,
        // Binary quirk: 0x6d = 109, outbound cathedral zone constant.
        0x60 => 0x6d,
        _ => -1,
    }
}

/// 1228:65c1 resolve_family_recycling_center_lower_selector_value.
pub fn resolve_family_recycling_center_lower_selector_value(world: &WorldState, sim_index: i64) -> i64 {
    let sim = &world.sims[sim_index as usize];
    match sim.state_code {
        0x41 => sim.destination_floor,
        0x62 => get_current_sim_type(sim) + 2,
        _ => -1,
    }
}

/// 1228:6757 get_housekeeping_room_claim_selector.
pub fn get_housekeeping_room_claim_selector(world: &WorldState, sim_index: i64) -> i64 {
    let sim = &world.sims[sim_index as usize];
    match sim.state_code {
        0x03 => sim.selected_floor,
        0x04 => get_current_sim_type(sim),
        _ => -1,
    }
}

/// 1228:662a dispatch_entertainment_guest_substate.
///
/// Compact 4-entry jump table at cs:0x66f0 for the entertainment guest
/// in-transit substates {0x41, 0x45, 0x60, 0x62}:
///   0x41 → CALLF 0x11b0:10fe (external advance_entertainment_attendance; TS: increment attendanceCounter)
///   0x45 → return 0xa (no-op return value 10)
///   0x60 → call get_current_sim_type + get_current_sim_variant, look up phase table, CALLF 0x1188:0d98
///   0x62 → return 0xa (same as 0x45)
///
/// Binary quirk: states 0x45 and 0x62 return the constant 0xa (10) with no
/// side effects. State 0x41 calls an external attendance-advancement routine.
/// State 0x60 performs a type+variant table lookup and calls a phase handler.
/// In TS, only the attendance increment for state 0x41 has an observable
/// effect; the other substates are no-ops at the current abstraction level.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn dispatch_entertainment_guest_substate(world: &mut WorldState, sim_index: i64) {
    let sim = &world.sims[sim_index as usize];
    let state = sim.state_code;
    let home_column = sim.home_column;

    if state == 0x41 {
        // Binary quirk: CALLF 0x11b0:10fe advances the entertainment attendance
        // counter for the active link record. In TS, find the link and increment.
        for sidecar in world.sidecars.iter_mut() {
            if let SidecarRecord::EntertainmentLink(link) = sidecar {
                if link.owner_subtype_index == home_column {
                    link.attendance_counter += 1;
                    break;
                }
            }
        }
    }
    // States 0x45 and 0x62: binary returns 0xa with no side effects.
    // State 0x60: binary looks up a phase table via type+variant and calls a
    // phase handler; no TS-visible state mutation at current abstraction level.
}

/// 1228:640c maybe_start_housekeeping_room_claim.
///
/// Gets the occupant runtime index for the sim's object. If the target sim
/// has stateCode == 0 and the star rating satisfies (starCount & 9) != 0,
/// activates the housekeeping connection state for that sim.
/// Binary quirk: (starCount & 9) != 0 covers stars 1 (bit 0) and 3+ (bit 3+
/// alias); in practice this gates housekeeping on tower rank.
///
/// (`sim` is `world.sims[sim_index]`.  Upstream never calls this function.)
pub fn maybe_start_housekeeping_room_claim(world: &mut WorldState, sim_index: i64) -> bool {
    let target_index = compute_object_occupant_runtime_index(world, sim_index);
    if target_index < 0 || target_index >= world.sims.len() as i64 {
        return false;
    }
    let target_sim = &world.sims[target_index as usize];
    if target_sim.state_code != 0 {
        return false;
    }
    // Binary quirk: (starRatingFlags & 9) != 0 — bit 0 = 1-star, bit 3 = 8+
    // but starCount is 1-indexed so we test the raw value bitwise.
    if world.star_count & 9 == 0 {
        return false;
    }
    let selected_floor = world.sims[sim_index as usize].selected_floor;
    // activateObjectFamilyHousekeepingConnectionState requires a TimeState,
    // but maybeStartHousekeepingRoomClaim is called from a context where
    // time is not available in this binary path; pass a minimal sentinel.
    // Binary quirk: the binary passes the global time pointer directly.
    //
    // (Upstream passes `null as never`, which throws if the handler reads
    // the time.  Rust cannot pass a null reference: a zero TimeState stands
    // in.  Upstream never calls this function.)
    let sentinel_time = crate::time::create_time_state();
    activate_object_family_housekeeping_connection_state(world, &sentinel_time, target_index, selected_floor);
    true
}

/// 1228:67d7 compute_object_occupant_runtime_index.
///
/// Returns the index in world.sims of the occupant slot identified by
/// (sim.floorAnchor, sim.facilitySlot, sim.baseOffset). The binary walks
/// g_unknown_ptr_array[floor].ptr facility structs to read the stored base
/// sim index for the per-floor object, then adds the occupant offset. In TS,
/// the equivalent is: find the index of the first sim sharing (floorAnchor,
/// facilitySlot) and add baseOffset.
///
/// (`sim` is `world.sims[sim_index]`.)
pub fn compute_object_occupant_runtime_index(world: &WorldState, sim_index: i64) -> i64 {
    let sim = &world.sims[sim_index as usize];
    let (floor_anchor, facility_slot, base_offset) = (sim.floor_anchor, sim.facility_slot, sim.base_offset);
    for (i, s) in world.sims.iter().enumerate() {
        if s.floor_anchor == floor_anchor && s.facility_slot == facility_slot && s.base_offset == 0 {
            return i as i64 + base_offset;
        }
    }
    -1
}

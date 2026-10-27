// ts: families/medical.ts

// families/medical.ts — Medical-center (family 0x0d) service queue + office-worker trip state machine.
//
// TODO: binary — no direct 1228:* function has been mapped to the medical
// subsystem yet. Once the medical dispatch handler is located, add the
// `SEG:OFFSET` header + binary-aligned names here.

pub use crate::sims::medical::{
    handle_medical_sim_arrival, invalidate_medical_slots_for_sidecar, process_medical_sim, try_start_medical_trip,
    MEDICAL_NOTIFICATION_MESSAGE, STATE_MEDICAL_DWELL, STATE_MEDICAL_TRIP, STATE_MEDICAL_TRIP_TRANSIT,
};

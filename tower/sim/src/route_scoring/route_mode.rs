// ts: route-scoring/route-mode.ts

// 11b8:1422 getCurrentSimRouteMode
//
// Binary: reads a pixel-column height metric from the facility subtype record
// for the current sim. The value is passed as `targetHeightMetric` into the
// route scorers (score_carrier_transfer_route, score_local_route_segment,
// score_housekeeping_route_segment) via select_best_route_candidate.
//
// The Ghidra-assigned name "get_current_sim_route_mode" is misleading — the
// return value is not a mode enum but a height reference used for distance
// scoring. In the TS implementation this role is filled by `sim.homeColumn`,
// passed directly to selectBestRouteCandidate. This function is therefore not
// called; it is preserved as a reference mapping to the binary address.
//
// `preferLocalMode` (whether to prefer escalator over stairs segments) is a
// separate boolean derived from sim.familyCode in resolveSimRouteBetweenFloors.

use crate::world::SimRecord;

/// Upstream's `"passenger" | "service"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimRouteMode {
    Passenger,
    Service,
}

const FAMILY_HOUSEKEEPING: i64 = 0x0f;

pub fn get_current_sim_route_mode(sim: &SimRecord) -> SimRouteMode {
    if sim.family_code == FAMILY_HOUSEKEEPING {
        SimRouteMode::Service
    } else {
        SimRouteMode::Passenger
    }
}

/// Derived helper: `true` when the scorer should prefer local (escalator)
/// segments, i.e. the sim is a passenger. Housekeeping prefers stairs.
pub fn sim_prefers_local_mode(sim: &SimRecord) -> bool {
    get_current_sim_route_mode(sim) == SimRouteMode::Passenger
}

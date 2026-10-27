// ts: route-scoring/select-cathedral.ts

// Cathedral guest (families 0x24-0x28) route selector wrapper.
//
// All five cathedral family codes share the parking-style state machine at
// 1228:5b5a (`gate_object_family_parking_state_handler`) and 1228:5cd2
// (`dispatch_object_family_parking_state_handler`). The two routing entry
// points — `handle_family_parking_outbound_route` (1228:5ddd) and
// `handle_family_parking_return_route` (1228:5e7e) — both call
// `resolve_sim_route_between_floors` (1218:0000) with `is_passenger_route = 1`
// (passenger-mode routing). That value forwards to
// `select_best_route_candidate` (11b8:1484) as `prefer_local_mode = 1`.
//
// In other words, cathedral routing is plain passenger routing. There is no
// per-family customization inside the route scorer; this wrapper exists to
// make the family→selector mapping explicit and binary-traceable in
// `selectRouteForFamily`.

use crate::world::WorldState;

use super::select_candidate::{select_best_route_candidate, RouteCandidate};

pub fn select_cathedral_route(
    world: &WorldState,
    from_floor: i64,
    to_floor: i64,
    target_height_metric: i64,
) -> Option<RouteCandidate> {
    select_best_route_candidate(
        world,
        from_floor,
        to_floor,
        /* preferLocalMode = */ Some(true),
        Some(target_height_metric),
    )
}

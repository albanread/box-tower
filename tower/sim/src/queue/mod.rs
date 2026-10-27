// ts: queue/index.ts

// Barrel for the queue/ submodule. Each export maps 1:1 to a binary
// function at segment 1218 (see file headers for SEG:OFFSET).

pub mod cancel;
pub mod dequeue;
pub mod dispatch_arrivals;
pub mod encoding;
pub mod enqueue;
pub mod process_travel;
pub mod resolve;
pub mod route_record;
pub mod scan;

pub use cancel::{cancel_runtime_route_request, decrement_route_queue_direction_load, dispatch_queued_route_until_request};
pub use dequeue::pop_unit_queue_request;
pub use dispatch_arrivals::{dispatch_carrier_car_arrivals, dispatch_destination_queue_entries};
pub use encoding::{
    decode_encoded_route_target_byte, decode_runtime_route_target, encode_runtime_route_target,
    RuntimeRouteTarget, ROUTE_TARGET_CARRIER_DOWN_BASE, ROUTE_TARGET_CARRIER_UP_BASE, ROUTE_TARGET_SPECIAL_LINK_MAX,
};
pub use enqueue::enqueue_request_into_route_queue;
pub use process_travel::{assign_request_to_runtime_route, process_unit_travel_queue};
pub use resolve::{resolve_sim_route_between_floors, ResolveSimRouteOptions, RouteResolution};
pub use route_record::{RouteRequestRing, ROUTE_QUEUE_CAPACITY_CONST};
pub use scan::{
    pop_active_route_slot_request, remove_request_from_active_route_slots, remove_request_from_unit_queue,
    store_request_in_active_route_slot,
};

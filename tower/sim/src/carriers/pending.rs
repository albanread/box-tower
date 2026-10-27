// ts: carriers/pending.ts

// 1098:0b10 decrement_car_pending_assignment_count
//
// Binary post-pass invoked per carrier at the end of `carrier_tick` (see
// ROUTING-BINARY-MAP.md §1). Distinct from the inline pendingAssignmentCount
// decrements already performed by `clearFloorRequestsOnArrival` when a route
// status slot is zeroed. The binary helper does additional housekeeping /
// hooks on top of those inline decrements.
//
// TODO(1098:0b10): TS currently folds this into the inline decrement inside
// `clearFloorRequestsOnArrival`. Extract a standalone pass if the binary's
// additional side effects (UI hooks, cross-car bookkeeping) diverge from our
// current inline behavior.

use crate::world::CarrierRecord;

/// Upstream takes `(carrier, car)`; here the car's index (TRANSLATION.md).
pub fn decrement_car_pending_assignment_count(_carrier: &mut CarrierRecord, _car_index: i64) {
    // Intentionally empty: binary post-pass semantics not yet separated from
    // the inline decrement in clearFloorRequestsOnArrival. See TODO above.
}

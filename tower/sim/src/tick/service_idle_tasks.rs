// ts: tick/service-idle-tasks.ts

// 1268:01a6 service_idle_tasks
//
// Win16 idle pass: between messages the binary calls
// `run_simulation_day_scheduler` followed by `carrier_tick`. This function
// mirrors that orchestration and serves as the entry point for
// `TowerSim.step()` (which, in Phase 1, is a thin wrapper plus UI-facing
// cell-patch + notification draining).
//
// Phase 7: the `onArrival` / `onBoarding` callback plumbing has been
// removed — family dispatch and stress accumulation happen inline inside
// the queue path, matching the binary's call graph.

use super::carrier_tick::carrier_tick;
use super::day_scheduler::{run_simulation_day_scheduler, SimCtx};

/// Mutable context passed into `serviceIdleTasks`. The function updates
/// `ctx.time` in place after the day scheduler advances the tick so the
/// inline arrival/boarding paths observe the advanced time (matching the
/// binary, which updates `g_day_tick` before `carrier_tick` runs).
pub type ServiceIdleTasksContext<'a> = SimCtx<'a>;

pub fn service_idle_tasks(ctx: &mut ServiceIdleTasksContext) {
    *ctx.time = run_simulation_day_scheduler(ctx.world, ctx.ledger, ctx.time);
    carrier_tick(ctx.world, ctx.ledger, ctx.time);
    // BOX: emergency -- medical incidents (nothing while BOX RULES are off).
    crate::box_ext::emergency::tick(ctx.world, ctx.ledger, ctx.time);
    // BOX: car elevator -- platforms and vehicles (nothing while BOX RULES are off).
    crate::box_ext::car_elevator::tick(ctx.world, ctx.time);
    // BOX: car elevator -- commuters whose cars have parked set off from them.
    crate::box_ext::car_elevator::commute(ctx.world, ctx.ledger, ctx.time);
    // BOX: car elevator -- residents' cars come home in the evening.
    crate::box_ext::car_elevator::residents(ctx.world, ctx.time);
    // BOX: street -- fire engines and police cars drawn in the street; runtime
    // presentation state only, never saved.
    crate::box_ext::street::tick(ctx.world);
    // BOX: fire -- the alarm, the evacuation and the all-clear (nothing while
    // BOX RULES are off).
    crate::box_ext::fire::tick(ctx.world, ctx.time);
}

// ts: scheduler.ts

// Phase 1 of the routing refactor moved the checkpoint driver into
// tick/day-scheduler.ts (1208:0196 run_simulation_day_scheduler). This file
// is retained as a thin re-export so existing callers keep compiling.
pub use crate::tick::day_scheduler::{run_checkpoints, run_simulation_day_scheduler, SimState};

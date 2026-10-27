//! The Rust half of the lockstep check (harness/lockstep.mjs runs the
//! TypeScript half and compares).
//!
//!     tower-lockstep FIXTURE [--every N] [--max-ticks M]
//!
//! Builds FIXTURE's tower exactly as tests/trace.rs does (the same code:
//! tests/support, included here by path), then steps it and writes one
//! JSON line per tick to stdout:
//!
//!     {"t":simTime,"day":dayCounter,"dayTick":dayTick,"step":StepResult|null,"snap":saveState()|null}
//!
//! The first line is the built tower before any step (`step` null).  `snap`
//! is written on the first line, every N steps after it and on the last.
//!
//! What it does each tick, which lockstep.mjs does the same:
//! - a trace fixture runs to its last trace entry (after
//!   dropTerminalDuplicateDump), placing each scheduled batch when the
//!   simulation reaches the trace entry with the batch's day and tick, as
//!   trace.test.ts does (batches are placed after that tick's step, last
//!   batch first);
//! - "fire" and "fire_accept" are seeded as their trace tests seed them and
//!   run 2700 steps, answering the fire prompt (declined for "fire",
//!   accepted for "fire_accept") after the first step whose dayTick reaches
//!   fireStartTick + 2.
//!
//! `--max-ticks M` stops after M steps.

#[path = "../../tests/support/mod.rs"]
mod support;

use std::io::{BufWriter, Write};

use serde_json::{json, Value};
use support::*;
use tower_sim::index::{SimCommand, StepResult, TowerSim};

const FIRE_STEPS: i64 = 2700;

struct Out {
    w: BufWriter<std::io::StdoutLock<'static>>,
}

impl Out {
    fn emit(&mut self, sim: &TowerSim, step: Option<&StepResult>, snap: bool) {
        let state = sim.save_state();
        let record = json!({
            "t": sim.sim_time(),
            "day": state.time.day_counter,
            "dayTick": state.time.day_tick,
            "step": step.map(|s| serde_json::to_value(s).unwrap()).unwrap_or(Value::Null),
            "snap": if snap { serde_json::to_value(&state).unwrap() } else { Value::Null },
        });
        serde_json::to_writer(&mut self.w, &record).unwrap();
        self.w.write_all(b"\n").unwrap();
        // Flushed every line so that the harness can stop at the first
        // difference (a closed pipe ends this process).
        if self.w.flush().is_err() {
            std::process::exit(0);
        }
    }
}

fn usage() -> ! {
    eprintln!("usage: tower-lockstep FIXTURE [--every N] [--max-ticks M] [--ticks T]");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut fixture: Option<String> = None;
    let mut every: i64 = 1;
    let mut max_ticks: Option<i64> = None;
    let mut ticks: Option<i64> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--every" => every = args.next().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or_else(|| usage()),
            "--max-ticks" => max_ticks = Some(args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage())),
            "--ticks" => ticks = Some(args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage())),
            _ if fixture.is_none() && !a.starts_with("--") => fixture = Some(a),
            _ => usage(),
        }
    }
    let fixture = fixture.unwrap_or_else(|| usage());
    let mut out = Out {
        w: BufWriter::new(std::io::stdout().lock()),
    };

    if fixture == "fire" || fixture == "fire_accept" {
        let accept = fixture == "fire_accept";
        let (spec, trace, _) = load_fixture(&fixture);
        let seed = &trace[0];
        let (tower_id, name) = if accept {
            ("trace-fire-accept", "Trace Fire Accept")
        } else {
            ("trace-fire", "Trace Fire")
        };
        let mut sim = seed_fire(&spec, seed, tower_id, name, accept);
        let respond_at = seed.fire.as_ref().expect("fixture must include fire state").fire_start_tick + 2;
        let mut responded = false;
        let steps = max_ticks.map_or(FIRE_STEPS, |m| m.min(FIRE_STEPS));
        out.emit(&sim, None, true);
        for i in 1..=steps {
            let r = sim.step();
            if !responded && sim.save_state().time.day_tick >= respond_at {
                sim.submit_command(&SimCommand::PromptResponse {
                    prompt_id: format!("fire_{}", seed.day),
                    accepted: accept,
                });
                responded = true;
            }
            out.emit(&sim, Some(&r), i % every == 0 || i == steps);
        }
        return;
    }

    let (spec, raw_trace, raw) = load_fixture(&fixture);
    let trace = drop_terminal_duplicate_dump(raw_trace, &raw);
    let sim_entries = &trace[1.min(trace.len())..];
    let mut sim = prepare_from_trace(&spec, &trace);

    // The scheduled batches the trace test places: those whose (day, tick)
    // is a simulation entry's, at that entry's total tick.
    let mut scheduled: Vec<(i64, Vec<FacilitySpec>)> = spec
        .scheduled_facilities
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|b| sim_entries.iter().any(|e| e.day == b.day && e.tick == b.tick))
        .map(|b| (trace_tick_to_total_ticks(b.day, b.tick), b.facilities))
        .collect();
    let apply_scheduled = |sim: &mut TowerSim, scheduled: &mut Vec<(i64, Vec<FacilitySpec>)>| {
        let mut i = scheduled.len();
        while i > 0 {
            i -= 1;
            if scheduled[i].0 == sim.sim_time() {
                let (_, facilities) = scheduled.remove(i);
                place_facility_list(sim, &facilities);
            }
        }
    };
    let end = sim_entries.last().map_or(sim.sim_time(), |e| trace_tick_to_total_ticks(e.day, e.tick));
    let end = match max_ticks {
        Some(m) => end.min(sim.sim_time() + m),
        None => end,
    };
    // --ticks T: exactly T steps, past the recording's end if need be
    let end = match ticks {
        Some(t) => sim.sim_time() + t,
        None => end,
    };

    apply_scheduled(&mut sim, &mut scheduled);
    out.emit(&sim, None, true);
    let mut i = 0;
    while sim.sim_time() < end {
        let r = sim.step();
        i += 1;
        apply_scheduled(&mut sim, &mut scheduled);
        let last = sim.sim_time() >= end;
        out.emit(&sim, Some(&r), i % every == 0 || last);
    }
}

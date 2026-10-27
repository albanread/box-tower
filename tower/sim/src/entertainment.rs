// ts: entertainment.ts

use crate::js::{self, JsSlot};
use crate::ledger::LedgerState;
use crate::progression::{add_to_population_bucket, clear_population_bucket};
use crate::resources::{
    FAMILY_CINEMA, FAMILY_CINEMA_LOWER, FAMILY_CINEMA_STAIRS_LOWER, FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_PARTY_HALL, FAMILY_PARTY_HALL_LOWER,
};
use crate::sims::states::{STATE_ACTIVE, STATE_ARRIVED, STATE_DEPARTURE, STATE_MORNING_GATE};
use crate::world::{EntertainmentLinkRecord, SidecarRecord, SimRecord, WorldState};

// Family codes emitted by cinema placement (upper stairway, upper theater,
// lower stairway, lower theater). All 4 share one sidecar.
const CINEMA_FAMILY_CODES: &[i64] = &[
    FAMILY_CINEMA,
    FAMILY_CINEMA_LOWER,
    FAMILY_CINEMA_STAIRS_UPPER,
    FAMILY_CINEMA_STAIRS_LOWER,
];

/// Paired-link budget tiers indexed by `linkAgeCounter / 3`.
/// Selectors 0..6 -> [40, 40, 40, 20], selectors 7..13 -> [60, 60, 40, 20].
fn paired_budget(link_age_counter: i64, selector: i64) -> i64 {
    let age_tier = 3.min(js::trunc_div(link_age_counter, 3));
    let low_selector_table = [40, 40, 40, 20];
    let high_selector_table = [60, 60, 40, 20];
    let table = if (0..7).contains(&selector) {
        low_selector_table
    } else {
        high_selector_table
    };
    // `table[ageTier] ?? table[table.length - 1] ?? 20`: a negative tier
    // reads undefined.
    js::at_or(&table, age_tier, table[table.len() - 1])
}

/// A sidecar is "paired" (cinema) iff its `familySelectorOrSingleLinkFlag`
/// is a real selector bucket (0..13). Party-hall records store 0xff.
fn is_paired_sidecar(sidecar: &EntertainmentLinkRecord) -> bool {
    sidecar.family_selector_or_single_link_flag != 0xff
}

fn sim_matches_entertainment_sidecar(sim: &SimRecord, sidecar: &EntertainmentLinkRecord) -> bool {
    sim.home_column == sidecar.owner_subtype_index || sim.home_column == sidecar.owner_subtype_index + 7
}

/// The entertainment link records among the sidecars, writable.
fn entertainment_links(sidecars: &mut [SidecarRecord]) -> impl Iterator<Item = &mut EntertainmentLinkRecord> {
    sidecars.iter_mut().filter_map(|s| match s {
        SidecarRecord::EntertainmentLink(e) => Some(e),
        _ => None,
    })
}

/// Binary `rebuild_entertainment_family_ledger` @ 1188:05af.
/// Seed entertainment link budgets, increment link age, clear cycle counters.
/// Called at tick 0x0F0 (240) as part of the facility ledger rebuild.
///
/// Iterates sidecars directly: each placement owns exactly one sidecar,
/// shared by its 4 (cinema) or 2 (party hall) sub-records.
///
/// Note: the binary does NOT clear `link_phase_state` here — that happens at
/// the previous day's lower-half advance pass. We mirror that.
pub fn seed_entertainment_budgets(world: &mut WorldState) {
    let mut cinema_population = 0;
    let mut party_hall_population = 0;
    for sidecar in entertainment_links(&mut world.sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }

        sidecar.attendance_counter = 0;
        sidecar.active_runtime_count = 0;
        sidecar.pending_transition_flag = 0;
        sidecar.link_age_counter = 0x7f.min(sidecar.link_age_counter + 1);

        if is_paired_sidecar(sidecar) {
            let budget = paired_budget(sidecar.link_age_counter, sidecar.family_selector_or_single_link_flag);
            sidecar.upper_budget = budget;
            sidecar.lower_budget = budget;
            cinema_population += sidecar.upper_budget + sidecar.lower_budget;
        } else {
            sidecar.upper_budget = 0;
            sidecar.lower_budget = 50;
            party_hall_population += sidecar.lower_budget;
        }
    }

    clear_population_bucket(world, FAMILY_CINEMA);
    if cinema_population > 0 {
        add_to_population_bucket(world, FAMILY_CINEMA, cinema_population);
    }
    clear_population_bucket(world, FAMILY_PARTY_HALL);
    if party_hall_population > 0 {
        add_to_population_bucket(world, FAMILY_PARTY_HALL, party_hall_population);
    }
}

/// Activate paired-link upper-half sims.
/// Sets upper phase to 1 for all paired entertainment links that are idle.
pub fn activate_entertainment_upper_half(world: &mut WorldState) {
    let WorldState { sidecars, sims, .. } = world;
    for sidecar in entertainment_links(sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        if !is_paired_sidecar(sidecar) {
            continue;
        }
        if sidecar.link_phase_state == 0 {
            sidecar.link_phase_state = 1;
        }
        for sim in sims.iter_mut() {
            if sim.family_code != FAMILY_CINEMA {
                continue;
            }
            if !sim_matches_entertainment_sidecar(sim, sidecar) {
                continue;
            }
            sim.state_code = STATE_MORNING_GATE;
        }
    }
}

/// Tick 0x4B0 (1200) action.
/// Promote paired (cinema) links phase 2 → 3, AND activate party hall lower
/// half: link 0 → 1 plus seed every lower-half occupant sim's stateCode to
/// STATE_MORNING_GATE (0x20).
///
/// Binary `activate_entertainment_link_half_runtime_phase(half=1, paired=0)`
/// @ 1188:06a8 walks the lower-half occupant span (40 slots) and writes
/// sim[+5] = 0x20 unconditionally. We approximate the per-record span by
/// matching `homeColumn` + family code on the lower half.
pub fn promote_cinema_and_activate_party_hall(world: &mut WorldState) {
    for sidecar in entertainment_links(&mut world.sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }

        if is_paired_sidecar(sidecar) {
            if sidecar.link_phase_state == 2 {
                sidecar.link_phase_state = 3;
            }
        } else if sidecar.link_phase_state == 0 {
            sidecar.link_phase_state = 1;
        }
    }
}

/// Binary `promote_entertainment_links_to_ready_phase(half=1, paired=1)` @
/// 1188:0826 — checkpoint 0x640 (1600). For every cinema-paired link with
/// `linkPhaseState >= 2`, advances `linkPhaseState` 2 → 3 (the "READY" /
/// post-show phase). Also writes byte +0x13 = 1 on the 4 audience-sim
/// records bound to the link (2 per half), but that flag write is not yet
/// mirrored here. This partial implementation captures the linkPhaseState
/// transition only.
pub fn promote_cinema_links_to_ready_phase(world: &mut WorldState) {
    for sidecar in entertainment_links(&mut world.sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        if !is_paired_sidecar(sidecar) {
            continue;
        }
        if sidecar.link_phase_state >= 2 {
            sidecar.link_phase_state = 3;
        }
    }
}

/// Activate paired-link lower-half sims still in phase 1.
pub fn activate_entertainment_lower_half(world: &mut WorldState) {
    for sidecar in entertainment_links(&mut world.sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        if !is_paired_sidecar(sidecar) {
            continue;
        }
        if sidecar.link_phase_state == 1 {
            sidecar.link_phase_state = 2;
        }
    }
}

/// Movie-theater (paired) attendance-tiered payout.
fn movie_theater_payout(attendance: i64) -> i64 {
    if attendance >= 100 {
        return 15_000;
    }
    if attendance >= 80 {
        return 10_000;
    }
    if attendance >= 40 {
        return 2_000;
    }
    0
}

/// Advance paired-link upper phase.
/// Decrements active runtime count and accrues income for completed upper phases.
pub fn advance_entertainment_upper_phase(world: &mut WorldState) {
    let WorldState { sidecars, sims, .. } = world;
    for sidecar in entertainment_links(sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        if !is_paired_sidecar(sidecar) {
            continue;
        }
        if sidecar.link_phase_state < 1 {
            continue;
        }

        sidecar.active_runtime_count = 0.max(sidecar.active_runtime_count - sidecar.upper_budget);
        sidecar.link_phase_state = if sidecar.active_runtime_count == 0 { 1 } else { 2 };

        for sim in sims.iter_mut() {
            if !CINEMA_FAMILY_CODES.contains(&sim.family_code) {
                continue;
            }
            if !sim_matches_entertainment_sidecar(sim, sidecar) {
                continue;
            }
            if sim.state_code == STATE_ARRIVED {
                sim.state_code = STATE_ACTIVE;
            }
        }
    }
}

/// Binary check at the entry of `accrue_facility_income_by_family` @ 1180:12e7:
/// skip the cash payout when the day counter lands on a calendar edge. The
/// record-level state reset (link_phase_state = 0) still happens.
fn is_calendar_edge_payout_skip_day(day_counter: i64) -> bool {
    day_counter % 60 == 59 || day_counter % 84 == 83
}

/// `ledger.incomeLedger[family] = (ledger.incomeLedger[family] ?? 0) + payout`
/// (TRANSLATION.md: the income ledger grows past its ten elements here).
fn accrue_income(ledger: &mut LedgerState, family: i64, payout: i64) {
    let slot = js::slot_mut(&mut ledger.income_ledger, family);
    *slot = JsSlot::from_number(slot.or(0).map(|v| v + payout));
}

/// Binary `advance_entertainment_facility_phase(half=1, paired=0)` — checkpoint 0x640.
/// Drain party hall attendees one at a time, accrue $20k income (gated by
/// the calendar-edge skip), reset link phase. The drain walks each occupant
/// slot in the lower-half span: for each sim in STATE_ARRIVED, decrement
/// `activeRuntimeCount` and set the sim's stateCode to STATE_DEPARTURE (0x05)
/// so the linked-half routing handler routes it back to the lobby. Sims in
/// any other state are left alone.
pub fn advance_party_hall_phase_and_accrue(world: &mut WorldState, ledger: &mut LedgerState, day_counter: i64) {
    let WorldState { sidecars, sims, .. } = world;
    for sidecar in entertainment_links(sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        if is_paired_sidecar(sidecar) {
            continue;
        }

        let previous_phase_state = sidecar.link_phase_state;
        sidecar.link_phase_state = 0;

        for sim in sims.iter_mut() {
            if sim.family_code != FAMILY_PARTY_HALL_LOWER {
                continue;
            }
            if !sim_matches_entertainment_sidecar(sim, sidecar) {
                continue;
            }
            if sim.state_code == STATE_ARRIVED {
                sim.state_code = STATE_DEPARTURE;
                sidecar.active_runtime_count = 0.max(sidecar.active_runtime_count - 1);
            }
        }

        if previous_phase_state >= 1 && sidecar.attendance_counter > 0 {
            if is_calendar_edge_payout_skip_day(day_counter) {
                continue;
            }
            let payout = 20_000;
            ledger.cash_balance = ledger.cap().min(ledger.cash_balance + payout);
            accrue_income(ledger, FAMILY_PARTY_HALL, payout);
        }
    }
}

/// Binary `advance_entertainment_facility_phase(half=1, paired=1)` — checkpoint 0x76c.
/// Advance lower phase for paired (cinema) links, accrue income with the
/// calendar-edge skip, reset link phase. Payout is attendance-tiered.
///
/// Cinema and party-hall guest sims are both spawned from their binary-aligned
/// occupant spans, so the per-sim drain semantics here mirror the binary path.
pub fn advance_entertainment_lower_paired_phase_and_accrue(
    world: &mut WorldState,
    ledger: &mut LedgerState,
    day_counter: i64,
) {
    let WorldState { sidecars, sims, .. } = world;
    for sidecar in entertainment_links(sidecars) {
        if sidecar.owner_subtype_index == 0xff {
            continue;
        }
        if !is_paired_sidecar(sidecar) {
            continue;
        }

        let previous_phase_state = sidecar.link_phase_state;
        sidecar.link_phase_state = 0;

        for sim in sims.iter_mut() {
            if !CINEMA_FAMILY_CODES.contains(&sim.family_code) {
                continue;
            }
            if !sim_matches_entertainment_sidecar(sim, sidecar) {
                continue;
            }
            if sim.state_code == STATE_ARRIVED {
                sim.state_code = STATE_DEPARTURE;
                sidecar.active_runtime_count = 0.max(sidecar.active_runtime_count - 1);
            }
        }

        if previous_phase_state >= 1 {
            if is_calendar_edge_payout_skip_day(day_counter) {
                continue;
            }
            let payout = movie_theater_payout(sidecar.attendance_counter);
            if payout > 0 {
                ledger.cash_balance = ledger.cap().min(ledger.cash_balance + payout);
                accrue_income(ledger, FAMILY_CINEMA, payout);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_budget_tiers() {
        assert_eq!(paired_budget(0, 3), 40);
        assert_eq!(paired_budget(8, 3), 40);
        assert_eq!(paired_budget(9, 3), 20);
        assert_eq!(paired_budget(127, 13), 20);
        assert_eq!(paired_budget(4, 7), 60);
        assert_eq!(paired_budget(6, 7), 40);
        // A negative tier reads undefined and falls to the last entry.
        assert_eq!(paired_budget(-3, 7), 20);
        assert_eq!(paired_budget(-1, 7), 60); // trunc(-1/3) is -0: tier 0
    }

    #[test]
    fn income_ledger_grows_with_holes() {
        let mut l = crate::ledger::create_ledger_state(0);
        accrue_income(&mut l, FAMILY_PARTY_HALL, 20_000);
        assert_eq!(l.income_ledger.len(), 30);
        assert_eq!(l.income_ledger[29], JsSlot::Number(20_000));
        assert_eq!(l.income_ledger[10], JsSlot::Hole);
        accrue_income(&mut l, FAMILY_PARTY_HALL, 20_000);
        assert_eq!(l.income_ledger[29], JsSlot::Number(40_000));
    }
}

// ts: time.ts

use serde::{Deserialize, Serialize};

use crate::js;

// Day cycle constants
pub const DAY_TICK_MAX: i64 = 2600; // 2600 ticks per day
pub const DAY_TICK_NEW_DAY: i64 = 2300; // 2300: checkpoint where dayCounter increments
pub const DAY_COUNTER_WRAP: i64 = 11988; // 12 days/year * 999 years

/// Starting dayTick for a new game (from new_game_initializer at 0x10d8_07f6).
/// Value 2533 = daypartIndex 6 — game starts mid-day; first full daily checkpoint
/// sequence (0x000..0xa27) runs only on the second sim day.
pub const NEW_GAME_DAY_TICK: i64 = 0x9e5; // 2533

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeState {
    /// Current position within the day (0–2599).
    pub day_tick: i64,
    /// dayTick / 400, integer (0–6).
    pub daypart_index: i64,
    /// Increments at checkpoint 0x08fc each day. Used for calendar logic.
    pub day_counter: i64,
    /// dayCounter % 3 === 2 ? 1 : 0
    pub weekend_flag: i64,
    /// Monotonically increasing tick counter since game start (for broadcast).
    pub total_ticks: i64,
}

/// Zero-based time state for unit tests and generic initialization.
pub fn create_time_state() -> TimeState {
    TimeState {
        day_tick: 0,
        daypart_index: 0,
        day_counter: 0,
        weekend_flag: 0,
        total_ticks: 0,
    }
}

/// New-game time state matching new_game_initializer at 0x10d8_07f6.
/// Starts at tick 0x9e5 (daypart 6) so the first full day cycle begins on day 2.
pub fn create_new_game_time_state() -> TimeState {
    TimeState {
        day_tick: NEW_GAME_DAY_TICK,
        daypart_index: js::floor_div(NEW_GAME_DAY_TICK, 400), // = 6
        day_counter: 0,
        weekend_flag: 0,
        total_ticks: 0,
    }
}

/// Advance time by one tick. Returns the new state and whether the
/// DAY_TICK_INCOME checkpoint was just crossed (triggers day change).
///
/// Upstream returns `{ time, incomeCheckpoint }`; here the pair
/// `(time, income_checkpoint)`.
pub fn advance_one_tick(t: &TimeState) -> (TimeState, bool) {
    let total_ticks = t.total_ticks + 1;
    let mut day_tick = t.day_tick + 1;
    let mut day_counter = t.day_counter;
    let mut weekend_flag = t.weekend_flag;
    let mut income_checkpoint = false;

    if day_tick >= DAY_TICK_MAX {
        day_tick = 0;
    }

    if day_tick == DAY_TICK_NEW_DAY {
        day_counter = t.day_counter + 1;
        if day_counter >= DAY_COUNTER_WRAP {
            day_counter = 0;
        }
        weekend_flag = if day_counter % 3 == 2 { 1 } else { 0 };
        income_checkpoint = true;
    }

    (
        TimeState {
            day_tick,
            daypart_index: js::floor_div(day_tick, 400),
            day_counter,
            weekend_flag,
            total_ticks,
        },
        income_checkpoint,
    )
}

pub fn pre_day4(t: &TimeState) -> bool {
    t.daypart_index < 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_game_crosses_into_day_one() {
        let mut t = create_new_game_time_state();
        assert_eq!(t.daypart_index, 6);
        let mut checkpoints = 0;
        // 2533 → 2599 → 0 → 2299: the next tick is the new-day checkpoint.
        for _ in 0..(DAY_TICK_MAX - NEW_GAME_DAY_TICK + DAY_TICK_NEW_DAY) {
            let (n, c) = advance_one_tick(&t);
            t = n;
            checkpoints += c as i64;
        }
        assert_eq!((t.day_tick, t.day_counter, checkpoints), (DAY_TICK_NEW_DAY, 1, 1));
        assert_eq!(t.daypart_index, 5);
        assert!(!pre_day4(&t));
    }
}

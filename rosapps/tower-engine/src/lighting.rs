//! The tower's lighting as the user sets it (graphics design 7.1, units 11
//! and 12): the quality the presenters draw in, and the clock held at an
//! hour for pictures.
//!
//! The engine's protocol gains two commands:
//!
//!   quality N   0 classic, 1 smooth, 2 rich (the live block's `quality`);
//!               larger numbers are taken as rich
//!   hour H      the clock held at H (0 .. 24, fractions allowed: 20.5 is
//!               20:30) for the pictures; a negative H lets it run again
//!
//! A presenter whose rich program will not build draws classic whatever is
//! asked (design 3.3).  The day's lamps are the simulation's day as it is.
//!
//! For the pictures and the parity test (design 8.5), as dump.request is
//! for the live block: a file light.request beside the engine holds such
//! commands, a line each, with "speed N" (0 paused .. 4, as the Speed
//! choice) so that nothing moves between a dump and a screenshot, and
//! "fixture N" (the Tower menu's test towers, from 0) for a tower to look
//! at where no one can click the menu; the engine carries them out and
//! removes the file.

use tower_surface::block::Tower;

use crate::publish::day_tick_to_hour;

/// the qualities' names, by number, for a choice in the panes
#[allow(dead_code)]
pub const QUALITIES: [&str; 3] = ["Classic", "Smooth", "Rich"];

/// the commands in light.request beside the engine, if there is one (it is
/// removed): (command, number)
pub fn asked(dir: &std::path::Path) -> Vec<(String, f64)> {
    let req = dir.join("light.request");
    let Ok(text) = std::fs::read_to_string(&req) else { return Vec::new() };
    let _ = std::fs::remove_file(&req);
    text.lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let c = w.next()?;
            let n = w.next()?.parse::<f64>().ok()?;
            matches!(c, "quality" | "hour" | "speed" | "fixture").then(|| (c.to_string(), n))
        })
        .collect()
}

/// "quality N": the quality to publish
pub fn quality(n: f64) -> u8 {
    if n.is_nan() || n <= 0.0 {
        0
    } else {
        (n as u32).min(2) as u8
    }
}

/// the quality a name in QUALITIES stands for
#[allow(dead_code)]
pub fn quality_named(name: &str) -> Option<u8> {
    QUALITIES.iter().position(|q| q.eq_ignore_ascii_case(name.trim())).map(|i| i as u8)
}

/// "hour H": the clock held there, in 1/64 hour, or None to let it run
pub fn hold(h: f64) -> Option<u16> {
    if h.is_nan() || h < 0.0 {
        None
    } else {
        Some(((h * 64.0).round() as u32 % 1536) as u16)
    }
}

/// the day tick nearest an hour (1/64 h): what the simulation's clock is set
/// to while it is held there, so that the rooms open and close as at that
/// hour
pub fn day_tick_at(h64: u16) -> i64 {
    let want = h64 as f64 / 64.0;
    let dist = |t: i64| {
        let d = (day_tick_to_hour(t) - want).rem_euclid(24.0);
        d.min(24.0 - d)
    };
    (0..2600).min_by(|a, b| dist(*a).total_cmp(&dist(*b))).unwrap_or(400)
}

/// the header's hour set to the held hour exactly (a day tick only comes
/// near it), after a publish
pub fn held_hour(t: &mut Tower, hold: Option<u16>) {
    if let Some(h) = hold {
        t.hour = h % 1536;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commands() {
        assert_eq!(quality(0.0), 0);
        assert_eq!(quality(1.0), 1);
        assert_eq!(quality(2.0), 2);
        assert_eq!(quality(7.0), 2);
        assert_eq!(quality(-1.0), 0);
        assert_eq!(quality_named("rich"), Some(2));
        assert_eq!(quality_named("Smooth"), Some(1));
        assert_eq!(hold(20.5), Some(1312));
        assert_eq!(hold(-1.0), None);
        assert_eq!(hold(24.0), Some(0));
    }

    #[test]
    fn held_hours_are_the_menus() {
        // the View menu's noon and midnight, as main.rs held them before
        assert_eq!(day_tick_at(12 * 64), 400);
        assert_eq!(day_tick_at(0), 2300);
        for h in [6.0, 12.0, 20.5, 2.0] {
            let t = day_tick_at(hold(h).unwrap());
            assert!((day_tick_to_hour(t) - h).abs() < 0.05, "{h}: tick {t}");
        }
    }
}

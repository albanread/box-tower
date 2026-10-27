//! The signs' sprite numbers.  Sprites 1 to 255 are the signs
//! (art/src/signs/<lang>, packed for one language), so that a room
//! record's sign byte names its sprite directly.  The order is fixed here,
//! the same in every language (the files' names are), and only ever grows
//! at the end: an engine and an art block made at different times agree.
//!
//! A name is a sign's file name without `.svg`; a sign drawn in frames
//! (`evacuate.0`, `evacuate.1`) is one sprite with its frames side by side.

/// the banners, by kind and by the room's width in cells
pub const BANNER_KINDS: [&str; 4] = ["for-rent", "for-sale", "sold", "let"];
pub const BANNER_WIDTHS: [u32; 6] = [4, 6, 9, 10, 12, 16];

/// every other sign but the floor plates, in their fixed order
const OTHERS: [&str; 40] = [
    "open", "closed", "open-neon", "vacancy", "no-vacancy", "full", "exit", "hotel", "cinema",
    "now-showing", "parking", "metro", "ambulance", "police", "fire", "evacuate", "lobby",
    "reception", "offices", "party-hall", "clinic", "security", "staff-only", "no-entry",
    "car-lift", "sale", "out-of-order", "shop", "cafe", "restaurant",
    // spare places, so that a sign added later keeps the floor plates' numbers
    "", "", "", "", "", "", "", "", "", "",
];

/// The signs in sprite order: index 0 is sprite 1.  An empty name is a
/// place kept free.
pub fn names() -> Vec<String> {
    let mut v = Vec::new();
    for k in BANNER_KINDS {
        for w in BANNER_WIDTHS {
            v.push(format!("{k}-{w}"));
        }
    }
    v.extend(OTHERS.iter().map(|s| s.to_string()));
    for b in (1..=10).rev() {
        v.push(format!("floor-b{b}"));
    }
    v.push("floor-g".into());
    for f in 1..=109 {
        v.push(format!("floor-{f}"));
    }
    assert!(v.len() <= 255, "the signs fit sprites 1-255");
    v
}

/// a sign's sprite, 1-255, or None
pub fn sprite(name: &str) -> Option<u32> {
    if name.is_empty() {
        return None;
    }
    names().iter().position(|n| n == name).map(|i| i as u32 + 1)
}

/// the banner for a room `cells` wide: the widest drawn that fits, else the
/// narrowest (rooms are 4 to 31 cells; the banners are drawn to the slot
/// widths of the rooms that can be let or sold)
pub fn banner(kind: &str, cells: u32) -> Option<u32> {
    let w = BANNER_WIDTHS.iter().rev().find(|&&w| w <= cells).copied().unwrap_or(BANNER_WIDTHS[0]);
    sprite(&format!("{kind}-{w}"))
}

/// the floor plate's sign name for a sim floor index (10 is the ground)
pub fn floor_plate(floor: i64) -> String {
    match floor - 10 {
        0 => "floor-g".into(),
        n if n < 0 => format!("floor-b{}", -n),
        n => format!("floor-{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers_are_fixed() {
        let n = names();
        assert_eq!(n.len(), 24 + 40 + 120);
        assert_eq!(sprite("for-rent-4"), Some(1));
        assert_eq!(sprite("let-16"), Some(24));
        assert_eq!(sprite("open"), Some(25));
        assert_eq!(sprite("floor-b10"), Some(65));
        assert_eq!(sprite("floor-g"), Some(75));
        assert_eq!(sprite("floor-109"), Some(184));
        assert_eq!(banner("for-rent", 9), sprite("for-rent-9"));
        assert_eq!(banner("for-sale", 31), sprite("for-sale-16"));
        assert_eq!(floor_plate(0), "floor-b10");
        assert_eq!(floor_plate(10), "floor-g");
    }
}

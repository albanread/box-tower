//! The twin's proofs (design 11, unit 3): three of upstream's trace-fixture
//! towers, built as the simulation's own tests build them, published by the
//! seed of unit 4 (tests/seed) and drawn in classic at midnight and noon of
//! their second day; the same bytes on two runs; at 4x a room's pixels are
//! its picture's texels exactly.
//!
//! The pictures go to $TOWER_TWIN_OUT (else this crate's target directory):
//! for each tower and hour, the tower at 1:1 cropped to the building, and a
//! 4x piece of its lower floors.  Needs the rendered art and upstream's
//! fixtures ($TOWER_UPSTREAM, as sim/tests); without them the tests say so
//! and pass.

#[path = "../../sim/tests/support/mod.rs"]
mod support;
mod seed;

use std::path::PathBuf;

use support::*;
use tower_surface::block::*;
use tower_surface::pack::{self, Options, Packed};
use tower_surface::twin::{self, Textures, View};

fn art() -> Option<Packed> {
    let a = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../art");
    if !a.join("out/4/rooms").is_dir() {
        eprintln!("no rendered art: run python3 art/tools/render.py");
        return None;
    }
    Some(pack::pack(&Options::new(a, 4, "en")).unwrap())
}

fn have_fixtures() -> bool {
    let ok = fixture_dir().join("build_mixed.json").exists();
    if !ok {
        eprintln!("no upstream fixtures at {} (TOWER_UPSTREAM)", fixture_dir().display());
    }
    ok
}

fn out_dir() -> PathBuf {
    let d = std::env::var_os("TOWER_TWIN_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("twin"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn draw(t: &Tower, p: &Packed, tex: &Textures, v: &View) -> Vec<u8> {
    let a = twin::render(t, &p.block, tex, v).unwrap();
    let b = twin::render(t, &p.block, tex, v).unwrap();
    assert!(a == b, "two renders differ");
    a
}

fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

#[test]
fn three_towers_noon_and_midnight() {
    let Some(p) = art() else { return };
    if !have_fixtures() {
        return;
    }
    let dir = out_dir();
    let a = p.head();
    for name in ["mixed", "dense_hotel", "sky_office"] {
        let (spec, trace, _) = load_fixture(name);
        let mut sim = prepare_from_trace(&spec, &trace);
        let mut sky_noon = [0u8; 4];
        for (when, day, tick) in [("midnight", 2, 2300), ("noon", 2, 400)] {
            advance_to(&mut sim, trace_tick_to_total_ticks(day, tick));
            let t = seed::publish(&sim, &p);
            assert_eq!(t.hour, if when == "noon" { 12 * 64 } else { 0 }, "{name} {when}");
            let census = seed::census(&t, &p);
            assert!(census.contains_key("rooms/lobby"), "{name}: no lobby");
            assert!(!census.keys().any(|k| k.starts_with('#')), "{name}: a room without a picture: {census:?}");
            // every room's cells name it, and nothing else does
            for i in 1..t.nrooms as usize {
                let r = t.room[i];
                for y in r.ytop()..r.ytop() + r.height() {
                    for x in r.x0()..r.x0() + r.width() {
                        assert_eq!(t.cell[y as usize][x as usize].room as usize, i, "{name}: room {i} at {x},{y}");
                    }
                }
            }
            let tex = Textures::new(&t, &p.block);
            let (x0, y0, x1, y1) = seed::bounds(&t);
            let (cx0, cy0) = (x0.saturating_sub(10), y0.saturating_sub(3));
            let (cx1, cy1) = ((x1 + 10).min(GRID_W as u32), GRID_H as u32);

            // the tower at 1:1, the building and what is round it
            let v = View::whole(1.0).cells(cx0, cy0, cx1, cy1);
            let img = draw(&t, &p, &tex, &v);
            pack::write_png(&dir.join(format!("{name}-{when}-1x.png")), v.clip[2] as usize, v.clip[3] as usize, &img)
                .unwrap();
            // the sky's top left: upstream's gradient, by day and by night
            let sky = px(&img, v.clip[2], 0, 0);
            if when == "noon" {
                sky_noon = sky;
            } else {
                assert!(sky[2] < 80, "{name}: midnight sky {sky:?}");
            }

            // 4x, the lowest floors above the ground and the first below
            let v4 = View::whole(4.0).cells(x0.saturating_sub(2), GROUND as u32 - 4, (x0 + 60).min(GRID_W as u32), GROUND as u32 + 2);
            let img4 = draw(&t, &p, &tex, &v4);
            pack::write_png(&dir.join(format!("{name}-{when}-4x.png")), v4.clip[2] as usize, v4.clip[3] as usize, &img4)
                .unwrap();
            eprintln!("{name} {when}: {} rooms, cells {x0},{y0} to {x1},{y1}; {census:?}", t.nrooms - 1);
        }
        assert!(sky_noon[2] > 150, "{name}: noon sky {sky_noon:?}");
        let _ = &a;
    }
}

/// classic at 4x: a room's pixels are its picture's texels, exactly (the
/// parity test's strictest case, design 8.5)
#[test]
fn classic_at_4x_is_texel_exact() {
    let Some(p) = art() else { return };
    let a = p.head();
    let mut t = Tower::new(4);
    t.hour = 12 * 64;
    // an office alone on the ground floor
    let office = p.art_of("rooms/office.2");
    let mut r = Room::place(100, 109, 9, 1);
    r.set_art(office);
    t.room[1] = r;
    for x in 100..109 {
        t.cell[109][x].room = 1;
    }
    t.nrooms = 2;
    let tex = Textures::new(&t, &p.block);
    let v = View::whole(4.0).cells(100, 109, 109, 110);
    let img = twin::render(&t, &p.block, &tex, &v).unwrap();
    let e = a.art[office as usize];
    let (ax, ay) = ((e[0] & 0xffff) as usize, (e[0] >> 16) as usize);
    let w = a.atlas_w as usize;
    let mut checked = 0;
    // inside the room less its gap (one unit, four pixels, right and bottom)
    for y in 0..60u32 {
        for x in 0..(9 * 16 - 4) as u32 {
            let i = ((ay + y as usize) * w + ax + x as usize) * 4;
            let texel = &p.block[a.at_albedo() + i..a.at_albedo() + i + 3];
            assert_eq!(&px(&img, v.clip[2], x, y)[..3], texel, "pixel {x},{y}");
            checked += 1;
        }
    }
    assert_eq!(checked, 60 * 140);
    // the gap shows the sky behind
    let gap = px(&img, v.clip[2], 9 * 16 - 2, 30);
    assert!(gap[2] > gap[0], "the gap is sky: {gap:?}");

    // vacant: the room names its banner; greyed when stuck
    t.room[1].set_sign(tower_surface::signs::banner("for-rent", 9).unwrap());
    t.room[1].set_state(S_VACANT | S_STUCK);
    let tex = Textures::new(&t, &p.block); // the rooms section, uploaded again
    let img2 = twin::render(&t, &p.block, &tex, &v).unwrap();
    pack::write_png(&out_dir().join("office-for-rent-4x.png"), v.clip[2] as usize, v.clip[3] as usize, &img2).unwrap();
    let changed = img.chunks_exact(4).zip(img2.chunks_exact(4)).filter(|(a, b)| a != b).count();
    assert!(changed > 1000, "the banner and the grey change the picture ({changed} pixels)");
    let g = px(&img2, v.clip[2], 2, 58);
    assert!(g[0] == g[1] && g[1] == g[2], "stuck is grey: {g:?}");
}

/// smooth and rich (units 11 and 12): an office lit at night is brighter
/// than the same office dark; its window shows the sky (dawn's warm
/// horizon, night's blue); the sky at night is the ramp's; two renders give
/// the same bytes
#[test]
fn smooth_and_rich_light_the_rooms() {
    let Some(p) = art() else { return };
    let mut t = Tower::new(4);
    // office.0 (it has a window) alone on the ground floor
    let office = p.art_of("rooms/office.0");
    let mut r = Room::place(100, 109, 9, 1);
    r.set_art(office);
    r.set_profile(P_OFFICE);
    t.room[1] = r;
    for x in 100..109 {
        t.cell[109][x].room = 1;
    }
    t.nrooms = 2;
    let v = View::whole(4.0).cells(98, 105, 111, 110);
    let mean = |img: &[u8]| img.chunks_exact(4).map(|c| c[0] as u64 + c[1] as u64 + c[2] as u64).sum::<u64>() / (img.len() as u64 / 4);
    let room = View::whole(4.0).cells(100, 109, 109, 110);
    for q in [1u8, 2] {
        t.quality = q;
        t.hour = 0;
        t.room[1].set_state(S_LIT);
        let tex = Textures::new(&t, &p.block);
        let lit = twin::render(&t, &p.block, &tex, &room).unwrap();
        assert!(lit == twin::render(&t, &p.block, &tex, &room).unwrap(), "q{q}: two renders differ");
        t.room[1].set_state(S_DARK);
        let tex = Textures::new(&t, &p.block);
        let dark = twin::render(&t, &p.block, &tex, &room).unwrap();
        assert!(mean(&lit) > mean(&dark) * 3, "q{q}: lit {} dark {}", mean(&lit), mean(&dark));
        // by day the lamps make little difference
        t.hour = 12 * 64;
        let day_dark = twin::render(&t, &p.block, &tex, &room).unwrap();
        assert!(mean(&day_dark) > mean(&lit) / 2, "q{q}: by day the room is lit by the day");
        for (hour, name) in [(6 * 64, "dawn"), (12 * 64, "noon"), (1312, "dusk"), (0, "midnight")] {
            t.hour = hour;
            t.room[1].set_state(0);
            let tex = Textures::new(&t, &p.block);
            let img = twin::render(&t, &p.block, &tex, &v).unwrap();
            pack::write_png(&out_dir().join(format!("office-q{q}-{name}-4x.png")), v.clip[2] as usize, v.clip[3] as usize, &img)
                .unwrap();
        }
    }
    // the window's panes show the sky: at dawn warmer (red over blue) than at noon
    let a = p.head();
    let e = a.art[office as usize];
    assert_ne!(e[3] & A_AUX, 0);
    let aux = &p.block[a.at_aux()..a.at_sprites()];
    let (ax, ay) = ((e[0] & 0xffff) as usize, (e[0] >> 16) as usize);
    let pane = (0..64usize)
        .flat_map(|y| (0..144usize).map(move |x| (x, y)))
        .find(|&(x, y)| aux[((ay + y) * a.atlas_w as usize + ax + x) * 2] == 2 && x > 4 && y > 4)
        .expect("a pane");
    t.quality = 1;
    t.room[1].set_state(S_DARK);
    let tex = Textures::new(&t, &p.block);
    let at = |t: &mut Tower, h: u16| {
        t.hour = h;
        let img = twin::render(t, &p.block, &tex, &room).unwrap();
        px(&img, room.clip[2], pane.0 as u32 + 1, pane.1 as u32 + 1)
    };
    let (dawn, noon, night) = (at(&mut t, 6 * 64), at(&mut t, 12 * 64), at(&mut t, 0));
    assert!(dawn[0] as i32 - dawn[2] as i32 > noon[0] as i32 - noon[2] as i32, "dawn {dawn:?} noon {noon:?}");
    assert!(night[2] > night[0] && night[2] < 120, "night {night:?}");
}

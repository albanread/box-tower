//! tower.h and block.rs agree: every size and offset the same, and a block
//! written in Rust reads back, field for field, in C.
//!
//! The C side is tests/c/tower_dump.c, built here with the system's C
//! compiler (`$CC`, else `cc`) against rosgd/include.

use std::fmt::Write as _;
use std::mem::{offset_of, size_of};
use std::path::{Path, PathBuf};
use std::process::Command;

use tower_surface::block::*;

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("layout");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// tower_dump, built once for the test run (the tests run in parallel)
fn reader() -> PathBuf {
    static EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    EXE.get_or_init(build_reader).clone()
}

fn build_reader() -> PathBuf {
    let exe = scratch().join("tower_dump");
    let include = here().join("../../../include");
    let src = here().join("tests/c/tower_dump.c");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let out = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O1", "-I"])
        .arg(&include)
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap_or_else(|e| panic!("{cc}: {e}"));
    assert!(out.status.success(), "{cc} failed:\n{}", String::from_utf8_lossy(&out.stderr));
    exe
}

fn run(exe: &Path, args: &[&str]) -> String {
    let out = Command::new(exe).args(args).output().unwrap();
    assert!(out.status.success(), "tower_dump {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn sizes_and_offsets() {
    assert_eq!(size_of::<Cell>(), 8);
    assert_eq!(size_of::<Room>(), 16);
    assert_eq!(size_of::<Fixture>(), 16);
    assert_eq!(size_of::<Mover>(), 8);
    assert_eq!(size_of::<Tower>(), 712_960);
    assert_eq!(size_of::<TowerArt>(), 34_368);
    assert_eq!(size_of::<SurfTower>(), 32);
    assert_eq!(offset_of!(Tower, outline_rgba), 48);
    assert_eq!(offset_of!(Tower, cell), 192);
    assert_eq!(offset_of!(Tower, room), 368_832);
    assert_eq!(offset_of!(Tower, fix), 630_976);
    assert_eq!(offset_of!(Tower, stops), 639_168);
    assert_eq!(offset_of!(Tower, pal), 647_360);
    assert_eq!(offset_of!(Tower, mover), 647_424);
    assert_eq!(offset_of!(TowerArt, art), 64);
    assert_eq!(offset_of!(TowerArt, spr), 16_448);
    assert_eq!(offset_of!(TowerArt, ramp), 32_832);
    assert_eq!(offset_of!(TowerArt, prof), 33_600);
}

macro_rules! off {
    ($s:expr, $t:ident, $c:literal, $($f:ident),+) => {
        $( writeln!($s, "{}.{} {}", $c, stringify!($f), offset_of!($t, $f)).unwrap(); )+
    };
}

/// the C compiler's layout, line for line as tower_dump prints it
#[test]
fn layout_matches_c() {
    let mut s = String::new();
    writeln!(s, "sizeof ros_tower {}", size_of::<Tower>()).unwrap();
    writeln!(s, "sizeof ros_tower_art {}", size_of::<TowerArt>()).unwrap();
    writeln!(s, "sizeof ros_surf_tower {}", size_of::<SurfTower>()).unwrap();
    writeln!(s, "sizeof ros_tower_cell {}", size_of::<Cell>()).unwrap();
    writeln!(s, "sizeof ros_tower_room {}", size_of::<Room>()).unwrap();
    writeln!(s, "sizeof ros_tower_fixture {}", size_of::<Fixture>()).unwrap();
    writeln!(s, "sizeof ros_tower_mover {}", size_of::<Mover>()).unwrap();
    off!(s, Tower, "ros_tower", magic, version, frame, art, grid_w, grid_h, ground_row, scale, nrooms,
         nfixtures, nmovers, quality, rate, view, flags, hour, day, outline, reserved0, outline_rgba,
         back_rgba, gen, room_gen, reserved, cell, room, fix, stops, pal, mover);
    off!(s, Cell, "ros_tower_cell", room, back, front, flags);
    off!(s, Mover, "ros_tower_mover", x, y, spr, param);
    off!(s, TowerArt, "ros_tower_art", magic, version, gen, size, scale, atlas_w, atlas_h, spr_w, spr_h,
         nart, nsprites, emit_shift, flags, lang, reserved, art, spr, ramp, prof);
    off!(s, SurfTower, "ros_surf_tower", backing, offset, size, frame, art_backing, art_offset, art_size,
         art_gen);
    assert_eq!(run(&reader(), &["layout"]), s);
}

/// a value for field `k` of element `i`: distinct, never zero, using the
/// field's whole width
fn v32(i: usize, k: usize) -> u32 {
    hash((i as u32).wrapping_mul(31).wrapping_add(k as u32 + 1)) | 1
}
fn v16(i: usize, k: usize) -> u16 {
    (v32(i, k) >> 7) as u16 | 1
}

#[test]
fn rust_writes_c_reads() {
    let exe = reader();
    let dir = scratch();

    // ---- the live block: every field set, then printed as C prints it
    let mut t = Tower::zeroed();
    t.magic = TOWER_MAGIC;
    t.version = TOWER_VERSION;
    t.frame = 0xdead_beef;
    t.art = 77;
    t.grid_w = GRID_W as u16;
    t.grid_h = GRID_H as u16;
    t.ground_row = GROUND as u16;
    t.scale = 4;
    t.nrooms = 16_000;
    t.nfixtures = 511;
    t.nmovers = 8191;
    t.quality = 2;
    t.rate = 30;
    t.view = 1;
    t.flags = F_WEEKEND;
    t.hour = 1535;
    t.day = 65_000;
    t.outline = [-3, 7, 300, -32768];
    t.reserved0 = 0xabcd;
    t.outline_rgba = 0xff00_ffff;
    t.back_rgba = 0x8040_2010;
    for i in 0..8 {
        t.gen[i] = v32(i, 1);
    }
    for i in 0..16 {
        t.room_gen[i] = v32(i, 2);
    }
    for i in 0..10 {
        t.reserved[i] = v32(i, 3);
    }
    for y in 0..GRID_H {
        for x in 0..STRIDE {
            let i = y * STRIDE + x;
            t.cell[y][x] = Cell { room: v16(i, 0), back: v16(i, 1), front: v16(i, 2), flags: v16(i, 3) };
        }
    }
    for i in 0..ROOMS {
        t.room[i].w = [v32(i, 4), v32(i, 5), v32(i, 6), v32(i, 7)];
    }
    for i in 0..FIXTURES {
        t.fix[i].w = [v32(i, 8), v32(i, 9), v32(i, 10), v32(i, 11)];
    }
    for s in 0..SHAFTS {
        for r in 0..ROWS {
            t.stops[s][r] = (v32(s * ROWS + r, 12) >> 13) as u8;
        }
    }
    for i in 0..16 {
        t.pal[i] = v32(i, 13);
    }
    for i in 0..MOVERS {
        t.mover[i] = Mover { x: v16(i, 14), y: v16(i, 15), spr: v16(i, 16), param: v16(i, 17) };
    }
    let live = dir.join("live.bin");
    std::fs::write(&live, t.as_bytes()).unwrap();

    let mut s = String::new();
    writeln!(s, "magic {}\nversion {}\nframe {}\nart {}", t.magic, t.version, t.frame, t.art).unwrap();
    writeln!(s, "grid {} {}\nground_row {}\nscale {}", t.grid_w, t.grid_h, t.ground_row, t.scale).unwrap();
    writeln!(s, "counts {} {} {}", t.nrooms, t.nfixtures, t.nmovers).unwrap();
    writeln!(s, "bytes {} {} {} {}", t.quality, t.rate, t.view, t.flags).unwrap();
    writeln!(s, "hour {}\nday {}", t.hour, t.day).unwrap();
    writeln!(s, "outline {} {} {} {}", t.outline[0], t.outline[1], t.outline[2], t.outline[3]).unwrap();
    writeln!(s, "reserved0 {}\noutline_rgba {}\nback_rgba {}", t.reserved0, t.outline_rgba, t.back_rgba).unwrap();
    for i in 0..8 {
        writeln!(s, "gen[{i}] {}", t.gen[i]).unwrap();
    }
    for i in 0..16 {
        writeln!(s, "room_gen[{i}] {}", t.room_gen[i]).unwrap();
    }
    for i in 0..10 {
        writeln!(s, "reserved[{i}] {}", t.reserved[i]).unwrap();
    }
    for y in 0..GRID_H {
        for x in 0..STRIDE {
            let c = t.cell[y][x];
            writeln!(s, "cell[{y}][{x}] {} {} {} {}", c.room, c.back, c.front, c.flags).unwrap();
        }
    }
    for (i, r) in t.room.iter().enumerate() {
        writeln!(s, "room[{i}] {} {} {} {}", r.w[0], r.w[1], r.w[2], r.w[3]).unwrap();
    }
    for (i, r) in t.fix.iter().enumerate() {
        writeln!(s, "fix[{i}] {} {} {} {}", r.w[0], r.w[1], r.w[2], r.w[3]).unwrap();
    }
    for sh in 0..SHAFTS {
        for r in 0..ROWS {
            writeln!(s, "stops[{sh}][{r}] {}", t.stops[sh][r]).unwrap();
        }
    }
    for i in 0..16 {
        writeln!(s, "pal[{i}] {}", t.pal[i]).unwrap();
    }
    for (i, m) in t.mover.iter().enumerate() {
        writeln!(s, "mover[{i}] {} {} {} {}", m.x, m.y, m.spr, m.param).unwrap();
    }
    let c = run(&exe, &["live", live.to_str().unwrap()]);
    for (n, (a, b)) in c.lines().zip(s.lines()).enumerate() {
        assert_eq!(a, b, "live block, line {n}: C read {a:?}, Rust wrote {b:?}");
    }
    assert_eq!(c.lines().count(), s.lines().count());

    // ---- the art block's head and tables
    let mut a = TowerArt::zeroed();
    a.magic = TOWER_ART_MAGIC;
    a.version = TOWER_VERSION;
    a.gen = 5;
    a.size = 123_456_789;
    a.scale = 8;
    a.atlas_w = 8192;
    a.atlas_h = 2056;
    a.spr_w = 2048;
    a.spr_h = 1032;
    a.nart = 1000;
    a.nsprites = 900;
    a.emit_shift = 1;
    a.flags = TWA_F_AUX;
    a.lang = u16::from_le_bytes(*b"fr");
    for i in 0..7 {
        a.reserved[i] = v32(i, 20);
    }
    for i in 0..ART {
        a.art[i] = [v32(i, 21), v32(i, 22), v32(i, 23), v32(i, 24)];
    }
    for i in 0..SPRITES {
        a.spr[i] = [v32(i, 25), v32(i, 26), v32(i, 27), v32(i, 28)];
    }
    for r in 0..4 {
        for c in 0..RAMP {
            let w = v32(r * RAMP + c, 29).to_le_bytes();
            a.ramp[r][c] = w;
        }
    }
    for d in 0..2 {
        for p in 0..PROFILES {
            for h in 0..24 {
                a.prof[d][p][h] = (v32((d * PROFILES + p) * 24 + h, 30) >> 11) as u8;
            }
        }
    }
    let artf = dir.join("art.bin");
    std::fs::write(&artf, a.as_bytes()).unwrap();
    let mut s = String::new();
    writeln!(s, "magic {}\nversion {}\ngen {}\nsize {}", a.magic, a.version, a.gen, a.size).unwrap();
    writeln!(s, "scale {}\natlas {} {}\nspr {} {}", a.scale, a.atlas_w, a.atlas_h, a.spr_w, a.spr_h).unwrap();
    writeln!(s, "counts {} {}\nemit_shift {}\nflags {}\nlang {}", a.nart, a.nsprites, a.emit_shift, a.flags, a.lang)
        .unwrap();
    for i in 0..7 {
        writeln!(s, "reserved[{i}] {}", a.reserved[i]).unwrap();
    }
    for (i, e) in a.art.iter().enumerate() {
        writeln!(s, "art[{i}] {} {} {} {}", e[0], e[1], e[2], e[3]).unwrap();
    }
    for (i, e) in a.spr.iter().enumerate() {
        writeln!(s, "spr[{i}] {} {} {} {}", e[0], e[1], e[2], e[3]).unwrap();
    }
    for r in 0..4 {
        for c in 0..RAMP {
            let e = a.ramp[r][c];
            writeln!(s, "ramp[{r}][{c}] {} {} {} {}", e[0], e[1], e[2], e[3]).unwrap();
        }
    }
    for d in 0..2 {
        for p in 0..PROFILES {
            for h in 0..24 {
                writeln!(s, "prof[{d}][{p}][{h}] {}", a.prof[d][p][h]).unwrap();
            }
        }
    }
    writeln!(
        s,
        "at {} {} {} {} {} {}",
        a.at_albedo(),
        a.at_emis(),
        a.at_aux(),
        a.at_sprites(),
        a.at_spremis(),
        a.total_size()
    )
    .unwrap();
    assert_eq!(run(&exe, &["art", artf.to_str().unwrap()]), s);

    // the planes' offsets without the aux picture, too
    a.flags = 0;
    std::fs::write(&artf, a.as_bytes()).unwrap();
    let c = run(&exe, &["art", artf.to_str().unwrap()]);
    let last = c.lines().last().unwrap().to_string();
    assert_eq!(
        last,
        format!("at {} {} {} {} {} {}", a.at_albedo(), a.at_emis(), a.at_aux(), a.at_sprites(), a.at_spremis(), a.total_size())
    );

    // ---- the surface's 32 bytes
    let sf = SurfTower {
        backing: 1,
        offset: 0x0100_0000,
        size: 712_960,
        frame: 42,
        art_backing: 2,
        art_offset: 0x0140_0000,
        art_size: 40_000_000,
        art_gen: 3,
    };
    let surf = dir.join("surf.bin");
    std::fs::write(&surf, sf.as_bytes()).unwrap();
    assert_eq!(
        run(&exe, &["surf", surf.to_str().unwrap()]),
        format!("live 1 {} 712960 42\nart 2 {} 40000000 3\n", 0x0100_0000, 0x0140_0000)
    );
}

#[test]
fn room_fields_pack() {
    let mut r = Room::place(374, 119, 375, 7);
    r.set_art(1023);
    r.set_profile(11);
    r.set_eval(3);
    r.set_value(200);
    r.set_state(S_VACANT | S_SALE | S_DARK);
    r.set_occupancy(255);
    r.set_sign(255);
    assert_eq!((r.x0(), r.ytop(), r.width(), r.height()), (374, 119, 375, 7));
    assert_eq!(r.w[1], 1023 | 11 << 16 | 3 << 20 | 200 << 24);
    assert_eq!(r.w[2], (S_VACANT | S_SALE | S_DARK) | 255 << 16 | 255 << 24);
}

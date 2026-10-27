//! twincheck: a presenter's picture of the tower against the twin's (the
//! parity test, graphics design 8.5).
//!
//!   cargo run --release --example twincheck -- LIVE ART SHOT.png BX BY BW BH X0 Y0 X1 Y1 [DIFF.png] [TWIN.png]
//!
//! LIVE is a live block as the engine published it (its dump-live.bin),
//! ART the art block (towerpack's; !Tower carries it zlib-compressed), SHOT a
//! screenshot of the screen it was drawn on, BX BY BW BH the tower's icon's
//! box on that screen (target pixels: the Mac's `info box-v2`), and X0 Y0
//! X1 Y1 the piece of the screen to compare (inside the icon, clear of the
//! pointer).  The twin draws the same piece; the two are compared channel
//! by channel.  Classic at 4x is to match exactly; otherwise the design's
//! tolerance (at most 2/255 on 99.9 per cent of pixels, 8/255 on all) is
//! reported.  DIFF gets the differences, magnified; TWIN the twin's piece.
//!
//! What moves with the presenter's own clock (its seconds, which the twin
//! cannot know: cockroaches in an infested room, a picture's frames run by
//! the clock, fire) is left out of the comparison and counted apart.

use std::path::Path;
use std::process::ExitCode;

use tower_surface::block::*;
use tower_surface::pack::{read_png, write_png};
use tower_surface::twin::{self, Textures, View};

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 11 {
        eprintln!("twincheck LIVE ART SHOT.png BX BY BW BH X0 Y0 X1 Y1 [DIFF.png] [TWIN.png]");
        return ExitCode::from(2);
    }
    let n = |i: usize| a[i].parse::<i64>().expect("a number");
    let live = std::fs::read(&a[0]).expect("the live block");
    let art = std::fs::read(&a[1]).expect("the art block");
    let shot = read_png(Path::new(&a[2])).expect("the shot");
    let t = Tower::from_bytes(&live);
    let (bx, by, bw, bh) = (n(3) as f32, n(4) as f32, n(5) as f32, n(6) as f32);
    let (x0, y0, x1, y1) = (n(7) as u32, n(8) as u32, n(9) as u32, n(10) as u32);
    let tex = Textures::new(&t, &art);
    let v = View { box_: [bx, by, bw, bh], target: [shot.w as u32, shot.h as u32], clip: [x0, y0, x1 - x0, y1 - y0], secs: 0.0 };
    let img = match twin::render(&t, &art, &tex, &v) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("twincheck: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    // the cells whose picture moves with the clock
    let ah = TowerArt::from_bytes(&art);
    let anim = |e: u16| e != 0 && (ah.art.get(e as usize).map(|r| r[3] & (A_ANIM | A_FIRE) != 0).unwrap_or(false));
    let moving = |px: u32, py: u32| -> bool {
        let mx = (px as f32 + 0.5 - bx) / bw * (GRID_W * UNIT_W) as f32;
        let my = (py as f32 + 0.5 - by) / bh * (GRID_H * UNIT_H) as f32;
        let (cx, cy) = ((mx / UNIT_W as f32).floor() as i64, (my / UNIT_H as f32).floor() as i64);
        if cx < 0 || cy < 0 || cx >= GRID_W as i64 || cy >= GRID_H as i64 {
            return false;
        }
        let c = t.cell[cy as usize][cx as usize];
        let r = t.room[c.room as usize];
        (c.room != 0 && (r.w[2] & (S_INFESTED | S_SHOWING) != 0 || anim((r.w[1] & 0xffff) as u16))) || anim(c.back) || anim(c.front)
    };
    // and the movers whose frames run by the clock (a vehicle's wheels, a beacon)
    let k = [bw / (GRID_W * UNIT_W) as f32, bh / (GRID_H * UNIT_H) as f32];
    let anim_quads: Vec<[f32; 4]> = t.mover[..(t.nmovers as usize).min(MOVERS)]
        .iter()
        .filter_map(|m| {
            let sp = ah.spr.get((m.spr & M_SPRITE) as usize)?;
            if sp[3] & A_ANIM == 0 {
                return None;
            }
            let (w, h) = ((sp[2] & 0xffff) as f32 * 0.25, (sp[2] >> 16) as f32 * 0.25);
            let (fx, fy) = (m.x as f32 * 0.25, m.y as f32 * 0.25);
            Some([bx + (fx - w / 2.0) * k[0], by + (fy - h) * k[1], bx + (fx + w / 2.0) * k[0], by + fy * k[1]])
        })
        .collect();
    let moving_mover = |px: u32, py: u32| {
        let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
        anim_quads.iter().any(|q| cx >= q[0] && cx < q[2] && cy >= q[1] && cy < q[3])
    };
    let mut skipped = 0usize;
    let mut diff = vec![0u8; w * h * 4];
    let (mut worst, mut over2, mut differ) = (0u8, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let s = ((y0 as usize + y) * shot.w + x0 as usize + x) * 4;
            let o = (y * w + x) * 4;
            if moving(x0 + x as u32, y0 + y as u32) || moving_mover(x0 + x as u32, y0 + y as u32) {
                skipped += 1;
                diff[o..o + 4].copy_from_slice(&[0, 0, 96, 255]);
                continue;
            }
            let mut m = 0u8;
            for c in 0..3 {
                m = m.max(shot.px[s + c].abs_diff(img[o + c]));
            }
            worst = worst.max(m);
            differ += (m > 0) as usize;
            over2 += (m > 2) as usize;
            let k = (m as u32 * 32).min(255) as u8;
            diff[o..o + 4].copy_from_slice(&[k, k, k, 255]);
        }
    }
    let total = w * h - skipped;
    if skipped > 0 {
        println!("twincheck: {skipped} pixels left out: they move with the presenter's clock (cockroaches, frames, fire)");
    }
    println!(
        "twincheck: {total} pixels, {differ} differ ({:.4}%), {over2} by more than 2/255 ({:.4}%), worst {worst}/255",
        100.0 * differ as f64 / total as f64,
        100.0 * over2 as f64 / total as f64
    );
    if let Some(p) = a.get(11) {
        write_png(Path::new(p), w, h, &diff).unwrap();
    }
    if let Some(p) = a.get(12) {
        write_png(Path::new(p), w, h, &img).unwrap();
    }
    let exact = t.quality == 0 && (bw / (GRID_W * UNIT_W) as f32 - 4.0).abs() < 1e-6;
    let pass = if exact { worst == 0 } else { worst <= 8 && over2 * 1000 <= total };
    println!("twincheck: {} ({})", if pass { "PASS" } else { "FAIL" }, if exact { "classic at 4x: exact" } else { "within the tolerance" });
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

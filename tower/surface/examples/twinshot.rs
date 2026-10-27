//! twinshot: the twin's picture of a live block at a chosen hour and quality
//! (graphics design 8.5), for looking at the lighting without a presenter.
//!
//!   cargo run --release --example twinshot -- LIVE ART OUT.png ZOOM HOUR QUALITY [X0 Y0 X1 Y1]
//!
//! LIVE is a live block as the engine published it (its dump-live.bin), ART
//! the art block (towerpack's), ZOOM target pixels a unit (1 is 1:1, 4 is
//! 4x), HOUR the time of day (20.5 is 20:30), QUALITY 0 classic, 1 smooth,
//! 2 rich.  X0 Y0 X1 Y1, in cells, crop the picture (the whole tower if
//! left out).  The block's own day and weekend flag are kept.

use std::path::Path;
use std::process::ExitCode;

use tower_surface::block::*;
use tower_surface::pack::write_png;
use tower_surface::twin::{self, Textures, View};

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 6 {
        eprintln!("twinshot LIVE ART OUT.png ZOOM HOUR QUALITY [X0 Y0 X1 Y1]");
        return ExitCode::from(2);
    }
    let live = std::fs::read(&a[0]).expect("the live block");
    let art = std::fs::read(&a[1]).expect("the art block");
    let zoom: f32 = a[3].parse().expect("a zoom");
    let hour: f32 = a[4].parse().expect("an hour");
    let mut t = Tower::from_bytes(&live);
    t.hour = ((hour * 64.0).round() as u16) % 1536;
    t.quality = a[5].parse().expect("a quality");
    let tex = Textures::new(&t, &art);
    let mut v = View::whole(zoom);
    if a.len() >= 10 {
        let c = |i: usize| a[i].parse::<u32>().expect("a cell");
        v = v.cells(c(6), c(7), c(8), c(9));
    }
    match twin::render(&t, &art, &tex, &v) {
        Ok(img) => {
            write_png(Path::new(&a[2]), v.clip[2] as usize, v.clip[3] as usize, &img).unwrap();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("twinshot: {e}");
            ExitCode::FAILURE
        }
    }
}

//! towerpack's proofs: every table entry lies inside its atlas, the fixed
//! entries are where the design puts them, and the same art gives the same
//! bytes.  Needs the rendered art (python3 art/tools/render.py); without it
//! the tests say so and pass.

use std::path::PathBuf;

use tower_surface::block::*;
use tower_surface::pack::{self, Options};
use tower_surface::signs;

fn art() -> Option<PathBuf> {
    let a = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../art");
    if a.join("out/4/rooms").is_dir() {
        Some(a)
    } else {
        eprintln!("no rendered art: run python3 art/tools/render.py");
        None
    }
}

#[test]
fn packs_inside_and_the_same_twice() {
    let Some(a) = art() else { return };
    for lang in ["en", "fr"] {
        let o = Options::new(&a, 4, lang);
        let p = pack::pack(&o).unwrap();
        pack::verify(&p.block).unwrap();
        let q = pack::pack(&o).unwrap();
        assert!(p.block == q.block, "{lang}: two runs differ");
        assert_eq!(p.manifest(), q.manifest());
    }
}

#[test]
fn fixed_entries() {
    let Some(a) = art() else { return };
    let p = pack::pack(&Options::new(&a, 4, "en")).unwrap();
    let h = p.head();
    assert_eq!(h.scale, 4);
    assert_eq!(h.emit_shift, 1);
    // the aux picture carries the glass (data/glass.txt): an office's window
    assert_eq!(h.flags & TWA_F_AUX, TWA_F_AUX);
    let o = h.art[p.art_of("rooms/office.0") as usize];
    assert_ne!(o[3] & A_AUX, 0, "office.0 has glass");
    assert_eq!(h.art[p.art_of("rooms/office.1") as usize][3] & A_AUX, 0, "office.1 has none");
    let aux = &p.block[h.at_aux()..h.at_sprites()];
    let (x, y, w, hh) = ((o[0] & 0xffff) as usize, (o[0] >> 16) as usize, (o[1] & 0xffff) as usize, (o[1] >> 16) as usize);
    let panes = (y..y + hh).flat_map(|r| (x..x + w).map(move |c| (r, c))).filter(|&(r, c)| aux[(r * h.atlas_w as usize + c) * 2] == 2).count();
    assert!(panes > 300, "office.0's panes: {panes} texels");
    assert_eq!(h.lang, u16::from_le_bytes(*b"en"));
    assert_eq!(p.art_of("world/skyline-far"), ART_SKY_FAR);
    assert_eq!(p.art_of("world/skyline-near"), ART_SKY_NEAR);
    // a shaft's eight pieces from its base
    let b = p.art_of("transport/elevator");
    for (i, n) in ["-top", "-pit", "-landing-skip", "-landing.0", "-landing.1", "-landing.2", "-landing.3"]
        .iter()
        .enumerate()
    {
        assert_eq!(p.art_of(&format!("transport/elevator{n}")), b + 1 + i as u32);
    }
    assert_eq!(p.sprite_of("rooms/cockroach"), SPR_ROACH);
    assert_eq!(h.spr[SPR_ROACH as usize][3] >> 16, 4, "the cockroach has four frames");
    assert_eq!(p.sprite_of("signs/for-rent-9"), signs::sprite("for-rent-9").unwrap());
    // a car kind's nine
    let c = p.sprite_of("transport/elevator-car");
    assert_eq!(p.sprite_of("transport/elevator-car-load.3"), c + 8);
    // flags
    let lobby = h.art[p.art_of("rooms/lobby") as usize];
    assert!(lobby[3] & A_REPEAT != 0);
    let fire = h.art[p.art_of("events/fire") as usize];
    assert!(fire[3] & A_FIRE != 0 && fire[3] & A_ANIM != 0 && fire[3] >> 16 == 6);
    let office = h.art[p.art_of("rooms/office.0") as usize];
    assert_eq!((office[1] & 0xffff, office[1] >> 16), (9 * 16, 64), "office: 9 cells x 1 floor at S = 4");
    assert!(office[3] & A_EMIT != 0);
    // the profiles: weekday office lamps on at ten, few at three (cleaners)
    assert!(h.prof[0][P_OFFICE as usize][10] > 200 && h.prof[0][P_OFFICE as usize][3] < 40);
    assert!(h.prof[1][P_OFFICE as usize][10] < 100, "fewer at the weekend");
}

#[test]
fn retina_scale_8() {
    let Some(a) = art() else { return };
    if !a.join("out/8/rooms").is_dir() {
        return;
    }
    let p = pack::pack(&Options::new(&a, 8, "fr")).unwrap();
    let h = p.head();
    assert_eq!(h.scale, 8);
    let office = h.art[p.art_of("rooms/office.0") as usize];
    assert_eq!((office[1] & 0xffff, office[1] >> 16), (9 * 32, 128));
    // a sprite's size in the tower does not depend on the scale
    let p4 = pack::pack(&Options::new(&a, 4, "fr")).unwrap();
    assert_eq!(h.spr[SPR_ROACH as usize][2], p4.head().spr[SPR_ROACH as usize][2]);
}

#[test]
fn verify_catches_an_entry_outside() {
    let Some(a) = art() else { return };
    let p = pack::pack(&Options::new(&a, 4, "en")).unwrap();
    let mut b = p.block.clone();
    let at = std::mem::offset_of!(TowerArt, art) + 16 * 5; // art 5's x word
    b[at..at + 4].copy_from_slice(&(4090u32 | 10 << 16).to_le_bytes());
    assert!(pack::verify(&b).is_err());
}

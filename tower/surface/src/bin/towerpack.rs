//! towerpack: the tower's art, rendered by art/tools/render.py, packed into
//! an art block (tower.h's struct ros_tower_art and its pictures).
//!
//!   towerpack --out FILE [--art DIR] [--scale 4|8] [--lang en] [--gen N]
//!             [--manifest FILE] [--dump DIR]
//!
//! --art      the art tree (default: ../art beside this crate); reads
//!            out/<scale>/ and, when present, src/world/sky-ramp.png and
//!            src/lamps.txt
//! --scale    4 for the presenters' atlas, 8 for the Mac's Retina set
//! --lang     the signs' language (art/out/<scale>/signs/<lang>)
//! --manifest every entry's number, name and place, one a line
//! --dump     the atlases as PNG (straight alpha), to look at
//!
//! The block is checked (every table entry inside its atlas) before it is
//! written; the same art gives the same bytes.

use std::path::PathBuf;
use std::process::ExitCode;

use tower_surface::pack;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("towerpack: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut art = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../art");
    let (mut scale, mut lang, mut gen) = (4u32, "en".to_string(), 1u32);
    let (mut out, mut manifest, mut dump) = (None::<PathBuf>, None::<PathBuf>, None::<PathBuf>);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().ok_or_else(|| format!("{a} wants a value"));
        match a.as_str() {
            "--art" => art = val()?.into(),
            "--scale" => scale = val()?.parse().map_err(|_| "--scale: a number")?,
            "--lang" => lang = val()?,
            "--gen" => gen = val()?.parse().map_err(|_| "--gen: a number")?,
            "--out" => out = Some(val()?.into()),
            "--manifest" => manifest = Some(val()?.into()),
            "--dump" => dump = Some(val()?.into()),
            "-h" | "--help" => {
                println!("towerpack --out FILE [--art DIR] [--scale 4|8] [--lang en] [--gen N] [--manifest FILE] [--dump DIR]");
                return Ok(());
            }
            _ => return Err(format!("{a}: unknown (--help)")),
        }
    }
    let out = out.ok_or("--out FILE is wanted")?;
    let mut o = pack::Options::new(art, scale, &lang);
    o.gen = gen;
    let p = pack::pack(&o)?;
    for n in &p.notes {
        eprintln!("towerpack: {n}");
    }
    std::fs::write(&out, &p.block).map_err(|e| format!("{}: {e}", out.display()))?;
    let a = p.head();
    println!(
        "{}: {} bytes; atlas {} x {} ({} entries), sprites {} x {} ({}), scale {}, signs {}",
        out.display(),
        p.block.len(),
        a.atlas_w,
        a.atlas_h,
        a.nart,
        a.spr_w,
        a.spr_h,
        a.nsprites,
        a.scale,
        lang
    );
    if let Some(m) = manifest {
        std::fs::write(&m, p.manifest()).map_err(|e| format!("{}: {e}", m.display()))?;
    }
    if let Some(d) = dump {
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for (name, w, h, px) in pack::pictures(&p.block) {
            pack::write_png(&d.join(format!("{name}.png")), w, h, &px)?;
        }
    }
    Ok(())
}

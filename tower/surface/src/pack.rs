//! The art packer (towerpack's library): art/out/<scale> to an art block
//! (tower.h's struct ros_tower_art and the pictures after it).
//!
//! What it does, by design 10.1 and 10.2:
//!
//! - Reads every PNG render.py wrote for one scale (4, or 8 for the Mac's
//!   Retina set) and the signs of one language.  `name.png` is a picture,
//!   `name.emit.png` its emission; `name.N.png` are frames of one picture
//!   (laid side by side) or variants (consecutive entries), by FRAMES below.
//!   People facing left are dropped (the shader flips the right-facing
//!   ones); a person's `.tint` layer (art/src/people/NOTES.md) is packed in
//!   the place of its emission, and its sprite flagged SPR_TINT: the movers'
//!   pass reads it as the weights of the top's, the skin's and the legs'
//!   colours.
//! - Sorts each picture into the room atlas (rooms, building and world
//!   pieces, the skylines, shafts, stairs and escalators, fire) or the
//!   sprite atlas (anything drawn as a free quad: people, cars, vehicles,
//!   clouds, sun, moon, stars, events, signs, the cockroach).
//! - Shelf-packs each atlas, a gutter of one unit (S texels) round every
//!   picture with its edge texels repeated, both sides multiples of 8 so
//!   that mip levels 0-3 halve exactly.
//! - Writes the tables with the fixed entries first (design 5.4, 5.5): art
//!   1 and 2 the skylines, then each shaft kind's eight pieces; sprites
//!   1-255 the signs (signs.rs), 256 the cockroach, then each car kind's
//!   nine sprites; everything else after, in name order.
//! - The emission picture at half size, premultiplied.  The aux picture
//!   (design 10.2) carries only the glass for now: no art yet has
//!   material, group, slot or height layers, so until it does the windows
//!   that look out (the panes, which show the sky in smooth and rich) are
//!   named by their colours in data/glass.txt (art/src/glass.txt when the
//!   art has one).  Where no picture has glass the aux picture is left out
//!   and ROS_TWA_F_AUX is clear.
//! - The sky ramp: art/src/world/sky-ramp.png (48 x 4) when the art has
//!   one, else the default, fitted to upstream's gradients.  The lamp
//!   profiles: art/src/lamps.txt when the art has one, else data/lamps.txt.
//!
//! Everything is sorted and every number derived from the files, so the
//! same art gives the same bytes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::block::*;
use crate::signs;

/// A picture: straight-alpha RGBA as read, premultiplied by `premultiply`.
#[derive(Clone)]
pub struct Pic {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Pic {
    pub fn new(w: usize, h: usize) -> Pic {
        Pic { w, h, px: vec![0; w * h * 4] }
    }
    fn get(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.w + x) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }
    fn put(&mut self, x: usize, y: usize, v: [u8; 4]) {
        let i = (y * self.w + x) * 4;
        self.px[i..i + 4].copy_from_slice(&v);
    }
}

pub fn read_png(path: &Path) -> Result<Pic, String> {
    let f = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut rd = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; rd.output_buffer_size()];
    let info = rd.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let (ct, _) = rd.output_color_type();
    let mut p = Pic::new(w, h);
    let n = w * h;
    for i in 0..n {
        let v = match ct {
            png::ColorType::Rgba => [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]],
            png::ColorType::Rgb => [buf[i * 3], buf[i * 3 + 1], buf[i * 3 + 2], 255],
            png::ColorType::GrayscaleAlpha => [buf[i * 2], buf[i * 2], buf[i * 2], buf[i * 2 + 1]],
            png::ColorType::Grayscale => [buf[i], buf[i], buf[i], 255],
            png::ColorType::Indexed => return Err(format!("{}: indexed after expansion", path.display())),
        };
        p.px[i * 4..i * 4 + 4].copy_from_slice(&v);
    }
    Ok(p)
}

pub fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) -> Result<(), String> {
    let f = fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| format!("{}: {e}", path.display()))?;
    wr.write_image_data(rgba).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

fn premultiply(p: &mut Pic) {
    for px in p.px.chunks_exact_mut(4) {
        let a = px[3] as u32;
        for c in &mut px[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
}

/// premultiplied back to straight alpha, for looking at a dump
pub fn unpremultiply(px: &[u8]) -> Vec<u8> {
    let mut v = px.to_vec();
    for p in v.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 0 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    v
}

// ---- the art's names --------------------------------------------------------

/// Which numbered pictures are frames of one picture (and whether the clock
/// runs them); every other `name.N` is a variant, an entry of its own.
fn frames_rule(dir: &str, base: &str) -> Option<bool> {
    match dir {
        "events" | "vehicles" => Some(true),
        "building" => matches!(base, "construct" | "roof-beacon").then_some(true),
        "rooms" => (base == "cockroach").then_some(true),
        "transport" => (base == "escalator").then_some(true),
        "world" => (base == "star").then_some(false),
        "people" => base.contains("-walk-").then_some(false),
        "signs" => (base == "evacuate").then_some(false),
        _ => None,
    }
}

/// true: the sprite atlas; false: the room atlas
fn is_sprite(dir: &str, base: &str) -> bool {
    match dir {
        "rooms" => base == "cockroach",
        "building" => false,
        "world" => base.starts_with("cloud-") || matches!(base, "sun" | "moon" | "star"),
        "transport" => base.contains("-car") || base == "car-lift-platform",
        "events" => !matches!(base, "fire" | "fire-left" | "fire-right" | "burnt"),
        _ => true, // people, vehicles, signs
    }
}

/// One table entry before packing: its frames' pictures and emissions.
struct Item {
    name: String, // dir/base, as the manifest and the engine name it
    sprite: bool,
    anim: bool,
    tint: bool, // the "emission" pictures are tint layers
    frames: Vec<(Pic, Option<Pic>)>,
    // where it went
    x: usize,
    y: usize,
}

impl Item {
    fn fw(&self) -> usize {
        self.frames[0].0.w
    }
    fn w(&self) -> usize {
        self.fw() * self.frames.len()
    }
    fn h(&self) -> usize {
        self.frames[0].0.h
    }
    fn emit(&self) -> bool {
        self.frames.iter().any(|f| f.1.is_some())
    }
}

/// what towerpack is asked for
#[derive(Clone, Debug)]
pub struct Options {
    /// the art tree (rosgd/ports/tower/art): out/<scale> and src/ are read
    pub art: PathBuf,
    pub scale: u32,
    pub lang: String,
    /// the art block's generation
    pub gen: u32,
}

impl Options {
    pub fn new(art: impl Into<PathBuf>, scale: u32, lang: &str) -> Options {
        Options { art: art.into(), scale, lang: lang.into(), gen: 1 }
    }
}

/// the packed art: the block, and every entry's number by name
pub struct Packed {
    pub block: Vec<u8>,
    pub art: BTreeMap<String, u32>,
    pub sprites: BTreeMap<String, u32>,
    pub notes: Vec<String>,
}

impl Packed {
    pub fn head(&self) -> Box<TowerArt> {
        TowerArt::from_bytes(&self.block)
    }
    pub fn art_of(&self, name: &str) -> u32 {
        *self.art.get(name).unwrap_or_else(|| panic!("no art entry {name}"))
    }
    pub fn sprite_of(&self, name: &str) -> u32 {
        *self.sprites.get(name).unwrap_or_else(|| panic!("no sprite {name}"))
    }
    /// the manifest: one line an entry, `art|sprite index name x y w h frames flags`
    pub fn manifest(&self) -> String {
        let a = self.head();
        let mut s = String::new();
        s.push_str("# kind index name x y w h frames flags (texels; w is all frames)\n");
        let mut rows: Vec<(&str, u32, &String)> = Vec::new();
        rows.extend(self.art.iter().map(|(n, &i)| ("art", i, n)));
        rows.extend(self.sprites.iter().map(|(n, &i)| ("sprite", i, n)));
        rows.sort_by(|p, q| (p.0, p.1).cmp(&(q.0, q.1)));
        for (k, i, n) in rows {
            let e = if k == "art" { a.art[i as usize] } else { a.spr[i as usize] };
            let frames = (e[3] >> 16).max(1);
            let w = if k == "art" { e[1] & 0xffff } else { (e[1] & 0xffff) * frames };
            s.push_str(&format!(
                "{k} {i} {n} {} {} {} {} {} {}\n",
                e[0] & 0xffff,
                e[0] >> 16,
                w,
                e[1] >> 16,
                frames,
                e[3] & 0xffff
            ));
        }
        s
    }
}

fn split_frame(stem: &str) -> Option<(&str, u32)> {
    let (b, n) = stem.rsplit_once('.')?;
    n.parse::<u32>().ok().map(|n| (b, n))
}

/// every PNG of one directory, as (name, picture path, emission path)
fn list_dir(dir: &Path) -> Result<Vec<(String, PathBuf, Option<PathBuf>)>, String> {
    let mut files: Vec<String> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e} (run python3 art/tools/render.py)", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".png"))
        .collect();
    files.sort();
    let mut v = Vec::new();
    for f in &files {
        let stem = &f[..f.len() - 4];
        if stem.ends_with(".emit") || stem.contains(".tint") {
            continue;
        }
        let emit = dir.join(format!("{stem}.emit.png"));
        // a person's tint layer goes where an emission would
        let tint = dir.join(format!("{stem}.tint.png"));
        let layer = if emit.exists() { Some(emit) } else { tint.exists().then_some(tint) };
        v.push((stem.to_string(), dir.join(f), layer));
    }
    Ok(v)
}

fn collect(o: &Options, notes: &mut Vec<String>) -> Result<Vec<Item>, String> {
    let root = o.art.join("out").join(o.scale.to_string());
    let mut items: BTreeMap<String, Item> = BTreeMap::new();
    let mut frame_parts: BTreeMap<String, BTreeMap<u32, (Pic, Option<Pic>)>> = BTreeMap::new();
    let dirs = ["rooms", "building", "world", "transport", "events", "people", "vehicles", "signs"];
    for d in dirs {
        let path = if d == "signs" { root.join("signs").join(&o.lang) } else { root.join(d) };
        if d == "signs" && !path.is_dir() {
            return Err(format!("{}: no signs for language {:?}", path.display(), o.lang));
        }
        for (stem, pic, emit) in list_dir(&path)? {
            if d == "people" && stem.contains("-left") {
                continue; // the shader flips the right-facing figure
            }
            let mut p = read_png(&pic)?;
            premultiply(&mut p);
            let e = match emit {
                Some(ep) => {
                    let mut e = read_png(&ep)?;
                    if (e.w, e.h) != (p.w, p.h) {
                        return Err(format!("{}: not the picture's size", ep.display()));
                    }
                    premultiply(&mut e);
                    Some(e)
                }
                None => None,
            };
            if let Some((base, n)) = split_frame(&stem) {
                if frames_rule(d, base).is_some() {
                    frame_parts.entry(format!("{d}/{base}")).or_default().insert(n, (p, e));
                    continue;
                }
            }
            let base = stem.as_str();
            let key = format!("{d}/{base}");
            let sprite = is_sprite(d, split_frame(base).map(|b| b.0).unwrap_or(base));
            let tint = d == "people" && e.is_some();
            items.insert(key.clone(), Item { name: key, sprite, anim: false, tint, frames: vec![(p, e)], x: 0, y: 0 });
        }
    }
    for (key, parts) in frame_parts {
        let (d, base) = key.split_once('/').unwrap();
        let (d, base) = (d.to_string(), base.to_string());
        let (d, base) = (d.as_str(), base.as_str());
        let anim = frames_rule(d, base).unwrap();
        let n = parts.len() as u32;
        if parts.keys().copied().ne(0..n) {
            return Err(format!("{key}: frames not numbered 0..{}", n - 1));
        }
        let frames: Vec<_> = parts.into_values().collect();
        if frames.iter().any(|f| (f.0.w, f.0.h) != (frames[0].0.w, frames[0].0.h)) {
            return Err(format!("{key}: frames of different sizes"));
        }
        let tint = d == "people" && frames.iter().any(|f| f.1.is_some());
        items.insert(key.clone(), Item { name: key, sprite: is_sprite(d, base), anim, tint, frames, x: 0, y: 0 });
    }
    let ignored = ["mat", "groups", "slots", "height", "full"];
    for it in items.values() {
        if let Some((_, layer)) = it.name.rsplit_once('.') {
            if ignored.contains(&layer) {
                notes.push(format!("{}: aux layer not read yet (design 10.2)", it.name));
            }
        }
    }
    Ok(items.into_values().collect())
}

/// shelf-pack: every item at (x, y), a gutter `g` round it; returns the
/// atlas height (a multiple of 8)
fn shelf_pack(items: &mut [&mut Item], width: usize, g: usize) -> Result<usize, String> {
    items.sort_by(|a, b| (b.h(), b.w(), &a.name).cmp(&(a.h(), a.w(), &b.name)));
    let (mut x, mut y, mut shelf) = (0usize, 0usize, 0usize);
    for it in items.iter_mut() {
        let (w, h) = (it.w() + 2 * g, it.h() + 2 * g);
        if w > width {
            return Err(format!("{}: {} texels wide, the atlas {}", it.name, it.w(), width));
        }
        if x + w > width {
            y += shelf;
            x = 0;
            shelf = 0;
        }
        if shelf == 0 {
            shelf = h;
        }
        it.x = x + g;
        it.y = y + g;
        x += w;
    }
    Ok((y + shelf).div_ceil(8) * 8)
}

/// draw the items into an atlas, their gutters filled with their edge texels
fn blit(atlas: &mut Pic, items: &[&mut Item], g: usize, emission: bool) {
    for it in items {
        let (w, h) = (it.w(), it.h());
        let fw = it.fw();
        let texel = |x: isize, y: isize| -> [u8; 4] {
            let x = x.clamp(0, w as isize - 1) as usize;
            let y = y.clamp(0, h as isize - 1) as usize;
            let f = &it.frames[x / fw];
            let p = if emission { f.1.as_ref() } else { Some(&f.0) };
            p.map(|p| p.get(x % fw, y)).unwrap_or([0; 4])
        };
        for y in -(g as isize)..(h + g) as isize {
            for x in -(g as isize)..(w + g) as isize {
                let v = texel(x, y);
                atlas.put((it.x as isize + x) as usize, (it.y as isize + y) as usize, v);
            }
        }
    }
}

/// half size, a 2 x 2 box, rounded
pub fn halve(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (hw, hh) = (w / 2, h / 2);
    let mut d = vec![0u8; hw * hh * 4];
    for y in 0..hh {
        for x in 0..hw {
            for c in 0..4 {
                let s = |xx: usize, yy: usize| src[(yy * w + xx) * 4 + c] as u32;
                let v = s(2 * x, 2 * y) + s(2 * x + 1, 2 * y) + s(2 * x, 2 * y + 1) + s(2 * x + 1, 2 * y + 1);
                d[(y * hw + x) * 4 + c] = ((v + 2) / 4) as u8;
            }
        }
    }
    d
}

// ---- the sky ramp and the lamp profiles --------------------------------------

/// upstream's nightAlpha, from an hour 0..24
pub fn night_alpha(hour: f32) -> f32 {
    let h = if hour < 7.0 { hour + 24.0 } else { hour };
    if h < 20.0 {
        0.0
    } else if h < 21.0 {
        h - 20.0
    } else if h < 28.0 {
        1.0
    } else if h < 30.0 {
        1.0 - (h - 28.0) * 0.5
    } else {
        0.0
    }
}

/// the default ramp, until the art paints one: night, dawn, day and dusk
/// as keys at the hours below, linear between them, read at each half
/// hour.  By day and by night the zenith and the horizon are upstream's
/// (the tops and bottoms of its two gradients); dawn (05:00-07:30) and
/// dusk (18:00-21:30) pass through a mauve and a warm horizon.  Ambient is
/// what lights things outdoors (white by day, a dim blue at night); misc
/// is daylight (how far the lamps' look gives way to the day's), the
/// stars, the sun and the glass's reflection.
fn default_ramp() -> [[[u8; 4]; RAMP]; 4] {
    // hour, zenith, horizon, ambient (r g b), daylight, stars, sun, glass
    type Key = (f32, u32, u32, [f32; 3], f32, f32, f32, f32);
    const NIGHT: ([f32; 3], u32, u32) = ([0.30, 0.34, 0.50], 0x04091a, 0x0e1f4a);
    const DAY: ([f32; 3], u32, u32) = ([1.0, 1.0, 1.0], 0x1a3a6e, 0xb4ddf0);
    let keys: [Key; 14] = [
        (0.0, NIGHT.1, NIGHT.2, NIGHT.0, 0.0, 1.0, 0.0, 0.05),
        (4.5, NIGHT.1, NIGHT.2, NIGHT.0, 0.0, 1.0, 0.0, 0.05),
        (5.5, 0x0b1438, 0x5a4a78, [0.45, 0.44, 0.58], 0.15, 0.5, 0.0, 0.06),
        (6.0, 0x1c3366, 0xe89a78, [0.78, 0.66, 0.62], 0.45, 0.1, 0.35, 0.08),
        (6.5, 0x1e3a70, 0xf4c08c, [0.92, 0.84, 0.76], 0.75, 0.0, 0.6, 0.09),
        (7.5, DAY.1, DAY.2, DAY.0, 1.0, 0.0, 0.8, 0.10),
        (12.0, DAY.1, DAY.2, DAY.0, 1.0, 0.0, 1.0, 0.10),
        (17.5, DAY.1, DAY.2, DAY.0, 1.0, 0.0, 0.8, 0.10),
        (18.5, 0x1e3a70, 0xf0c890, [1.0, 0.93, 0.84], 0.95, 0.0, 0.6, 0.10),
        (19.5, 0x26305e, 0xf08858, [0.86, 0.66, 0.58], 0.6, 0.0, 0.4, 0.09),
        (20.5, 0x141a40, 0x7a3a58, [0.55, 0.48, 0.60], 0.25, 0.4, 0.0, 0.07),
        (21.5, NIGHT.1, NIGHT.2, NIGHT.0, 0.0, 1.0, 0.0, 0.05),
        (24.0, NIGHT.1, NIGHT.2, NIGHT.0, 0.0, 1.0, 0.0, 0.05),
        (24.0, NIGHT.1, NIGHT.2, NIGHT.0, 0.0, 1.0, 0.0, 0.05),
    ];
    let rgb = |c: u32| [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0];
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut r = [[[0u8; 4]; RAMP]; 4];
    for i in 0..RAMP {
        let hour = i as f32 * 0.5;
        let k = keys.windows(2).position(|w| hour >= w[0].0 && hour < w[1].0).unwrap_or(0);
        let (p, q) = (keys[k], keys[k + 1]);
        let t = if q.0 > p.0 { (hour - p.0) / (q.0 - p.0) } else { 0.0 };
        let (z0, z1, h0, h1) = (rgb(p.1), rgb(q.1), rgb(p.2), rgb(q.2));
        for c in 0..3 {
            r[0][i][c] = b(lerp(z0[c], z1[c], t));
            r[1][i][c] = b(lerp(h0[c], h1[c], t));
            r[2][i][c] = b(lerp(p.3[c], q.3[c], t));
        }
        r[0][i][3] = 255;
        r[1][i][3] = 255;
        r[2][i][3] = 255;
        r[3][i] = [b(lerp(p.4, q.4, t)), b(lerp(p.5, q.5, t)), b(lerp(p.6, q.6, t)), b(lerp(p.7, q.7, t))];
    }
    r
}

pub const PROFILE_NAMES: [&str; 12] = [
    "never", "always", "office", "home", "hotel", "shop", "restaurant", "fast-food", "entertainment",
    "service", "parking", "lobby",
];

pub const DEFAULT_LAMPS: &str = include_str!("../data/lamps.txt");

/// the panes that look out, by picture (design 10.2's material layer, until
/// the art has one): `name colour ...`, a name ending in * a prefix
pub const DEFAULT_GLASS: &str = include_str!("../data/glass.txt");
/// how near (each channel, of 255) a texel must be to a pane's colour
const GLASS_NEAR: u8 = 4;
/// the material number of glass (the shaders' M_GLASS)
const M_GLASS: u32 = 2;

pub fn parse_glass(text: &str) -> Result<Vec<(String, Vec<[u8; 3]>)>, String> {
    let mut v = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let bad = |m: &str| format!("glass line {}: {m}", ln + 1);
        if f.len() < 2 {
            return Err(bad("want a picture and its pane colours"));
        }
        let mut cols = Vec::new();
        for c in &f[1..] {
            let n = u32::from_str_radix(c, 16).ok().filter(|_| c.len() == 6).ok_or_else(|| bad("a colour is six hex digits"))?;
            cols.push([(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        }
        v.push((f[0].to_string(), cols));
    }
    Ok(v)
}

fn glob(pat: &str, name: &str) -> bool {
    match pat.strip_suffix('*') {
        Some(p) => name.starts_with(p),
        None => pat == name,
    }
}

pub fn parse_lamps(text: &str) -> Result<[[[u8; 24]; PROFILES]; 2], String> {
    let mut p = [[[0u8; 24]; PROFILES]; 2];
    for (ln, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let bad = |m: &str| format!("lamps line {}: {m}", ln + 1);
        if f.len() != 26 {
            return Err(bad("want a profile, a day and 24 numbers"));
        }
        let prof = PROFILE_NAMES.iter().position(|n| *n == f[0]).ok_or_else(|| bad("unknown profile"))?;
        let day = match f[1] {
            "weekday" => 0,
            "weekend" => 1,
            _ => return Err(bad("day is weekday or weekend")),
        };
        for h in 0..24 {
            p[day][prof][h] = f[2 + h].parse::<u8>().map_err(|_| bad("a number 0-255"))?;
        }
    }
    Ok(p)
}

// ---- the block ----------------------------------------------------------------

/// the fixed room-atlas entries after the skylines: each shaft kind's eight
/// pieces (body, top, pit, skip, landing doors 0-3), the car lift's four
/// entrance frames after its eight
fn fixed_art() -> Vec<String> {
    let mut v = vec!["world/skyline-far".to_string(), "world/skyline-near".to_string()];
    for k in ["elevator", "elevator-express", "elevator-service", "car-lift"] {
        v.push(format!("transport/{k}"));
        v.push(format!("transport/{k}-top"));
        v.push(format!("transport/{k}-pit"));
        v.push(format!("transport/{k}-landing-skip"));
        for n in 0..4 {
            v.push(format!("transport/{k}-landing.{n}"));
        }
    }
    for n in 0..4 {
        v.push(format!("transport/car-lift-entrance.{n}"));
    }
    v
}

/// the fixed sprites after the signs: the cockroach, then each car kind's
/// nine (car, doors 0-3, load 0-3)
fn fixed_sprites() -> Vec<String> {
    let mut v = vec!["rooms/cockroach".to_string()];
    for k in ["elevator", "elevator-express", "elevator-service"] {
        v.push(format!("transport/{k}-car"));
        for n in 0..4 {
            v.push(format!("transport/{k}-car-doors.{n}"));
        }
        for n in 0..4 {
            v.push(format!("transport/{k}-car-load.{n}"));
        }
    }
    v
}

pub fn pack(o: &Options) -> Result<Packed, String> {
    if !matches!(o.scale, 4 | 8 | 16) {
        return Err(format!("scale {}: render.py makes 4, 8 and 16", o.scale));
    }
    let lang = o.lang.as_bytes();
    if lang.len() != 2 {
        return Err(format!("language {:?}: two letters", o.lang));
    }
    let mut notes = Vec::new();
    let mut items = collect(o, &mut notes)?;
    let s = o.scale as usize;
    let g = s; // a unit's gutter
    let atlas_w = 1024 * s;
    let spr_w = 512 * s;

    // ---- numbering
    let mut art: BTreeMap<String, u32> = BTreeMap::new();
    let mut spr: BTreeMap<String, u32> = BTreeMap::new();
    let by_name: BTreeMap<String, usize> = items.iter().enumerate().map(|(i, it)| (it.name.clone(), i)).collect();
    for (i, n) in fixed_art().iter().enumerate() {
        if !by_name.contains_key(n) {
            return Err(format!("{n}: missing (a fixed art entry)"));
        }
        art.insert(n.clone(), i as u32 + 1);
    }
    for (i, n) in signs::names().iter().enumerate() {
        if n.is_empty() {
            continue;
        }
        let key = format!("signs/{n}");
        if by_name.contains_key(&key) {
            spr.insert(key, i as u32 + SPR_SIGNS);
        } else {
            notes.push(format!("{key}: no picture in {}", o.lang));
        }
    }
    for (i, n) in fixed_sprites().iter().enumerate() {
        if !by_name.contains_key(n) {
            return Err(format!("{n}: missing (a fixed sprite)"));
        }
        spr.insert(n.clone(), i as u32 + SPR_ROACH);
    }
    let mut next_art = art.len() as u32 + 1;
    let mut next_spr = SPR_ROACH + fixed_sprites().len() as u32;
    for it in &items {
        if it.sprite {
            if it.name.starts_with("signs/") && !spr.contains_key(&it.name) {
                notes.push(format!("{}: not in signs.rs, not packed", it.name));
                continue;
            }
            spr.entry(it.name.clone()).or_insert_with(|| {
                next_spr += 1;
                next_spr - 1
            });
        } else {
            art.entry(it.name.clone()).or_insert_with(|| {
                next_art += 1;
                next_art - 1
            });
        }
    }
    if next_art as usize > ART || next_spr as usize > SPRITES {
        return Err(format!("{next_art} art entries, {next_spr} sprites: too many"));
    }
    items.retain(|it| if it.sprite { spr.contains_key(&it.name) } else { art.contains_key(&it.name) });

    // ---- packing
    let (mut rooms, mut sprites): (Vec<&mut Item>, Vec<&mut Item>) = items.iter_mut().partition(|it| !it.sprite);
    let atlas_h = shelf_pack(&mut rooms, atlas_w, g)?;
    let spr_h = shelf_pack(&mut sprites, spr_w, g)?;
    if atlas_h > 16384 || spr_h > 16384 {
        return Err(format!("atlas {atlas_w} x {atlas_h}, sprites {spr_w} x {spr_h}: over 16384"));
    }
    let mut albedo = Pic::new(atlas_w, atlas_h);
    let mut emis = Pic::new(atlas_w, atlas_h);
    blit(&mut albedo, &rooms, g, false);
    blit(&mut emis, &rooms, g, true);
    let mut sp = Pic::new(spr_w, spr_h);
    let mut spe = Pic::new(spr_w, spr_h);
    blit(&mut sp, &sprites, g, false);
    blit(&mut spe, &sprites, g, true);
    let emis_half = halve(&emis.px, atlas_w, atlas_h);

    // ---- the glass, into the aux picture (material 2 where a pane is)
    let glass_txt = o.art.join("src/glass.txt");
    let glass = if glass_txt.exists() {
        notes.push(format!("glass from {}", glass_txt.display()));
        parse_glass(&fs::read_to_string(&glass_txt).map_err(|e| format!("{}: {e}", glass_txt.display()))?)?
    } else {
        parse_glass(DEFAULT_GLASS)?
    };
    let mut aux = vec![0u8; atlas_w * atlas_h * 2];
    let mut glazed: Vec<String> = Vec::new();
    for it in rooms.iter() {
        let panes: Vec<[u8; 3]> =
            glass.iter().filter(|(pat, _)| glob(pat, &it.name)).flat_map(|(_, c)| c.iter().copied()).collect();
        if panes.is_empty() {
            continue;
        }
        let mut any = false;
        for y in 0..it.h() {
            for x in 0..it.w() {
                let v = it.frames[x / it.fw()].0.get(x % it.fw(), y);
                // a pane is opaque, so its premultiplied texel is its colour
                let pane = v[3] == 255
                    && panes.iter().any(|c| (0..3).all(|k| v[k].abs_diff(c[k]) <= GLASS_NEAR));
                if pane {
                    aux[((it.y + y) * atlas_w + it.x + x) * 2] = M_GLASS as u8;
                    any = true;
                }
            }
        }
        if any {
            glazed.push(it.name.clone());
        }
    }

    // ---- the head and the tables
    let mut a = TowerArt::zeroed();
    a.magic = TOWER_ART_MAGIC;
    a.version = TOWER_VERSION;
    a.gen = o.gen;
    a.scale = o.scale as u16;
    a.atlas_w = atlas_w as u16;
    a.atlas_h = atlas_h as u16;
    a.spr_w = spr_w as u16;
    a.spr_h = spr_h as u16;
    a.nart = next_art as u16;
    a.nsprites = next_spr as u16;
    a.emit_shift = 1;
    a.flags = if glazed.is_empty() { 0 } else { TWA_F_AUX };
    a.lang = u16::from_le_bytes([lang[0], lang[1]]);
    for it in rooms.iter() {
        let i = art[&it.name] as usize;
        let base = it.name.rsplit('/').next().unwrap();
        let mut flags = 0;
        if it.anim {
            flags |= A_ANIM;
        }
        if it.emit() {
            flags |= A_EMIT;
        }
        if it.name == "rooms/lobby" {
            flags |= A_REPEAT;
        }
        if matches!(base, "fire" | "fire-left" | "fire-right") && it.name.starts_with("events/") {
            flags |= A_FIRE;
        }
        if glazed.contains(&it.name) {
            flags |= A_AUX;
        }
        a.art[i] = art_entry(it.x as u32, it.y as u32, it.w() as u32, it.h() as u32, 0, 0, flags, it.frames.len() as u32);
    }
    for it in sprites.iter() {
        let i = spr[&it.name] as usize;
        let mut flags = 0;
        if it.anim {
            flags |= A_ANIM;
        }
        if it.tint {
            flags |= SPR_TINT;
        } else if it.emit() {
            flags |= A_EMIT;
        }
        let q = |t: usize| (t * 4 / s) as u32; // texels to quarter units
        a.spr[i] = [
            it.x as u32 | (it.y as u32) << 16,
            it.fw() as u32 | (it.h() as u32) << 16,
            q(it.fw()) | q(it.h()) << 16,
            flags | (it.frames.len() as u32) << 16,
        ];
    }
    let ramp_png = o.art.join("src/world/sky-ramp.png");
    a.ramp = if ramp_png.exists() {
        let p = read_png(&ramp_png)?;
        if (p.w, p.h) != (RAMP, 4) {
            return Err(format!("{}: want {RAMP} x 4", ramp_png.display()));
        }
        let mut r = [[[0u8; 4]; RAMP]; 4];
        for (row, rr) in r.iter_mut().enumerate() {
            for (c, v) in rr.iter_mut().enumerate() {
                *v = p.get(c, row);
            }
        }
        notes.push(format!("sky ramp from {}", ramp_png.display()));
        r
    } else {
        default_ramp()
    };
    let lamps = o.art.join("src/lamps.txt");
    a.prof = if lamps.exists() {
        notes.push(format!("lamp profiles from {}", lamps.display()));
        parse_lamps(&fs::read_to_string(&lamps).map_err(|e| format!("{}: {e}", lamps.display()))?)?
    } else {
        parse_lamps(DEFAULT_LAMPS)?
    };

    // ---- the block
    let total = a.total_size();
    a.size = u32::try_from(total).map_err(|_| "the block is over 4 GB".to_string())?;
    let mut block = Vec::with_capacity(total);
    block.extend_from_slice(a.as_bytes());
    block.extend_from_slice(&albedo.px);
    block.extend_from_slice(&emis_half);
    if a.has_aux() {
        block.extend_from_slice(&aux);
    }
    block.extend_from_slice(&sp.px);
    block.extend_from_slice(&spe.px);
    assert_eq!(block.len(), total);
    verify(&block)?;
    Ok(Packed { block, art, sprites: spr, notes })
}

/// Every table entry lies inside its atlas, every count and size agrees.
pub fn verify(block: &[u8]) -> Result<(), String> {
    if block.len() < std::mem::size_of::<TowerArt>() {
        return Err("shorter than its head".into());
    }
    let a = TowerArt::from_bytes(block);
    if a.magic != TOWER_ART_MAGIC || a.version != TOWER_VERSION {
        return Err("magic or version".into());
    }
    if a.size as usize != block.len() || a.total_size() != block.len() {
        return Err(format!("size {} / {} for {} bytes", a.size, a.total_size(), block.len()));
    }
    for (w, h) in [(a.atlas_w, a.atlas_h), (a.spr_w, a.spr_h)] {
        if w % 8 != 0 || h % 8 != 0 || w == 0 || h == 0 {
            return Err(format!("an atlas {w} x {h}: not multiples of 8"));
        }
    }
    if a.nart as usize > ART || a.nsprites as usize > SPRITES {
        return Err("counts".into());
    }
    for i in 0..ART {
        let e = a.art[i];
        if i >= a.nart as usize || i == 0 {
            if e != [0; 4] {
                return Err(format!("art {i}: set past the count"));
            }
            continue;
        }
        if e[3] & A_SOLID != 0 {
            continue;
        }
        let (x, y, w, h) = (e[0] & 0xffff, e[0] >> 16, e[1] & 0xffff, e[1] >> 16);
        let frames = (e[3] >> 16).max(1);
        if w == 0 && h == 0 {
            continue; // a place not filled
        }
        if w == 0 || h == 0 || x + w > a.atlas_w as u32 || y + h > a.atlas_h as u32 || w % frames != 0 {
            return Err(format!("art {i}: {x},{y} {w}x{h} ({frames} frames) outside {} x {}", a.atlas_w, a.atlas_h));
        }
    }
    for i in 0..SPRITES {
        let e = a.spr[i];
        if i >= a.nsprites as usize || i == 0 {
            if e != [0; 4] {
                return Err(format!("sprite {i}: set past the count"));
            }
            continue;
        }
        let (x, y, fw, h) = (e[0] & 0xffff, e[0] >> 16, e[1] & 0xffff, e[1] >> 16);
        let frames = (e[3] >> 16).max(1);
        if fw == 0 && h == 0 {
            continue;
        }
        if fw == 0 || h == 0 || x + fw * frames > a.spr_w as u32 || y + h > a.spr_h as u32 {
            return Err(format!("sprite {i}: {x},{y} {fw}x{h} x{frames} outside {} x {}", a.spr_w, a.spr_h));
        }
    }
    Ok(())
}

/// the four pictures of a block, straight alpha, for looking at:
/// (name, width, height, RGBA)
pub fn pictures(block: &[u8]) -> Vec<(&'static str, usize, usize, Vec<u8>)> {
    let a = TowerArt::from_bytes(block);
    let (w, h) = (a.atlas_w as usize, a.atlas_h as usize);
    let (sw, sh) = (a.spr_w as usize, a.spr_h as usize);
    let mut v = vec![
        ("albedo", w, h, unpremultiply(&block[a.at_albedo()..a.at_emis()])),
        ("emission", a.emis_w(), a.emis_h(), unpremultiply(&block[a.at_emis()..a.at_aux()])),
        ("sprites", sw, sh, unpremultiply(&block[a.at_sprites()..a.at_spremis()])),
        ("sprite-emission", sw, sh, unpremultiply(&block[a.at_spremis()..a.total_size()])),
    ];
    if a.has_aux() {
        let aux = &block[a.at_aux()..a.at_sprites()];
        let rgba: Vec<u8> = aux.chunks_exact(2).flat_map(|p| [p[0] << 4 | p[0] >> 4, p[1] << 4 | p[1] >> 4, 0, 255]).collect();
        v.push(("aux", w, h, rgba));
    }
    v
}

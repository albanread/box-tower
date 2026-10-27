//! The twin: a CPU renderer of the presenters' tower shaders, the reference
//! both are compared against (design 8.5).
//!
//! It is the body of design 7.4 written again in Rust: the same functions
//! with the same names, in the same order, the same arithmetic in `f32`,
//! and textures sampled as GL defines it (bilinear with texel centres at
//! .5, clamp to edge, mip levels made by a rounded 2 x 2 box filter, linear
//! between levels).  A colour is written as round(x * 255).  Where a GPU's
//! own rounding differs the parity test's tolerance covers it; classic at
//! 4x is expected to match exactly.
//!
//! All three qualities (classic 0, smooth 1, rich 2): P1 the field, P2 the
//! fixtures (shafts, their landings, top and pit, stairs, escalators, the
//! car lift) and P3 the movers (cars, people with their tint layer,
//! vehicles), each pass blended over the last as the GPU blends
//! (premultiplied, into an 8-bit target).  Smooth adds the sky ramp, the
//! stars and the skylines, the lamps by profile, the emission at night and
//! windows that show the sky; rich adds the sun, shading, contact shadows,
//! light from lit neighbours, the glass and flames.

use crate::block::*;

// ---- vectors, as GLSL has them ---------------------------------------------

type Vec2 = [f32; 2];
type Vec3 = [f32; 3];
type Vec4 = [f32; 4];
type UVec4 = [u32; 4];

const CELL: Vec2 = [4.0, 16.0];
const LUMA: Vec3 = [0.299, 0.587, 0.114];
const LAMP: Vec3 = [1.00, 0.90, 0.74]; // a room lit at night
const DARK_IN: Vec3 = [0.10, 0.12, 0.20]; // a room unlit at night
const SLAB: Vec3 = [0.46, 0.46, 0.48];
const PART: Vec3 = [0.24, 0.24, 0.27];
const M_GLASS: u32 = 2;
const M_SCREEN: u32 = 7;
const ONE: Vec3 = [1.0, 1.0, 1.0];

/// GLSL's mod: x - y * floor(x / y)
fn gmod(x: f32, y: f32) -> f32 {
    x - y * (x / y).floor()
}
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}
fn mix3(x: Vec3, y: Vec3, a: f32) -> Vec3 {
    [mix(x[0], y[0], a), mix(x[1], y[1], a), mix(x[2], y[2], a)]
}
fn clampf(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}
fn dot3(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn rgb(v: Vec4) -> Vec3 {
    [v[0], v[1], v[2]]
}
fn mixv(x: Vec3, y: Vec3, a: Vec3) -> Vec3 {
    [mix(x[0], y[0], a[0]), mix(x[1], y[1], a[1]), mix(x[2], y[2], a[2])]
}
fn mul3(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}
fn scale3(a: Vec3, k: f32) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn add3(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clampf((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn fract(x: f32) -> f32 {
    x - x.floor()
}
fn length2(a: Vec2) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

// ---- the uniforms (design 7.2), filled as the host fills them --------------

#[derive(Clone, Debug)]
pub struct TwU {
    pub box_: Vec4,
    pub clip: Vec4,
    pub view: Vec4,
    pub clock: Vec4,
    pub zen: Vec4,
    pub hor: Vec4,
    pub amb: Vec4,
    pub misc: Vec4,
    pub grid: [i32; 4],
    pub counts: [i32; 4],
    pub sizes: [i32; 4],
    pub mode: [i32; 4],
    pub outline: [i32; 4],
    pub ocol: Vec4,
    pub pal: [Vec4; 16],
}

fn unpack8(w: u32) -> Vec4 {
    [(w & 255) as f32 / 255.0, ((w >> 8) & 255) as f32 / 255.0, ((w >> 16) & 255) as f32 / 255.0, ((w >> 24) & 255) as f32 / 255.0]
}

/// tw_light(): the sky ramp at an hour, linear between its two half-hour
/// columns, in float from the bytes (both presenters carry this function)
pub fn tw_light(a: &TowerArt, hour: f32) -> [Vec4; 4] {
    let t = gmod(hour * 2.0, RAMP as f32);
    let i0 = (t.floor() as usize).min(RAMP - 1);
    let i1 = (i0 + 1) % RAMP;
    let f = t - i0 as f32;
    let mut out = [[0.0; 4]; 4];
    for (row, o) in out.iter_mut().enumerate() {
        for c in 0..4 {
            let v0 = a.ramp[row][i0][c] as f32 / 255.0;
            let v1 = a.ramp[row][i1][c] as f32 / 255.0;
            o[c] = mix(v0, v1, f);
        }
    }
    out
}

/// The uniforms for drawing the tower in a box `bw` x `bh` target pixels at
/// (bx, by), on a target `tw` x `th`, `secs` seconds into the hour.
pub fn uniforms(t: &Tower, a: &TowerArt, bx: f32, by: f32, bw: f32, bh: f32, tw: f32, th: f32, secs: f32) -> TwU {
    let s = a.scale as f32;
    let tpp = s * (GRID_W * UNIT_W) as f32 / bw;
    let lod = clampf(tpp.log2(), 0.0, 3.0);
    let hour = (t.hour % 1536) as f32 / 64.0;
    let [zen, hor, amb, misc] = tw_light(a, hour);
    let mut pal = [[0.0; 4]; 16];
    for (i, p) in pal.iter_mut().enumerate() {
        *p = unpack8(t.pal[i]);
    }
    TwU {
        box_: [bx, by, bw, bh],
        clip: [0.0, 0.0, tw, th],
        view: [tw, th, tpp, lod],
        clock: [hour, secs, t.day as f32, th],
        zen,
        hor,
        amb,
        misc,
        grid: [GRID_W as i32, GRID_H as i32, (t.ground_row as i32).clamp(0, GRID_H as i32), a.scale as i32],
        counts: [(t.nrooms as i32).min(ROOMS as i32), a.nart as i32, a.nsprites as i32, t.quality as i32],
        sizes: [a.atlas_w as i32, a.atlas_h as i32, a.spr_w as i32, a.spr_h as i32],
        mode: [t.view as i32, a.emit_shift as i32, (t.nfixtures as i32).min(FIXTURES as i32), (t.nmovers as i32).min(MOVERS as i32)],
        outline: [t.outline[0] as i32, t.outline[1] as i32, t.outline[2] as i32, t.outline[3] as i32],
        ocol: unpack8(t.outline_rgba),
        pal,
    }
}

// ---- textures ---------------------------------------------------------------

/// An RGBA8 texture with its mip levels, sampled as GL's
/// LINEAR_MIPMAP_LINEAR with CLAMP_TO_EDGE.
pub struct Tex {
    levels: Vec<(usize, usize, Vec<u8>)>,
}

impl Tex {
    /// levels 0 .. max_level (each half the last, a rounded 2 x 2 box)
    pub fn new(w: usize, h: usize, px: &[u8], max_level: usize) -> Tex {
        let mut levels = vec![(w, h, px.to_vec())];
        while levels.len() <= max_level {
            let (w, h, p) = levels.last().unwrap();
            if *w < 2 || *h < 2 {
                break;
            }
            let half = crate::pack::halve(p, *w, *h);
            levels.push((w / 2, h / 2, half));
        }
        Tex { levels }
    }
    fn texel(&self, l: usize, x: i32, y: i32) -> Vec4 {
        let (w, h, p) = &self.levels[l];
        let x = x.clamp(0, *w as i32 - 1) as usize;
        let y = y.clamp(0, *h as i32 - 1) as usize;
        let i = (y * w + x) * 4;
        [p[i] as f32 / 255.0, p[i + 1] as f32 / 255.0, p[i + 2] as f32 / 255.0, p[i + 3] as f32 / 255.0]
    }
    fn bilinear(&self, l: usize, uv: Vec2) -> Vec4 {
        let (w, h, _) = &self.levels[l];
        let u = uv[0] * *w as f32 - 0.5;
        let v = uv[1] * *h as f32 - 0.5;
        let (i0, j0) = (u.floor(), v.floor());
        let (a, b) = (u - i0, v - j0);
        let (i0, j0) = (i0 as i32, j0 as i32);
        let t00 = self.texel(l, i0, j0);
        let t10 = self.texel(l, i0 + 1, j0);
        let t01 = self.texel(l, i0, j0 + 1);
        let t11 = self.texel(l, i0 + 1, j0 + 1);
        let mut o = [0.0; 4];
        for c in 0..4 {
            o[c] = (1.0 - a) * (1.0 - b) * t00[c] + a * (1.0 - b) * t10[c] + (1.0 - a) * b * t01[c] + a * b * t11[c];
        }
        o
    }
    /// textureLod
    pub fn lod(&self, uv: Vec2, lod: f32) -> Vec4 {
        let max = (self.levels.len() - 1) as f32;
        let lod = clampf(lod, 0.0, max);
        let d1 = lod.floor();
        let f = lod - d1;
        let d1 = d1 as usize;
        let a = self.bilinear(d1, uv);
        if f == 0.0 {
            return a;
        }
        let b = self.bilinear((d1 + 1).min(self.levels.len() - 1), uv);
        [mix(a[0], b[0], f), mix(a[1], b[1], f), mix(a[2], b[2], f), mix(a[3], b[3], f)]
    }
}

/// what the presenter uploads from the two blocks: the tables and pictures
pub struct Textures {
    fix: Vec<UVec4>,
    stops: Vec<[u8; ROWS]>,
    movers: Vec<Mover>,
    cells: Vec<[u16; 4]>,
    rooms: Vec<UVec4>,
    arttab: Vec<UVec4>,
    sprtab: Vec<UVec4>,
    albedo: Tex,
    emis: Tex,
    sprites: Tex,
    spremis: Tex,
    /// the aux picture (material | slot << 4, group | height << 4), atlas
    /// sized, or a 1 x 1 zero when the art has none
    aux: (usize, usize, Vec<[u8; 2]>),
    /// the lamp profiles the header's day names: 16 rows of 24 hours
    prof: [[u8; 24]; PROFILES],
}

impl Textures {
    pub fn new(t: &Tower, art: &[u8]) -> Textures {
        let a = TowerArt::from_bytes(art);
        let cells = t.cell.iter().flat_map(|r| r.iter().map(|c| [c.room, c.back, c.front, c.flags])).collect();
        let rooms = t.room.iter().map(|r| r.w).collect();
        let (w, h) = (a.atlas_w as usize, a.atlas_h as usize);
        let (sw, sh) = (a.spr_w as usize, a.spr_h as usize);
        Textures {
            fix: t.fix.iter().map(|f| f.w).collect(),
            stops: t.stops.to_vec(),
            movers: t.mover.to_vec(),
            cells,
            rooms,
            arttab: a.art.to_vec(),
            sprtab: a.spr.to_vec(),
            albedo: Tex::new(w, h, &art[a.at_albedo()..a.at_emis()], 3),
            emis: Tex::new(a.emis_w(), a.emis_h(), &art[a.at_emis()..a.at_aux()], 2),
            sprites: Tex::new(sw, sh, &art[a.at_sprites()..a.at_spremis()], 3),
            spremis: Tex::new(sw, sh, &art[a.at_spremis()..a.total_size()], 3),
            aux: if a.has_aux() {
                (w, h, art[a.at_aux()..a.at_sprites()].chunks_exact(2).map(|c| [c[0], c[1]]).collect())
            } else {
                (1, 1, vec![[0, 0]])
            },
            prof: a.prof[(t.flags & F_WEEKEND != 0) as usize],
        }
    }
}

/// a texelFetch past a texture's edge: GL leaves it undefined, so the body
/// never does one; the twin answers zero
fn fetch(t: &[UVec4], i: u32) -> UVec4 {
    t.get(i as usize).copied().unwrap_or([0; 4])
}

// ---- the body ---------------------------------------------------------------

pub fn hash(x: u32) -> u32 {
    crate::block::hash(x)
}
fn h01(x: u32) -> f32 {
    (hash(x) >> 8) as f32 * (1.0 / 16777216.0)
}

fn hash2(ix: i32, iy: i32, seed: u32) -> f32 {
    h01((ix as u32).wrapping_add(hash((iy as u32).wrapping_add(seed))))
}
fn vnoise(p: Vec2, seed: u32) -> f32 {
    let i = [p[0].floor(), p[1].floor()];
    let f = [p[0] - i[0], p[1] - i[1]];
    let u = [f[0] * f[0] * (3.0 - 2.0 * f[0]), f[1] * f[1] * (3.0 - 2.0 * f[1])];
    let k = [i[0] as i32, i[1] as i32];
    let a = hash2(k[0], k[1], seed);
    let b = hash2(k[0] + 1, k[1], seed);
    let c = hash2(k[0], k[1] + 1, seed);
    let d = hash2(k[0] + 1, k[1] + 1, seed);
    mix(mix(a, b, u[0]), mix(c, d, u[0]), u[1])
}

struct Res<'a> {
    u: &'a TwU,
    t: &'a Textures,
}

impl Res<'_> {
    fn artrec(&self, a: u32) -> UVec4 {
        fetch(&self.t.arttab, a.min((self.u.counts[1] as u32).wrapping_sub(1)))
    }
    fn sprrec(&self, s: u32) -> UVec4 {
        fetch(&self.t.sprtab, s.min((self.u.counts[2] as u32).wrapping_sub(1)))
    }
    fn room(&self, id: u32) -> UVec4 {
        fetch(&self.t.rooms, (id & 127) + (id >> 7) * 128)
    }
    fn cellat(&self, c: [i32; 2]) -> UVec4 {
        let u = self.u;
        if c[0] < 0 || c[1] < 0 || c[0] >= u.grid[0] || c[1] >= u.grid[1] {
            return [0; 4];
        }
        let k = self.t.cells[c[1] as usize * STRIDE + c[0] as usize];
        let mut k = [k[0] as u32, k[1] as u32, k[2] as u32, k[3] as u32];
        if k[0] >= u.counts[0] as u32 {
            k[0] = 0; // nothing is trusted
        }
        k
    }

    /// an art entry's rectangle, frame fr of it: x, y, w, h in texels
    fn rect(a: UVec4, fr: u32) -> UVec4 {
        let n = (a[3] >> 16).max(1);
        let w = (a[1] & 0xffff) / n;
        [(a[0] & 0xffff) + (fr % n) * w, a[0] >> 16, w, a[1] >> 16]
    }
    /// the picture (premultiplied) at p, texels in r, kept inside r at this lod
    fn atl(&self, r: UVec4, p: Vec2, lod: f32) -> Vec4 {
        let e = 0.5 * lod.ceil().exp2();
        let q = [
            clampf(p[0], e, (r[2] as f32 - e).max(e)) + r[0] as f32,
            clampf(p[1], e, (r[3] as f32 - e).max(e)) + r[1] as f32,
        ];
        self.t.albedo.lod([q[0] / self.u.sizes[0] as f32, q[1] / self.u.sizes[1] as f32], lod)
    }
    fn atl_emis(&self, r: UVec4, p: Vec2, lod: f32) -> Vec4 {
        let e = 0.5 * lod.ceil().exp2();
        let q = [
            clampf(p[0], e, (r[2] as f32 - e).max(e)) + r[0] as f32,
            clampf(p[1], e, (r[3] as f32 - e).max(e)) + r[1] as f32,
        ];
        self.t.emis.lod([q[0] / self.u.sizes[0] as f32, q[1] / self.u.sizes[1] as f32], (lod - self.u.mode[1] as f32).max(0.0))
    }
    fn auxat(&self, r: UVec4, p: Vec2) -> [u32; 2] {
        let qx = (p[0].floor() as i32).clamp(0, r[2] as i32 - 1) + r[0] as i32;
        let qy = (p[1].floor() as i32).clamp(0, r[3] as i32 - 1) + r[1] as i32;
        let (w, h, px) = &self.t.aux;
        // a texelFetch past the edge is undefined; the twin answers zero
        if qx < 0 || qy < 0 || qx as usize >= *w || qy as usize >= *h {
            return [0, 0];
        }
        let v = px[qy as usize * w + qx as usize];
        [v[0] as u32, v[1] as u32]
    }
    /// textureLod(prof, (s, t), 0): R8, linear, repeat across, clamp down;
    /// t is always a row's centre, so only the hours are interpolated
    fn prof(&self, s: f32, t: f32) -> f32 {
        let u = s * 24.0 - 0.5;
        let i0 = u.floor();
        let a = u - i0;
        let i0 = (i0 as i32).rem_euclid(24) as usize;
        let i1 = (i0 + 1) % 24;
        let row = ((t * PROFILES as f32 - 0.5).round() as i32).clamp(0, PROFILES as i32 - 1) as usize;
        let p = &self.t.prof[row];
        (1.0 - a) * (p[i0] as f32 / 255.0) + a * (p[i1] as f32 / 255.0)
    }
    /// sprite s, frame fr, at uv 0..1 in it
    fn spr(&self, s: u32, fr: u32, uv: Vec2, lod: f32, glow: &mut Vec4) -> Vec4 {
        let sr = self.sprrec(s);
        let n = (sr[3] >> 16).max(1);
        let sz = [(sr[1] & 0xffff) as f32, (sr[1] >> 16) as f32];
        let e = 0.5 * lod.ceil().exp2();
        let q = [
            clampf(clampf(uv[0], 0.0, 1.0) * sz[0], e, (sz[0] - e).max(e)) + ((sr[0] & 0xffff) + (fr % n) * (sr[1] & 0xffff)) as f32,
            clampf(clampf(uv[1], 0.0, 1.0) * sz[1], e, (sz[1] - e).max(e)) + (sr[0] >> 16) as f32,
        ];
        let t = [q[0] / self.u.sizes[2] as f32, q[1] / self.u.sizes[3] as f32];
        // the emission picture, or for a person the tint layer in its place
        *glow = if sr[3] & (A_EMIT | SPR_TINT) != 0 { self.t.spremis.lod(t, lod) } else { [0.0; 4] };
        self.t.sprites.lod(t, lod)
    }

    fn over(under: Vec3, top: Vec4) -> Vec3 {
        [top[0] + under[0] * (1.0 - top[3]), top[1] + under[1] * (1.0 - top[3]), top[2] + under[2] * (1.0 - top[3])]
    }

    // ---- light
    fn night_alpha(hour: f32) -> f32 {
        crate::pack::night_alpha(hour)
    }
    fn outdoors(&self, c: Vec3, glow: Vec3) -> Vec3 {
        if self.u.counts[3] == 0 {
            return c;
        }
        let d = 1.0 - self.u.misc[0];
        [c[0] * self.u.amb[0] + glow[0] * d, c[1] * self.u.amb[1] + glow[1] * d, c[2] * self.u.amb[2] + glow[2] * d]
    }

    fn lamp(&self, rm: UVec4, group: u32) -> f32 {
        let u = self.u;
        let st = rm[2] & 0xffff;
        if st & (S_DARK | S_CLOSED) != 0 {
            return 0.0;
        }
        if st & S_LIT != 0 {
            return 1.0;
        }
        let chance = self.prof((u.clock[0] + 0.5) / 24.0, (((rm[1] >> 16) & 15) as f32 + 0.5) / 16.0);
        let k = rm[3] ^ group.wrapping_mul(0x9e37_79b9) ^ (u.clock[2] as u32).wrapping_mul(0x85eb_ca6b);
        if h01(k) < chance {
            1.0
        } else {
            0.0
        }
    }

    // ---- the sky and the ground
    fn grad3(a: Vec3, b: Vec3, c: Vec3, mid: f32, t: f32) -> Vec3 {
        if t < mid {
            mix3(a, b, t / mid)
        } else {
            mix3(b, c, (t - mid) / (1.0 - mid))
        }
    }
    fn sky(&self, m: Vec2) -> Vec3 {
        let u = self.u;
        let ground = u.grid[2] as f32 * CELL[1];
        let t = clampf(m[1] / ground, 0.0, 1.0);
        if u.counts[3] == 0 {
            let d = Self::grad3([0.102, 0.227, 0.431], [0.357, 0.659, 0.831], [0.706, 0.867, 0.941], 0.6, t);
            let n = Self::grad3([0.016, 0.035, 0.102], [0.039, 0.071, 0.208], [0.055, 0.122, 0.290], 0.5, t);
            return mix3(d, n, Self::night_alpha(u.clock[0]));
        }
        let mut c = mix3(rgb(u.zen), rgb(u.hor), t * t);
        // stars: about one in 256 squares of 10 units, a pixel or two across
        let sq = [(m[0] / 10.0).floor() as i32, (m[1] / 10.0).floor() as i32];
        let h = hash((sq[0] as u32).wrapping_mul(73_856_093) ^ (sq[1] as u32).wrapping_mul(19_349_663));
        if h & 255 == 0 {
            let sp = [
                (sq[0] as f32 + ((h >> 8) & 255) as f32 / 255.0) * 10.0,
                (sq[1] as f32 + ((h >> 16) & 255) as f32 / 255.0) * 10.0,
            ];
            let d = length2([m[0] - sp[0], m[1] - sp[1]]) * u.grid[3] as f32 / u.view[2];
            c = add3(c, scale3([0.90, 0.94, 1.00], u.misc[1] * (1.0 - smoothstep(0.6, 1.6, d))));
        }
        if u.counts[3] >= 2 && u.misc[2] > 0.0 {
            // the sun
            let k = clampf((u.clock[0] - 6.0) / 13.0, 0.0, 1.0);
            let sun = [1500.0 * (0.1 + 0.8 * k), ground * (0.9 - 0.7 * (3.141_592_65 * k).sin())];
            let d = length2([m[0] - sun[0], m[1] - sun[1]]);
            let disc = 1.0 - smoothstep(14.0, 15.0, d);
            let glow = scale3(scale3([1.0, 0.80, 0.50], 0.25), (-d / 75.0).exp2());
            c = add3(c, scale3(add3(scale3([1.0, 0.95, 0.80], disc), glow), u.misc[2]));
        }
        // the skylines, far then near, standing on the ground
        for s in ART_SKY_FAR..=ART_SKY_NEAR {
            let ar = self.artrec(s);
            let r = Self::rect(ar, 0);
            let sc = u.grid[3] as f32;
            let hgt = r[3] as f32 / sc;
            let y = m[1] - (ground - hgt);
            if y >= 0.0 {
                let p = [gmod(m[0] * sc, r[2] as f32), y * sc];
                let g = if ar[3] & A_EMIT != 0 { self.atl_emis(r, p, u.view[3]) } else { [0.0; 4] };
                let a = self.atl(r, p, u.view[3]);
                let o = self.outdoors(rgb(a), rgb(g));
                c = [o[0] + c[0] * (1.0 - a[3]), o[1] + c[1] * (1.0 - a[3]), o[2] + c[2] * (1.0 - a[3])];
            }
        }
        c
    }

    /// a back or front piece: grid-anchored, repeating every its own width;
    /// GROUND pieces stand on the ground line
    fn piece(&self, a: u32, m: Vec2, c: [i32; 2], glow: &mut Vec3) -> Vec4 {
        let u = self.u;
        let ar = self.artrec(a);
        if ar[3] & A_SOLID != 0 {
            *glow = [0.0; 3];
            return unpack8(ar[0]);
        }
        let fr = if ar[3] & A_ANIM != 0 {
            ((u.clock[1] * 8.0) as u32).wrapping_add(hash((c[0] as u32).wrapping_mul(31).wrapping_add(c[1] as u32)))
        } else {
            0
        };
        let r = Self::rect(ar, fr);
        let s = u.grid[3] as f32;
        let p = [
            gmod(m[0] * s, r[2] as f32),
            if ar[3] & A_GROUND != 0 {
                m[1] * s - (u.grid[2] as f32 * CELL[1] * s - r[3] as f32)
            } else {
                gmod(m[1] * s, r[3] as f32)
            },
        ];
        *glow = if ar[3] & A_EMIT != 0 { rgb(self.atl_emis(r, p, u.view[3])) } else { [0.0; 3] };
        self.atl(r, p, u.view[3])
    }

    // ---- a room
    fn roaches(&self, rm: UVec4, l: Vec2, size: Vec2, floor_y: f32, mut col: Vec3) -> Vec3 {
        let u = self.u;
        let n = 6.min(2 + ((rm[0] >> 16) & 511) as i32 / 4);
        for k in 0..6 {
            if k >= n {
                break;
            }
            let hk = hash(rm[3].wrapping_add((k as u32).wrapping_mul(0x9e37_79b9)));
            let dir = if hk & 1 != 0 { 1.0 } else { -1.0 };
            let speed = 1.5 + ((hk >> 1) & 7) as f32 * 0.25;
            let bx = gmod((hk >> 8) as f32 + dir * speed * u.clock[1], size[0] - 4.0) + 2.0;
            let by = floor_y - 0.5 - (k & 1) as f32;
            let mut d = [(l[0] - (bx - 1.25)) / 2.5, (l[1] - (by - 2.0)) / 2.0];
            if d[0] >= 0.0 && d[1] >= 0.0 && d[0] < 1.0 && d[1] < 1.0 {
                if dir < 0.0 {
                    d[0] = 1.0 - d[0];
                }
                let mut g = [0.0; 4];
                let b = self.spr(SPR_ROACH, ((u.clock[1] * 9.0) as u32).wrapping_add(k as u32), d, u.view[3], &mut g);
                col = Self::over(col, b);
            }
        }
        col
    }

    fn shade_room(&self, rm: UVec4, m: Vec2, c: [i32; 2]) -> Vec4 {
        let u = self.u;
        let x0 = (rm[0] & 511) as f32;
        let yt = ((rm[0] >> 9) & 127) as f32;
        let wc = ((rm[0] >> 16) & 511) as f32;
        let hf = ((rm[0] >> 25) & 7).max(1) as f32;
        let st = rm[2] & 0xffff;
        let occ = (rm[2] >> 16) & 255;
        let size = [wc * CELL[0], hf * CELL[1]];
        let l = [m[0] - x0 * CELL[0], m[1] - yt * CELL[1]];
        let fy = gmod(l[1], CELL[1]);
        let q = u.counts[3];
        let (lod, s) = (u.view[3], u.grid[3] as f32);

        // the structure: classic leaves upstream's gaps (what is behind shows)
        let (gx, gy) = (if q == 0 { 1.0 } else { 0.75 }, 1.0);
        if l[0] >= size[0] - gx || fy >= CELL[1] - gy {
            if q == 0 {
                return [0.0; 4];
            }
            let s = if fy >= CELL[1] - gy { SLAB } else { PART };
            let k = mix3([0.45; 3], ONE, u.misc[0]);
            return [s[0] * k[0], s[1] * k[1], s[2] * k[2], 1.0];
        }

        let ar = self.artrec(rm[1] & 0xffff);
        let mut col: Vec3;
        let mut glow = [0.0; 3];
        let (mut mat, mut grp, mut ht) = (0u32, 0u32, 0u32);
        let mut r: UVec4 = [0; 4];
        let mut p: Vec2 = [0.0; 2];
        if ar[3] & A_SOLID != 0 {
            col = rgb(unpack8(ar[0]));
        } else {
            let fr = if ar[3] & A_ANIM != 0 || st & S_SHOWING != 0 { (u.clock[1] * 6.0) as u32 } else { 0 };
            r = Self::rect(ar, fr);
            let period = ar[2] >> 16;
            p = if ar[3] & A_REPEAT != 0 {
                [gmod(l[0] * s, if period != 0 { period } else { r[2] } as f32), l[1] / size[1] * r[3] as f32]
            } else {
                [l[0] / size[0] * r[2] as f32, l[1] / size[1] * r[3] as f32]
            };
            let mut a = self.atl(r, p, lod);
            if ar[3] & A_AUX != 0 {
                let x = self.auxat(r, p);
                let slot = x[0] >> 4;
                mat = x[0] & 15;
                grp = x[1] & 15;
                ht = x[1] >> 4;
                let alt = ar[2] & 0xffff;
                if slot != 0 && alt != 0 && occ as f32 > h01(rm[3] ^ slot.wrapping_mul(0x27d4_eb2f)) * 255.0 {
                    a = self.atl(Self::rect(self.artrec(alt), fr), p, lod); // someone is there
                }
            }
            col = rgb(a);
            if ar[3] & A_EMIT != 0 {
                glow = rgb(self.atl_emis(r, p, lod));
            }
        }
        if q >= 1 {
            let on = self.lamp(rm, if ar[3] & A_LAMP_ALL != 0 { 0 } else { grp });
            let day = u.misc[0];
            let base = col;
            let k = mixv(mix3(DARK_IN, ONE, day), mix3(LAMP, ONE, day), [on; 3]);
            col = add3(mul3(base, k), scale3(glow, on * (1.0 - day)));
            if mat == M_GLASS {
                col = mix3(col, self.sky(m), 0.7 - 0.35 * on);
            }
            if mat == M_SCREEN && st & S_SHOWING != 0 {
                col = add3(col, scale3([0.6, 0.7, 0.9], 0.25 + 0.2 * vnoise([u.clock[1] * 4.0, x0], 51)));
            }
            if q >= 2 {
                // the ceiling and the corners darker; objects throw a short
                // shadow down and to the right from the ceiling's lamps
                let ao = (1.0 - 0.22 * (1.0 - smoothstep(0.0, 2.5, fy)))
                    * (1.0 - 0.18 * (1.0 - smoothstep(0.0, 2.0, l[0].min(size[0] - gx - l[0]))));
                let mut sh: f32 = 0.0;
                if ar[3] & A_AUX != 0 {
                    for k in 1..=3 {
                        let d = k as f32 * lod.exp2();
                        let y = self.auxat(r, [p[0] - 1.0 * d, p[1] - 2.0 * d]);
                        if (y[1] >> 4) as f32 > ht as f32 + k as f32 {
                            sh = sh.max(1.0 - 0.25 * k as f32);
                        }
                    }
                }
                col = scale3(col, ao * (1.0 - 0.35 * sh));
                // lit neighbours' light through the partitions, at night
                if day < 0.95 {
                    let kl = self.cellat([x0 as i32 - 1, c[1]]);
                    let kr = self.cellat([(x0 + wc) as i32, c[1]]);
                    let mut sp = 0.0;
                    if kl[0] != 0 {
                        sp += self.lamp(self.room(kl[0]), 0) * (-l[0] / 6.0).exp2();
                    }
                    if kr[0] != 0 {
                        sp += self.lamp(self.room(kr[0]), 0) * (-(size[0] - l[0]) / 6.0).exp2();
                    }
                    col = add3(col, scale3(scale3(scale3(mul3(base, LAMP), sp), 0.35), 1.0 - day));
                }
                // the glass in front of it all: the sky, faintly, in streaks
                let w = fract((m[0] + m[1] * 0.35) / 225.0);
                let streak = smoothstep(0.40, 0.45, w) - smoothstep(0.52, 0.60, w);
                col = mix3(col, rgb(u.hor), u.misc[3] * (0.4 + 0.6 * streak));
            }
        }

        if st & S_STUCK != 0 {
            let g = dot3(col, LUMA) * 0.85;
            col = [g, g, g];
        }
        let sign = rm[2] >> 24;
        if sign != 0 && l[1] < CELL[1] {
            let mut g = [0.0; 4];
            col = Self::over(col, self.spr(sign, 0, [l[0] / size[0], l[1] / CELL[1]], lod, &mut g));
        }
        if st & S_INFESTED != 0 {
            col = self.roaches(rm, l, size, size[1] - gy, col);
        }
        let ev = (rm[1] >> 20) & 3;
        if u.mode[0] == 1 && ev != 0 {
            col = Self::over(col, u.pal[ev as usize]);
        }
        if u.mode[0] == 2 {
            col = Self::over(col, u.pal[4 + ((rm[1] >> 24) & 255) as usize * 12 / 256]);
        }
        if st & S_SELECTED != 0 {
            let e = 2.0 * u.view[2] / s;
            if l[0] < e || l[1] < e || l[0] > size[0] - gx - e || l[1] > size[1] - gy - e {
                col = rgb(u.ocol);
            }
        }
        [col[0], col[1], col[2], 1.0]
    }

    /// rich fire: flames rising through the fire pieces, and their glow
    fn burn(&self, col: Vec3, m: Vec2, fy: f32) -> Vec3 {
        let u = self.u;
        let up = 1.0 - fy / CELL[1];
        let n = vnoise([m[0] / 3.0, m[1] / 3.0 + u.clock[1] * 2.5], 61) * 0.7
            + vnoise([m[0] / 1.25, m[1] / 1.25 + u.clock[1] * 5.0], 62) * 0.3;
        let flame = smoothstep(0.35, 0.75, n * (1.25 - up));
        let hot = mix3([0.85, 0.20, 0.05], [1.0, 0.85, 0.35], flame);
        add3(mix3(scale3(col, 0.30), hot, flame), scale3([0.35, 0.10, 0.02], 1.0 - up))
    }

    // ---- the fragment
    fn field(&self, f: Vec2) -> Vec4 {
        let u = self.u;
        let world = [u.grid[0] as f32 * CELL[0], u.grid[1] as f32 * CELL[1]];
        let m = [f[0] / u.box_[2] * world[0], f[1] / u.box_[3] * world[1]];
        if m[0] < 0.0 || m[1] < 0.0 || m[0] >= world[0] || m[1] >= world[1] {
            return [0.0; 4];
        }
        let c = [(m[0] / CELL[0]).floor() as i32, (m[1] / CELL[1]).floor() as i32];
        let k = self.cellat(c);
        let mut col = if c[1] < u.grid[2] { self.sky(m) } else { [0.24, 0.13, 0.06] };
        let mut g = [0.0; 3];
        if k[1] != 0 {
            let b = self.piece(k[1], m, c, &mut g);
            let o = self.outdoors(rgb(b), g);
            col = [o[0] + col[0] * (1.0 - b[3]), o[1] + col[1] * (1.0 - b[3]), o[2] + col[2] * (1.0 - b[3])];
        }
        if k[0] != 0 {
            let r = self.shade_room(self.room(k[0]), m, c);
            col = mix3(col, rgb(r), r[3]);
        }
        if k[2] != 0 {
            let b = self.piece(k[2], m, c, &mut g);
            if u.counts[3] == 0 && self.artrec(k[2])[3] & A_FIRE != 0 {
                col = mix3(col, [0.87, 0.20, 0.20], 0.5 * b[3]);
            } else if u.counts[3] >= 2 {
                col = mix3(col, self.burn(col, m, gmod(m[1], CELL[1])), b[3] * 0.6);
            }
            let o = self.outdoors(rgb(b), g);
            col = Self::over(col, [o[0], o[1], o[2], b[3]]);
        }
        let o = u.outline;
        if o[2] > o[0] && c[0] >= o[0] && c[1] >= o[1] && c[0] < o[2] && c[1] < o[3] {
            let kk = [u.box_[2] / world[0], u.box_[3] / world[1]];
            let lo = [(m[0] - o[0] as f32 * CELL[0]) * kk[0], (m[1] - o[1] as f32 * CELL[1]) * kk[1]];
            let hi = [(o[2] as f32 * CELL[0] - m[0]) * kk[0], (o[3] as f32 * CELL[1] - m[1]) * kk[1]];
            if lo[0].min(lo[1]).min(hi[0].min(hi[1])) < 2.0 {
                col = rgb(u.ocol);
            }
        }
        [clampf(col[0], 0.0, 1.0), clampf(col[1], 0.0, 1.0), clampf(col[2], 0.0, 1.0), 1.0]
    }
}

// ---- P2, the fixtures (design 7.6, 7.7) ----------------------------------

/// tw_fix_vs: a fixture's quad, in units, and the record
fn fix_quad(u: &TwU, r: UVec4) -> (Vec2, Vec2) {
    let over_ = ((r[2] >> 8) & 255) as f32;
    let o = [(r[0] & 511) as f32 * CELL[0], ((r[0] >> 9) & 127) as f32 * CELL[1] - over_];
    let sz = [((r[0] >> 16) & 255) as f32 * CELL[0], ((r[0] >> 24) & 127) as f32 * CELL[1] + over_];
    let _ = u;
    (o, sz)
}

impl Res<'_> {
    /// tw_fix: shafts as body, top, pit and landings; stairs; escalators
    fn fixture(&self, f: UVec4, lp: Vec2) -> Vec4 {
        let u = self.u;
        let kind = (f[1] >> 16) & 255;
        let base = f[1] & 0xffff;
        let (s, lod) = (u.grid[3] as f32, u.view[3]);
        let over_ = ((f[2] >> 8) & 255) as f32;
        if kind <= 2 || kind == FIX_CARLIFT {
            if lp[1] < over_ {
                // the top, above the highest floor, standing on it
                let r = Self::rect(self.artrec(base + 1), 0);
                let p = [lp[0] * s, (lp[1] - over_) * s + r[3] as f32];
                let c = self.atl(r, p, lod);
                let o = self.outdoors(rgb(c), [0.0; 3]);
                return [o[0], o[1], o[2], c[3]];
            }
            let top = ((f[0] >> 9) & 127) as i32;
            let rows = ((f[0] >> 24) & 127) as i32;
            let row = top + ((lp[1] - over_) / CELL[1]).floor() as i32;
            let fy = gmod(lp[1] - over_, CELL[1]);
            let p = [lp[0] * s, fy * s];
            if row >= top + rows - 1 {
                // the pit, under the lowest floor
                let r = Self::rect(self.artrec(base + 2), 0);
                if p[1] >= r[3] as f32 {
                    return [0.0; 4];
                }
                let c = self.atl(r, p, lod);
                let o = self.outdoors(rgb(c), [0.0; 3]);
                return [o[0], o[1], o[2], c[3]];
            }
            let st = self.t.stops[(f[2] & 63) as usize][row.clamp(0, 127) as usize] as u32;
            let a = if st & 1 != 0 { base + 4 + ((st >> 4) & 3) } else { base + 3 };
            let body = self.atl(Self::rect(self.artrec(base), 0), p, lod);
            let r = Self::rect(self.artrec(a), 0);
            let front = self.atl(r, p, lod);
            let g = if self.artrec(a)[3] & A_EMIT != 0 { self.atl_emis(r, p, lod) } else { [0.0; 4] };
            let calling = if st & 6 != 0 { 1.0 } else { 0.25 };
            let gc = [g[0] * calling, g[1] * calling, g[2] * calling];
            let fo = self.outdoors(rgb(front), gc);
            let c = Self::over(self.outdoors(rgb(body), [0.0; 3]), [fo[0], fo[1], fo[2], front[3]]);
            return [c[0], c[1], c[2], body[3].max(front[3])];
        }
        let ar = self.artrec(base);
        let fr = if ar[3] & A_ANIM != 0 { (u.clock[1] * 8.0) as u32 } else { 0 };
        let r = Self::rect(ar, fr);
        let sz = [((f[0] >> 16) & 255) as f32 * CELL[0], ((f[0] >> 24) & 127) as f32 * CELL[1] + over_];
        let p = [lp[0] / sz[0] * r[2] as f32, lp[1] / sz[1] * r[3] as f32];
        let c = self.atl(r, p, lod);
        let g = if ar[3] & A_EMIT != 0 { self.atl_emis(r, p, lod) } else { [0.0; 4] };
        let o = self.outdoors(rgb(c), rgb(g));
        [o[0], o[1], o[2], c[3]]
    }

    /// tw_mov: a mover's fragment, premultiplied, or None where discarded
    fn mover(&self, mv: Mover, sp: UVec4, uv: Vec2, at: Vec2) -> Option<Vec4> {
        let u = self.u;
        if mv.spr & M_BEHIND != 0 {
            let c = [(at[0] / CELL[0]).floor() as i32, (at[1] / CELL[1]).floor() as i32];
            if c[0] >= 0 && c[1] >= 0 && c[0] < u.grid[0] && c[1] < u.grid[1]
                && self.t.cells[c[1] as usize * STRIDE + c[0] as usize][0] != 0
            {
                return None;
            }
        }
        let fr = if sp[3] & A_ANIM != 0 { (u.clock[1] * 8.0) as u32 } else { (mv.param & 15) as u32 };
        let mut g = [0.0; 4];
        let mut a = self.spr((mv.spr & M_SPRITE) as u32, fr, uv, u.view[3], &mut g);
        if sp[3] & SPR_TINT != 0 {
            // a person: the tint layer's weights put the clothes' and skin's colours in
            let w = g;
            g = [0.0; 4];
            if mv.param & P_TINTED != 0 {
                let top = TOPS[((mv.param >> 4) & 15) as usize];
                let legs = LEGS[((mv.param >> 8) & 3) as usize];
                let skin = SKINS[((mv.param >> 10) & 3) as usize];
                let k = 1.0 - w[0] - w[1] - w[2];
                for c in 0..3 {
                    a[c] = a[c] * k + (top[c] * w[0] + skin[c] * w[1] + legs[c] * w[2]) * a[3];
                }
            }
        }
        if a[3] <= 0.0 && g[3] <= 0.0 {
            return None;
        }
        let c = if mv.spr & M_LIT != 0 && u.counts[3] >= 1 {
            // lit from inside: a car
            add3(mul3(rgb(a), mix3(LAMP, ONE, u.misc[0])), scale3(rgb(g), 1.0 - u.misc[0]))
        } else {
            self.outdoors(rgb(a), rgb(g))
        };
        Some([c[0], c[1], c[2], a[3]])
    }
}

/// a mover's param: bits 0-3 its frame, 4-7 its top's colour, 8-9 its
/// legs', 10-11 its skin's, 15 tinted (design 5.8)
pub const P_TINTED: u16 = 0x8000;
fn hexrgb(v: u32) -> Vec3 {
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}
/// the people's colours (art/src/people/NOTES.md): tops 0-4 the calm
/// public's, 5-7 staff (housekeeping, security, medical), 8 stressed, 9 very
static TOPS: std::sync::LazyLock<[Vec3; 16]> = std::sync::LazyLock::new(|| {
    let mut t = [[0.0; 3]; 16];
    for (i, v) in [0x2f5d50, 0x3b5b8a, 0x6b5a7e, 0x7a6a4f, 0x4a4f57, 0xb9a3d6, 0x2f4a7a, 0xe9eef2, 0xff5fa2, 0xd81919]
        .iter()
        .enumerate()
    {
        t[i] = hexrgb(*v);
    }
    t
});
static LEGS: std::sync::LazyLock<[Vec3; 4]> =
    std::sync::LazyLock::new(|| [hexrgb(0x3b4250), hexrgb(0x2e3440), hexrgb(0xa33a6c), hexrgb(0x8a1010)]);
static SKINS: std::sync::LazyLock<[Vec3; 4]> =
    std::sync::LazyLock::new(|| [hexrgb(0xf1c27d), hexrgb(0xe0ac69), hexrgb(0xc68642), hexrgb(0x8d5524)]);

/// a quad from (t0) to (t1), target pixels, over the clip: each covered
/// pixel's centre (x0 <= x < x1, as the GPU's fill rule has an axis-aligned
/// quad) and the fraction of the quad it lies at
fn raster(t0: Vec2, t1: Vec2, clip: [u32; 4], mut f: impl FnMut(usize, usize, Vec2)) {
    let [cx, cy, cw, ch] = clip;
    let xs = ((t0[0] - 0.5).ceil().max(cx as f32) as i64, ((t1[0] - 0.5).ceil()).min((cx + cw) as f32) as i64);
    let ys = ((t0[1] - 0.5).ceil().max(cy as f32) as i64, ((t1[1] - 0.5).ceil()).min((cy + ch) as f32) as i64);
    for py in ys.0..ys.1 {
        for px in xs.0..xs.1 {
            let k = [(px as f32 + 0.5 - t0[0]) / (t1[0] - t0[0]), (py as f32 + 0.5 - t0[1]) / (t1[1] - t0[1])];
            f((px - cx as i64) as usize, (py - cy as i64) as usize, k);
        }
    }
}

/// premultiplied over an 8-bit target, as the GPU blends: ONE, ONE_MINUS_SRC_ALPHA
fn blend(px: &mut [u8], c: Vec4) {
    let b = |x: f32| (clampf(x, 0.0, 1.0) * 255.0 + 0.5).floor() as u8;
    for k in 0..3 {
        px[k] = b(c[k] + px[k] as f32 / 255.0 * (1.0 - c[3]));
    }
}

/// What to draw: the tower in a box on a target, the part of the target in
/// `clip` (x, y, w, h target pixels), `secs` into the hour.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub box_: [f32; 4],
    pub target: [u32; 2],
    pub clip: [u32; 4],
    pub secs: f32,
}

impl View {
    /// the whole tower at `zoom` target pixels a unit, all of it drawn
    pub fn whole(zoom: f32) -> View {
        let (w, h) = ((GRID_W * UNIT_W) as f32 * zoom, (GRID_H * UNIT_H) as f32 * zoom);
        View { box_: [0.0, 0.0, w, h], target: [w as u32, h as u32], clip: [0, 0, w as u32, h as u32], secs: 0.0 }
    }
    /// only cells x0..x1, rows y0..y1 of it
    pub fn cells(mut self, x0: u32, y0: u32, x1: u32, y1: u32) -> View {
        let k = [self.box_[2] / (GRID_W * UNIT_W) as f32, self.box_[3] / (GRID_H * UNIT_H) as f32];
        let px = |c: u32, i: usize| ((c as f32 * if i == 0 { 4.0 } else { 16.0 }) * k[i]).round() as u32;
        self.clip = [px(x0, 0), px(y0, 1), px(x1, 0) - px(x0, 0), px(y1, 1) - px(y0, 1)];
        self
    }
}

/// The three passes of a live block and an art block, at the block's
/// quality: the clip's pixels, RGBA8.  Where the shader discards (past the
/// tower), the live block's back colour.
pub fn render(t: &Tower, art: &[u8], tex: &Textures, v: &View) -> Result<Vec<u8>, String> {
    if t.quality > 2 {
        return Err(format!("quality {}: there are three, 0 to 2", t.quality));
    }
    let a = TowerArt::from_bytes(art);
    let u = uniforms(t, &a, v.box_[0], v.box_[1], v.box_[2], v.box_[3], v.target[0] as f32, v.target[1] as f32, v.secs);
    let r = Res { u: &u, t: tex };
    let [cx, cy, cw, ch] = v.clip;
    let back = t.back_rgba.to_le_bytes();
    let mut out = vec![0u8; cw as usize * ch as usize * 4];
    let world = [(GRID_W * UNIT_W) as f32, (GRID_H * UNIT_H) as f32];
    let to_target = |p: Vec2| [u.box_[0] + p[0] / world[0] * u.box_[2], u.box_[1] + p[1] / world[1] * u.box_[3]];
    let b = |x: f32| (clampf(x, 0.0, 1.0) * 255.0 + 0.5).floor() as u8;
    for y in 0..ch {
        for x in 0..cw {
            let f = [(cx + x) as f32 + 0.5 - u.box_[0], (cy + y) as f32 + 0.5 - u.box_[1]];
            let c = r.field(f);
            let i = (y as usize * cw as usize + x as usize) * 4;
            let px = if c[3] == 0.0 { [back[0], back[1], back[2], 255] } else { [b(c[0]), b(c[1]), b(c[2]), 255] };
            out[i..i + 4].copy_from_slice(&px);
        }
    }
    // P2, the fixtures, over the field
    for i in 0..(t.nfixtures as usize).min(FIXTURES) {
        let f = tex.fix[i];
        let (o, sz) = fix_quad(&u, f);
        let (t0, t1) = (to_target(o), to_target([o[0] + sz[0], o[1] + sz[1]]));
        raster(t0, t1, v.clip, |x, y, k| {
            let c = r.fixture(f, [k[0] * sz[0], k[1] * sz[1]]);
            let at = (y * cw as usize + x) * 4;
            blend(&mut out[at..at + 4], c);
        });
    }
    // P3, the movers, over them, in list order
    for i in 0..(t.nmovers as usize).min(MOVERS) {
        let mv = tex.movers[i];
        let sp = r.sprrec((mv.spr & M_SPRITE) as u32);
        let feet = [mv.x as f32 * 0.25, mv.y as f32 * 0.25];
        let sz = [(sp[2] & 0xffff) as f32 * 0.25, (sp[2] >> 16) as f32 * 0.25];
        let p0 = [feet[0] - 0.5 * sz[0], feet[1] - sz[1]];
        let (t0, t1) = (to_target(p0), to_target([p0[0] + sz[0], p0[1] + sz[1]]));
        raster(t0, t1, v.clip, |x, y, k| {
            let uv = [if mv.spr & M_FLIP != 0 { 1.0 - k[0] } else { k[0] }, k[1]];
            let at_ = [p0[0] + k[0] * sz[0], p0[1] + k[1] * sz[1]];
            if let Some(c) = r.mover(mv, sp, uv, at_) {
                let at = (y * cw as usize + x) * 4;
                blend(&mut out[at..at + 4], c);
            }
        });
    }
    Ok(out)
}

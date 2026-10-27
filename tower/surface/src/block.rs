//! rosgd/include/rosgd/tower.h in Rust: the live block, the art block's
//! header and the surface's 32 bytes, field for field and byte for byte.
//! tests/layout.rs checks every size and offset against the C compiler's.

use std::alloc::{alloc_zeroed, Layout};

pub const SURF_TOWER: u32 = 7;
pub const TOWER_MAGIC: u32 = 0x5257_4F54; // "TOWR"
pub const TOWER_ART_MAGIC: u32 = 0x4152_5754; // "TWRA"
pub const TOWER_VERSION: u32 = 1;

pub const GRID_W: usize = 375;
pub const GRID_H: usize = 120;
pub const GROUND: usize = 110;
pub const STRIDE: usize = 384;
pub const ROOMS: usize = 16384;
pub const FIXTURES: usize = 512;
pub const SHAFTS: usize = 64;
pub const ROWS: usize = 128;
pub const MOVERS: usize = 8192;
pub const ART: usize = 1024;
pub const SPRITES: usize = 1024;
pub const PROFILES: usize = 16;
pub const RAMP: usize = 48;
pub const UNIT_W: usize = 4;
pub const UNIT_H: usize = 16;

/// the live block's sections, for `gen`
pub const TW_CELLS: usize = 0;
pub const TW_ROOMS: usize = 1;
pub const TW_FIX: usize = 2;
pub const TW_STOPS: usize = 3;
pub const TW_PAL: usize = 4;
pub const TW_MOVERS: usize = 5;
pub const TW_NSECT: usize = 6;

pub const F_WEEKEND: u8 = 1;

/// a room's state bits (w2 bits 0-15)
pub const S_VACANT: u32 = 0x0001;
pub const S_SALE: u32 = 0x0002;
pub const S_STUCK: u32 = 0x0004;
pub const S_CLOSED: u32 = 0x0008;
pub const S_INFESTED: u32 = 0x0010;
pub const S_SHOWING: u32 = 0x0020;
pub const S_VIP: u32 = 0x0040;
pub const S_SELECTED: u32 = 0x0080;
pub const S_LIT: u32 = 0x0100;
pub const S_DARK: u32 = 0x0200;

/// lamp profiles
pub const P_NEVER: u32 = 0;
pub const P_ALWAYS: u32 = 1;
pub const P_OFFICE: u32 = 2;
pub const P_HOME: u32 = 3;
pub const P_HOTEL: u32 = 4;
pub const P_SHOP: u32 = 5;
pub const P_RESTAURANT: u32 = 6;
pub const P_FASTFOOD: u32 = 7;
pub const P_ENTERTAINMENT: u32 = 8;
pub const P_SERVICE: u32 = 9;
pub const P_PARKING: u32 = 10;
pub const P_LOBBY: u32 = 11;

/// an art entry's flags (w bits 0-15)
pub const A_REPEAT: u32 = 0x0001;
pub const A_LAMP_ALL: u32 = 0x0002;
pub const A_SOLID: u32 = 0x0004;
pub const A_GROUND: u32 = 0x0008;
pub const A_ANIM: u32 = 0x0010;
pub const A_EMIT: u32 = 0x0020;
pub const A_AUX: u32 = 0x0040;
pub const A_FIRE: u32 = 0x0080;

pub const ART_SKY_FAR: u32 = 1;
pub const ART_SKY_NEAR: u32 = 2;

pub const SPR_SIGNS: u32 = 1;
pub const SPR_NSIGNS: u32 = 255;
pub const SPR_ROACH: u32 = 256;
/// a sprite's flag (w bits 0-15): its "emission" picture is a person's tint
/// layer (art/src/people/NOTES.md), weights for the top, the skin, the legs
pub const SPR_TINT: u32 = 0x0040;

pub const FIX_STANDARD: u32 = 0;
pub const FIX_EXPRESS: u32 = 1;
pub const FIX_SERVICE: u32 = 2;
pub const FIX_STAIRS: u32 = 3;
pub const FIX_ESCALATOR: u32 = 4;
pub const FIX_CARLIFT: u32 = 5;

pub const STOP_SERVED: u8 = 0x01;
pub const STOP_UP: u8 = 0x02;
pub const STOP_DOWN: u8 = 0x04;
pub const STOP_DOORS: u8 = 0x30;

pub const M_SPRITE: u16 = 0x03ff;
pub const M_FLIP: u16 = 0x0400;
pub const M_BEHIND: u16 = 0x0800;
pub const M_LIT: u16 = 0x1000;

pub const TWA_F_AUX: u16 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub room: u16,
    pub back: u16,
    pub front: u16,
    pub flags: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Room {
    pub w: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fixture {
    pub w: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mover {
    pub x: u16,
    pub y: u16,
    pub spr: u16,
    pub param: u16,
}

/// struct ros_tower: the live block
#[repr(C)]
pub struct Tower {
    pub magic: u32,
    pub version: u32,
    pub frame: u32,
    pub art: u32,
    pub grid_w: u16,
    pub grid_h: u16,
    pub ground_row: u16,
    pub scale: u16,
    pub nrooms: u16,
    pub nfixtures: u16,
    pub nmovers: u16,
    pub quality: u8,
    pub rate: u8,
    pub view: u8,
    pub flags: u8,
    pub hour: u16,
    pub day: u16,
    pub outline: [i16; 4],
    pub reserved0: u16,
    pub outline_rgba: u32,
    pub back_rgba: u32,
    pub gen: [u32; 8],
    pub room_gen: [u32; 16],
    pub reserved: [u32; 10],
    pub cell: [[Cell; STRIDE]; GRID_H],
    pub room: [Room; ROOMS],
    pub fix: [Fixture; FIXTURES],
    pub stops: [[u8; ROWS]; SHAFTS],
    pub pal: [u32; 16],
    pub mover: [Mover; MOVERS],
}

/// struct ros_tower_art: the art block's head; the pictures follow it
#[repr(C)]
#[derive(Clone)]
pub struct TowerArt {
    pub magic: u32,
    pub version: u32,
    pub gen: u32,
    pub size: u32,
    pub scale: u16,
    pub atlas_w: u16,
    pub atlas_h: u16,
    pub spr_w: u16,
    pub spr_h: u16,
    pub nart: u16,
    pub nsprites: u16,
    pub emit_shift: u16,
    pub flags: u16,
    pub lang: u16,
    pub reserved: [u32; 7],
    pub art: [[u32; 4]; ART],
    pub spr: [[u32; 4]; SPRITES],
    pub ramp: [[[u8; 4]; RAMP]; 4],
    pub prof: [[[u8; 24]; PROFILES]; 2],
}

/// struct ros_surf_tower: Wimp_Extend &5204's R5
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfTower {
    pub backing: u32,
    pub offset: u32,
    pub size: u32,
    pub frame: u32,
    pub art_backing: u32,
    pub art_offset: u32,
    pub art_size: u32,
    pub art_gen: u32,
}

/// A zeroed `T` on the heap (every field of these structs is an integer, so
/// all-zero bytes are a valid value) without building it on the stack.
fn boxed_zeroed<T>() -> Box<T> {
    let layout = Layout::new::<T>();
    // SAFETY: T is one of this module's plain-integer repr(C) structs, for
    // which zero bytes are valid; the layout is T's own, and not zero-sized.
    unsafe {
        let p = alloc_zeroed(layout) as *mut T;
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(p)
    }
}

fn bytes_of<T>(v: &T) -> &[u8] {
    // SAFETY: plain-integer repr(C) structs with no padding (tests/layout.rs).
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

fn bytes_of_mut<T>(v: &mut T) -> &mut [u8] {
    // SAFETY: as bytes_of; any bytes are a valid value of these structs.
    unsafe { std::slice::from_raw_parts_mut(v as *mut T as *mut u8, std::mem::size_of::<T>()) }
}

impl Tower {
    /// An empty tower: magic, version, the grid's size and the scale set,
    /// everything else zero.
    pub fn new(scale: u16) -> Box<Tower> {
        let mut t = boxed_zeroed::<Tower>();
        t.magic = TOWER_MAGIC;
        t.version = TOWER_VERSION;
        t.grid_w = GRID_W as u16;
        t.grid_h = GRID_H as u16;
        t.ground_row = GROUND as u16;
        t.scale = scale;
        t.nrooms = 1;
        t.outline = [0, 0, 0, 0];
        t
    }
    pub fn zeroed() -> Box<Tower> {
        boxed_zeroed::<Tower>()
    }
    pub fn as_bytes(&self) -> &[u8] {
        bytes_of(self)
    }
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        bytes_of_mut(self)
    }
    /// A copy of a block's bytes (at least size_of::<Tower>() of them).
    pub fn from_bytes(b: &[u8]) -> Box<Tower> {
        let mut t = boxed_zeroed::<Tower>();
        let n = std::mem::size_of::<Tower>();
        t.as_bytes_mut().copy_from_slice(&b[..n]);
        t
    }
}

impl TowerArt {
    pub fn zeroed() -> Box<TowerArt> {
        boxed_zeroed::<TowerArt>()
    }
    pub fn as_bytes(&self) -> &[u8] {
        bytes_of(self)
    }
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        bytes_of_mut(self)
    }
    pub fn from_bytes(b: &[u8]) -> Box<TowerArt> {
        let mut t = boxed_zeroed::<TowerArt>();
        let n = std::mem::size_of::<TowerArt>();
        t.as_bytes_mut().copy_from_slice(&b[..n]);
        t
    }
    /// ros_tower_art_albedo .. ros_tower_art_size: where each picture starts
    pub fn at_albedo(&self) -> usize {
        std::mem::size_of::<TowerArt>()
    }
    pub fn at_emis(&self) -> usize {
        self.at_albedo() + self.atlas_w as usize * self.atlas_h as usize * 4
    }
    pub fn emis_w(&self) -> usize {
        (self.atlas_w >> self.emit_shift) as usize
    }
    pub fn emis_h(&self) -> usize {
        (self.atlas_h >> self.emit_shift) as usize
    }
    pub fn at_aux(&self) -> usize {
        self.at_emis() + self.emis_w() * self.emis_h() * 4
    }
    pub fn has_aux(&self) -> bool {
        self.flags & TWA_F_AUX != 0
    }
    pub fn at_sprites(&self) -> usize {
        self.at_aux() + if self.has_aux() { self.atlas_w as usize * self.atlas_h as usize * 2 } else { 0 }
    }
    pub fn at_spremis(&self) -> usize {
        self.at_sprites() + self.spr_w as usize * self.spr_h as usize * 4
    }
    pub fn total_size(&self) -> usize {
        self.at_spremis() + self.spr_w as usize * self.spr_h as usize * 4
    }
}

impl SurfTower {
    pub fn as_bytes(&self) -> &[u8] {
        bytes_of(self)
    }
}

// ---- the records' fields, packed as design 5.4-5.8 says -------------------

impl Room {
    /// w0 and w3 from the room's place: x0, top row, width in cells, height
    /// in floors; the seed a hash of x0 and the top row.
    pub fn place(x0: u32, ytop: u32, wc: u32, hf: u32) -> Room {
        let mut r = Room::default();
        r.w[0] = (x0 & 511) | (ytop & 127) << 9 | (wc & 511) << 16 | (hf & 7) << 25;
        r.w[3] = seed(x0, ytop);
        r
    }
    pub fn x0(&self) -> u32 {
        self.w[0] & 511
    }
    pub fn ytop(&self) -> u32 {
        (self.w[0] >> 9) & 127
    }
    pub fn width(&self) -> u32 {
        (self.w[0] >> 16) & 511
    }
    pub fn height(&self) -> u32 {
        (self.w[0] >> 25) & 7
    }
    pub fn set_art(&mut self, art: u32) {
        self.w[1] = (self.w[1] & !0xffff) | (art & 0xffff);
    }
    pub fn set_profile(&mut self, p: u32) {
        self.w[1] = (self.w[1] & !(15 << 16)) | (p & 15) << 16;
    }
    pub fn set_eval(&mut self, e: u32) {
        self.w[1] = (self.w[1] & !(3 << 20)) | (e & 3) << 20;
    }
    pub fn set_value(&mut self, v: u32) {
        self.w[1] = (self.w[1] & 0x00ff_ffff) | (v & 255) << 24;
    }
    pub fn set_state(&mut self, s: u32) {
        self.w[2] = (self.w[2] & !0xffff) | (s & 0xffff);
    }
    pub fn set_occupancy(&mut self, o: u32) {
        self.w[2] = (self.w[2] & !(255 << 16)) | (o & 255) << 16;
    }
    pub fn set_sign(&mut self, s: u32) {
        self.w[2] = (self.w[2] & 0x00ff_ffff) | (s & 255) << 24;
    }
}

/// a room's seed: the shaders' hash of its place
pub fn seed(x0: u32, ytop: u32) -> u32 {
    hash(x0.wrapping_mul(0x9e37_79b9) ^ hash(ytop.wrapping_add(0x632b_e5ab)))
}

/// the shaders' integer hash (the tile map's, unchanged)
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// an art or sprite table entry from its parts
pub fn art_entry(x: u32, y: u32, w: u32, h: u32, alt: u32, period: u32, flags: u32, frames: u32) -> [u32; 4] {
    [(x & 0xffff) | (y & 0xffff) << 16, (w & 0xffff) | (h & 0xffff) << 16, (alt & 0xffff) | (period & 0xffff) << 16, (flags & 0xffff) | (frames & 0xffff) << 16]
}

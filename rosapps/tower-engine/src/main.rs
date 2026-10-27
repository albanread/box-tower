// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Alban Read
//
// rosapps-tower -- !Tower's engine (ROSAPPS): the tower simulation
// (ports/tower/sim, a translation of tower-together, MIT) run on the box's
// Linux side and drawn by the presenter's GPU as the tower surface
// (ROS_SURF_TOWER, rosgd/include/rosgd/tower.h; ports/tower/docs/
// graphics-design.md).
//
// The broker starts it with two descriptors, as it starts the others:
//
//   3  a Unix stream socket: lines of text each way
//   4  the shared buffer (64 MB): the live block and the art block are
//      written into it at the offsets the broker gives ("tower LIVE ART")
//
// From the shell:
//   new / open PATH / save PATH / tower LIVE ART / scale PPMM100 / shown
//   press X Y MODS, drag, release, pointer X Y   (picture pixels)
//   key CODE MODS / tool NAME / menu M I / prop NAME VALUE / room H / quit
//   hover NAME / answer ID ACTION
//
// To the shell:
//   hello tower ... / doc 1 W H W H / title NAME / modified 0|1 / saved PATH
//   error TEXT / note TEXT / props ROW|... / inspect A|B|C|D / tick M I S
//   question ID|TITLE|TEXT|ACTIONS / centre X Y
//   tower (a frame of the live block is whole: the broker copies it)

use std::io::Write;
use std::os::raw::{c_int, c_void};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tower_sim::commands::SimCommand;
use tower_sim::index::TowerSim;
use tower_sim::resources::get_tile_star_requirement;
use tower_sim::snapshot::SimSnapshot;
use tower_surface::block::*;

mod lighting;
mod game;
mod movers;
mod publish;
use game::{Act, Ui, TOOLS, T_INSPECT};
use publish::{chunk_range, section_range, Art, Clock, Look, Publisher};

/// the shell's pixels to the millimetre at 100% (90 dpi)
const PPMM_100: f64 = 3.54;
const WORLD_W: f64 = (GRID_W * UNIT_W) as f64; // 1500 units
const WORLD_H: f64 = (GRID_H * UNIT_H) as f64; // 1920 units

pub const SPEEDS: [&str; 5] = ["Paused", "Slow", "Normal", "Fast", "Very fast"];
/// simulation ticks a second at each speed (a day is 2,600 ticks)
const TICKS_PER_SECOND: [u32; 5] = [0, 4, 20, 60, 200];
pub const STARS: [&str; 7] = ["", "*", "**", "***", "****", "*****", "Tower"];
const OUTLINE: u32 = 0xff00_d6ff; // 0xAABBGGRR: amber
const OUTLINE_BAD: u32 = 0xff33_33dd;

/// the fixture towers !Tower carries (upstream's trace fixtures' states, from
/// harness/dump-snapshots.mjs): the Tower menu's names and their files
const FIXTURES: [(&str, &str); 5] = [
    ("Mixed", "Mixed"),
    ("Sky office", "SkyOffice"),
    ("Dense hotel", "DenseHotel"),
    ("Offices", "Offices"),
    ("Dense office, dawn", "DenseOffice"),
];

// ---- the shared buffer -----------------------------------------------------------

struct Buffer {
    base: *mut u8,
    len: usize,
}

impl Buffer {
    fn map() -> Option<Buffer> {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(4, &mut st) } != 0 || st.st_size <= 0 {
            return None;
        }
        let len = st.st_size as usize;
        let p = unsafe {
            libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, 4, 0)
        };
        if p == libc::MAP_FAILED {
            return None;
        }
        Some(Buffer { base: p as *mut u8, len })
    }
    fn put(&self, at: usize, bytes: &[u8]) -> bool {
        if at + bytes.len() > self.len {
            return false;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.base.add(at), bytes.len()) };
        true
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base as *mut c_void, self.len) };
    }
}

// ---- the engine ------------------------------------------------------------------

struct Engine {
    out: UnixStream,
    dir: PathBuf,
    sim: TowerSim,
    clock: Clock,
    art: Option<Art>,
    publisher: Publisher,
    // the surface
    buf: Option<Buffer>,
    live_at: usize,
    art_at: usize,
    surface: bool,
    art_put: bool,
    waiting: bool,
    owed: bool,
    put_all: bool,
    dirty: bool,
    last_publish: Instant,
    // the view
    zoom: f64,
    look: Look,
    hold: Option<u16>, // the clock held at an hour (1/64 h) for the pictures, or None
    // the game
    speed: usize,
    tool: usize,
    hover: Option<(i64, i64)>,
    last_cell: Option<(i64, i64)>,
    path: Option<String>,
    title: String,
    modified: bool,
    news: String,
    told_props: String,
    told_inspect: String,
    told_ticks: Vec<u8>,
    selected: Option<(i64, i64)>,
    room: i32,
    centred: bool, // the view taken to the tower at the zoom the shell said
    scene: movers::Scene,
    last_frame: Instant,
    ui: Ui,
}

fn leaf_title(path: &str) -> String {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    let leaf = match leaf.rsplit_once(',') {
        Some((name, t)) if t.len() == 3 && t.chars().all(|c| c.is_ascii_hexdigit()) => name,
        _ => leaf,
    };
    leaf.strip_suffix(".json").unwrap_or(leaf).to_string()
}

fn money(v: i64) -> String {
    let t = v.abs().to_string();
    let mut o = String::new();
    for (i, c) in t.chars().enumerate() {
        if i > 0 && (t.len() - i) % 3 == 0 {
            o.push(',');
        }
        o.push(c);
    }
    if v < 0 {
        format!("-${o}")
    } else {
        format!("${o}")
    }
}

fn count(v: i64) -> String {
    money(v).replacen('$', "", 1)
}

/// REST names NAME: its value after a space, or "" if just the name
fn arg<'a>(rest: &'a str, name: &str) -> Option<&'a str> {
    if rest == name {
        Some("")
    } else {
        rest.strip_prefix(name).and_then(|r| r.strip_prefix(' ')).map(|r| r.trim())
    }
}

/// a file beside the engine, under any of its names (HostFS adds ",xxx")
fn beside(dir: &Path, names: &[&str]) -> Option<Vec<u8>> {
    names.iter().find_map(|n| std::fs::read(dir.join(n)).ok())
}

/// the art: the packed block (zlib) and its manifest, beside the engine
fn load_art(dir: &Path) -> Result<Art, String> {
    let z = beside(dir, &["Art,ffd", "Art"]).ok_or("no Art beside the engine")?;
    let block = miniz_oxide::inflate::decompress_to_vec_zlib(&z).map_err(|e| format!("the Art file: {e:?}"))?;
    let list = beside(dir, &["ArtList,fff", "ArtList"]).ok_or("no ArtList beside the engine")?;
    Art::new(block, &String::from_utf8_lossy(&list))
}

impl Engine {
    fn say(&mut self, s: String) {
        let _ = self.out.write_all(s.as_bytes());
        let _ = self.out.write_all(b"\n");
    }

    /// a line of text for the user, in RISC OS Latin-1
    fn say_text(&mut self, kind: &str, text: &str) {
        let mut b = format!("{kind} ").into_bytes();
        b.extend(text.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }));
        b.push(b'\n');
        let _ = self.out.write_all(&b);
    }

    // ---- documents ----

    fn tell_doc(&mut self) {
        // the whole tower at 100%: 1,500 x 1,920 pixels (a unit a pixel), in
        // 1/100 mm rounded up so that the shell's pixels come out whole
        let mm = |px: f64| (px / PPMM_100 * 100.0).ceil() as i64;
        let (w, h) = (mm(WORLD_W), mm(WORLD_H));
        self.say(format!("doc 1 {w} {h} {w} {h}"));
    }

    fn fresh(&mut self, sim: TowerSim, clock: Clock, title: &str, path: Option<String>) {
        self.sim = sim;
        self.clock = clock;
        self.path = path;
        self.title = title.to_string();
        self.modified = false;
        self.selected = None;
        // a question about the tower before is not this one's
        if let Some(s) = self.ui.held_speed.take() {
            self.speed = s;
        }
        if self.ui.asking.take().is_some() {
            self.say("unquestion".into());
        }
        self.ui.questions.clear();
        self.ui.log.clear();
        self.ui.flags = self.sim.world().event_state.game_state_flags;
        self.publisher.reset();
        self.scene.reset();
        self.say_text("title", title);
        self.say("modified 0".into());
        self.tell_doc();
        self.dirty = true;
        self.go_built();
        self.panes();
    }

    /// the view to the middle of what is built (the lobby's middle on the
    /// ground floor of a tower with nothing)
    fn go_built(&mut self) {
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for p in self.sim.cells_to_array() {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x + 1);
            y1 = y1.max(p.y + 1);
        }
        let (cx, cy) = if x0 > x1 {
            (GRID_W as f64 / 2.0, GROUND as f64 - 4.0)
        } else {
            ((x0 + x1) as f64 / 2.0, ((y0 + y1) as f64 / 2.0).max(GROUND as f64 - 8.0))
        };
        let k = self.zoom;
        self.say(format!("centre {} {}", (cx * UNIT_W as f64 * k) as i64, (cy * UNIT_H as f64 * k) as i64));
    }

    fn new_tower(&mut self) {
        let sim = if self.ui.new_box_rules {
            TowerSim::create_box("box", "New tower", None)
        } else {
            TowerSim::create("box", "New tower", None)
        };
        let clock = Clock::of(&sim);
        self.fresh(sim, clock, "New tower", None);
        self.news = "A new tower: lay a lobby on the ground floor, then floors and rooms above it.".into();
    }

    /// A new tower to try things in: five stars, so that every room and
    /// event is open to it, and $10,000,000 to build with
    fn test_game(&mut self) {
        self.new_tower();
        self.sim.set_star_count(5);
        self.sim.set_cash(10_000_000);
        self.title = "Test game".into();
        self.say_text("title", "Test game");
        self.news = "A test game: five stars and $10,000,000. Lay a lobby on the ground floor to begin.".into();
        self.panes();
    }

    /// A new tower for the player who wants no limits: BOX RULES, five
    /// stars (every room open to it) and $1,000,000,000 to build with (the
    /// funds may grow to BOX RULES' cap, $9,999,999,999)
    fn unlimited_game(&mut self) {
        let sim = TowerSim::create_box("box", "Unlimited tower", None);
        let clock = Clock::of(&sim);
        self.fresh(sim, clock, "Unlimited tower", None);
        self.sim.set_star_count(5);
        self.sim.set_cash(1_000_000_000);
        self.title = "Unlimited tower".into();
        self.say_text("title", "Unlimited tower");
        self.news = "An unlimited tower: BOX RULES, five stars and $1,000,000,000. Lay a lobby on the ground floor to begin.".into();
        self.panes();
    }

    fn open(&mut self, path: &str) {
        self.open_as(path, None);
    }

    fn open_as(&mut self, path: &str, name: Option<&str>) {
        let text = match std::fs::read(path) {
            Ok(t) => t,
            Err(e) => {
                self.say_text("error", &format!("{path} could not be read: {e}"));
                return;
            }
        };
        let snap: SimSnapshot = match serde_json::from_slice(&text) {
            Ok(s) => s,
            Err(e) => {
                self.say_text("error", &format!("{} is not a tower !Tower can read ({e}).", leaf_title(path)));
                return;
            }
        };
        let clock = Clock::new(snap.time.clone());
        let sim = TowerSim::from_snapshot(&snap);
        let title = name.map(str::to_string).unwrap_or_else(|| leaf_title(path));
        self.fresh(sim, clock, &title, Some(path.to_string()));
        self.news = format!("{title} opened.");
    }

    fn fixture(&mut self, i: usize) {
        let Some((_, file)) = FIXTURES.get(i) else { return };
        let dir = self.dir.join("Towers");
        let found = [format!("{file},a9d"), format!("{file}.json"), file.to_string()]
            .iter()
            .map(|n| dir.join(n))
            .find(|p| p.exists());
        match found {
            Some(p) => {
                let name = FIXTURES[i].0;
                self.open_as(&p.to_string_lossy(), Some(name));
                self.path = None; // a fixture is not saved over
                self.news = format!("{name}, a tower from the simulation's tests.");
            }
            None => self.say_text("error", &format!("!Tower has no fixture tower {file} (ports/rosapps/shell/build.sh)")),
        }
    }

    fn save(&mut self, path: &str) {
        let snap = self.sim.save_state();
        let ok = serde_json::to_vec(&snap).ok().map(|b| std::fs::write(path, b).is_ok()).unwrap_or(false);
        if !ok {
            self.say_text("error", &format!("{path} could not be written."));
            return;
        }
        self.path = Some(path.to_string());
        self.title = leaf_title(path); // the shell's title is the file's leaf name
        self.modified = false;
        self.say("modified 0".into());
        self.say_text("saved", path);
        self.panes();
    }

    fn touched(&mut self) {
        self.dirty = true;
        if !self.modified {
            self.modified = true;
            self.say("modified 1".into());
        }
    }

    // ---- the pointer ----

    /// the cell under a point in the picture (pixels at the zoom)
    fn cell_at(&self, x: f64, y: f64) -> (i64, i64) {
        let k = self.zoom.max(0.01);
        (((x / k) / UNIT_W as f64).floor() as i64, ((y / k) / UNIT_H as f64).floor() as i64)
    }

    /// the cells a tool would build on at a cell: left, row, width
    fn footprint(&self, (cx, cy): (i64, i64)) -> Option<(i64, i64, i64)> {
        match self.kind().act {
            Act::Bulldoze | Act::AddCar | Act::RemoveCar => Some((cx, cy, 1)),
            Act::Inspect | Act::Pan => None,
            Act::Place("floor" | "lobby", _) => Some((cx, cy, 1)),
            Act::Place(tile, how) => {
                let w = game::tile_width(tile);
                let mut x = cx - w / 2;
                if how == 2 {
                    // a shaft: onto the one beside it, to extend it
                    let mode = match tile {
                        "elevatorExpress" => 0,
                        "elevatorService" => 2,
                        _ => 1,
                    };
                    let floor = GRID_H as i64 - 1 - cy;
                    if let Some(c) = self.sim.live_carriers().iter().find(|c| {
                        c.carrier_mode == mode
                            && (c.column - x).abs() < w
                            && floor >= c.bottom_served_floor - 1
                            && floor <= c.top_served_floor + 1
                    }) {
                        x = c.column;
                    }
                }
                Some((x, cy, w))
            }
        }
    }

    fn place(&mut self, tile: &str, x: i64, y: i64) -> bool {
        if let Some(why) = self.refusal() {
            self.news = format!("{}: {why}.", self.kind().title);
            return false;
        }
        let r = self.sim.submit_command(&SimCommand::PlaceTile { x, y, tile_type: tile.to_string() });
        if r.accepted {
            self.touched();
            self.news.clear();
        } else if let Some(why) = r.reason {
            self.news = why;
        }
        r.accepted
    }

    fn apply(&mut self, cell: (i64, i64)) {
        let (cx, cy) = cell;
        if cx < 0 || cy < 0 || cx >= GRID_W as i64 || cy >= GRID_H as i64 {
            return;
        }
        match self.kind().act {
            Act::Inspect => {
                self.selected = Some(cell);
                self.ui.page = game::P_ROOM;
                self.dirty = true;
            }
            Act::Pan => {}
            Act::Bulldoze => {
                let r = self.sim.submit_command(&SimCommand::RemoveTile { x: cx, y: cy });
                if r.accepted {
                    self.touched();
                } else if let Some(why) = r.reason {
                    self.news = why;
                }
            }
            Act::AddCar | Act::RemoveCar => {
                // a car added to the shaft clicked, its home the floor
                // clicked; or the shaft's last car taken away (the game asks
                // first if people are riding or waiting)
                let q = self.sim.query_cell(cx, cy);
                if let Some(c) = q.carrier_info {
                    let x = c.column;
                    let cmd = if self.kind().act == Act::AddCar {
                        SimCommand::AddElevatorCar { x, y: cy }
                    } else {
                        SimCommand::RemoveElevatorCar { x, y: cy }
                    };
                    let r = self.sim.submit_command(&cmd);
                    let asked = self.sim.drain_prompts();
                    let asking = !asked.is_empty();
                    self.prompts(asked);
                    if r.accepted {
                        if !asking {
                            self.touched();
                        }
                        self.news.clear();
                    } else if let Some(why) = r.reason {
                        self.news = why;
                    }
                } else {
                    self.news = "Click on an elevator shaft.".into();
                }
                self.selected = Some(cell);
                self.ui.page = game::P_ROOM;
            }
            Act::Place(tile, _) => {
                if let Some((x, y, _)) = self.footprint(cell) {
                    self.place(tile, x, y);
                }
            }
        }
        self.panes();
    }

    fn press(&mut self, x: f64, y: f64) {
        let c = self.cell_at(x, y);
        self.last_cell = Some(c);
        self.apply(c);
    }

    fn drag(&mut self, x: f64, y: f64) {
        let how = self.kind().how();
        if how == 0 {
            return;
        }
        let c = self.cell_at(x, y);
        let Some(from) = self.last_cell else {
            self.last_cell = Some(c);
            self.apply(c);
            return;
        };
        // floors and lobbies along the row they began on; an elevator up and
        // down its column
        let c = if how == 1 { (c.0, from.1) } else { (from.0, c.1) };
        if c == from {
            return;
        }
        let (dx, dy) = (c.0 - from.0, c.1 - from.1);
        let n = dx.abs().max(dy.abs());
        for i in 1..=n {
            self.apply((from.0 + dx * i / n, from.1 + dy * i / n));
        }
        self.last_cell = Some(c);
    }

    fn set_tool(&mut self, name: &str) {
        if let Some(i) = TOOLS.iter().position(|t| t.sprite == name) {
            self.tool = i;
            self.dirty = true;
            self.ui.page = match self.kind().act {
                Act::Inspect | Act::AddCar | Act::RemoveCar => game::P_ROOM,
                Act::Pan => self.ui.page,
                _ => game::P_TOWER,
            };
            self.panes();
        }
    }

    fn menu(&mut self, m: i64, i: i64) {
        match (m, i) {
            (3, 0) => self.new_tower(),
            (3, 1) => self.test_game(),
            (3, 2) => self.unlimited_game(),
            (3, k) if k >= 3 => self.fixture(k as usize - 3),
            // the View menu's own: the clock running, or held at noon or midnight
            (2, 0) => self.hold = None,
            (2, 1) => self.hold = Some(12 * 64),
            (2, 2) => self.hold = Some(0),
            _ => {}
        }
        self.dirty = true;
        self.panes();
    }

    fn key(&mut self, code: u32) {
        if code == 32 && self.ui.held_speed.is_none() {
            self.speed = if self.speed == 0 { 2 } else { 0 };
            self.panes();
        }
    }

    /// a tick of the simulation, and what it says
    fn step(&mut self) {
        let r = self.sim.step();
        for n in r.notifications {
            self.notified(n.kind, &n.message);
        }
        if !r.prompts.is_empty() {
            self.prompts(r.prompts);
        }
        self.clock.sync(&self.sim);
        let day = self.sim.time().day_counter;
        if day != self.ui.failures_day {
            self.ui.failures_day = day;
            self.ui.failures = 0;
        }
        self.watch_events();
    }

    // ---- the panes ----

    fn clock_text(&self) -> String {
        let h = match self.hold {
            Some(h) => h as f64 / 64.0,
            None => self.clock.hour(),
        };
        let (hh, mm) = (h.floor() as i64, ((h - h.floor()) * 60.0).floor() as i64);
        let day = if self.clock.weekend() { "weekend" } else { "weekday" };
        format!("{hh:02}:{mm:02} {day}")
    }

    fn date_text(&self) -> String {
        let quarter = ["1st", "2nd", "3rd", "4th"][((self.clock.t.day_counter / 3) % 4) as usize];
        format!("{quarter} quarter, year {}", self.clock.t.day_counter / 12 + 1)
    }

    // ---- the surface ----

    /// "tower LIVE ART": the presenter draws the tower from the blocks at
    /// those bytes of the buffer
    fn surface_start(&mut self, live: usize, art: usize) {
        if self.buf.is_none() {
            self.buf = Buffer::map();
        }
        let need_art = self.art.as_ref().map(|a| a.block.len()).unwrap_or(0);
        match self.buf.as_ref() {
            Some(b) if live + std::mem::size_of::<Tower>() <= b.len && art + need_art <= b.len => {}
            _ => {
                self.say_text("error", "The engine's buffer is too small for the tower.");
                return;
            }
        }
        if self.art.is_none() {
            self.say_text("error", "!Tower's art is missing: the tower cannot be drawn.");
            return;
        }
        self.live_at = live;
        self.art_at = art;
        self.surface = true;
        self.art_put = false;
        self.put_all = true;
        self.waiting = false;
        self.dirty = true;
    }

    /// "scale PPMM100": the zoom, for where the pointer is; at a new zoom
    /// the view goes to the tower again (the shell keeps its top left)
    fn set_scale(&mut self, ppmm100: f64) {
        if ppmm100 > 0.0 {
            let z = ppmm100 / 100.0 / PPMM_100;
            let moved = (z - self.zoom).abs() > 1e-6;
            self.zoom = z;
            if moved || !self.centred {
                self.centred = true;
                self.go_built();
            }
        }
    }

    /// what changed put in the block, and the broker told
    fn render(&mut self) {
        if !self.surface || self.waiting {
            return;
        }
        let (Some(art), Some(b)) = (self.art.as_ref(), self.buf.as_ref()) else { return };
        if !self.art_put {
            if !b.put(self.art_at, &art.block) {
                return;
            }
            self.art_put = true;
            self.owed = true;
        }
        // the tool's outline under the pointer
        let mut look = self.look;
        look.outline = [0; 4];
        look.outline_rgba = OUTLINE;
        if let Some((x, y, w)) = self.hover.and_then(|c| self.footprint(c)) {
            look.outline = [x as i16, y as i16, (x + w) as i16, (y + 1) as i16];
            if self.refusal().is_some() {
                look.outline_rgba = OUTLINE_BAD;
            }
        } else if let (T_INSPECT, Some((x, y))) = (self.tool, self.selected) {
            look.outline = [x as i16, y as i16, x as i16 + 1, y as i16 + 1];
        }
        let due = self.dirty || look != self.look_published() || self.last_publish.elapsed() > Duration::from_millis(250);
        let at = self.live_at;
        if due {
            self.clock.sync(&self.sim);
            let mut clock = self.clock.clone();
            if let Some(h) = self.hold {
                // the pictures' clock: held at an hour (the lamps' day as it is)
                clock.t.day_tick = lighting::day_tick_at(h);
            }
            let ch = self.publisher.publish(&self.sim, &clock, art, &look);
            lighting::held_hour(&mut self.publisher.block, self.hold);
            self.last_publish = Instant::now();
            self.dirty = false;
            let blk = self.publisher.block.as_bytes();
            if self.put_all {
                b.put(at, blk);
                self.put_all = false;
            } else {
                for s in 0..TW_NSECT {
                    if s != TW_ROOMS && ch.sections & (1 << s) != 0 {
                        let r = section_range(s);
                        b.put(at + r.start, &blk[r]);
                    }
                }
                for k in 0..16 {
                    if ch.chunks & (1 << k) != 0 {
                        let r = chunk_range(k);
                        b.put(at + r.start, &blk[r]);
                    }
                }
                b.put(at, &blk[..192]); // the header, every time
            }
            self.owed = true;
        }
        // what moves, every frame
        let dt = self.last_frame.elapsed().as_secs_f64().min(0.5);
        self.last_frame = Instant::now();
        self.clock.sync(&self.sim);
        let ticks = TICKS_PER_SECOND[self.speed] as f64;
        let (stops, movers) = self.scene.frame(&self.sim, art, &self.clock, dt, ticks, &self.publisher.site);
        let ch = self.publisher.publish_moving(&stops, &movers);
        if ch.any() {
            let blk = self.publisher.block.as_bytes();
            for s in [TW_STOPS, TW_MOVERS] {
                if ch.sections & (1 << s) != 0 {
                    let r = section_range(s);
                    b.put(at + r.start, &blk[r]);
                }
            }
            b.put(at, &blk[..192]);
            self.owed = true;
        }
        if self.owed {
            self.owed = false;
            self.waiting = true;
            self.say("tower".into());
        }
    }

    /// For the parity test (graphics design 8.5): when a file dump.request
    /// is beside the engine, the live block as last published is written to
    /// dump-live.bin there, and the request removed
    fn dump_if_asked(&mut self) {
        let req = self.dir.join("dump.request");
        if !req.exists() {
            return;
        }
        let _ = std::fs::remove_file(&req);
        let _ = std::fs::write(self.dir.join("dump-live.bin"), self.publisher.block.as_bytes());
    }

    fn look_published(&self) -> Look {
        let t = &self.publisher.block;
        Look { quality: t.quality, view: t.view, outline: t.outline, outline_rgba: t.outline_rgba }
    }
}

/// The application's directory: the engine's own, or the one above it when
/// the engine is in the application's hidden .engine directory (install.sh)
fn app_dir() -> Option<std::path::PathBuf> {
    let d = std::env::current_exe().ok()?.parent()?.to_path_buf();
    if d.file_name().is_some_and(|n| n == ".engine") {
        return d.parent().map(|p| p.to_path_buf());
    }
    Some(d)
}

fn main() {
    let sock = unsafe { UnixStream::from_raw_fd(3) };
    let out = sock.try_clone().expect("the socket");
    let dir = app_dir().unwrap_or_default();
    let art = load_art(&dir);
    let sim = TowerSim::create_box("box", "New tower", None);
    let clock = Clock::of(&sim);
    let mut e = Engine {
        out,
        dir,
        sim,
        clock,
        art: None,
        publisher: Publisher::new(4),
        buf: None,
        live_at: 0,
        art_at: 0,
        surface: false,
        art_put: false,
        waiting: false,
        owed: false,
        put_all: true,
        dirty: true,
        last_publish: Instant::now(),
        zoom: 1.0,
        look: Look::default(),
        hold: None,
        speed: 2,
        tool: 0,
        hover: None,
        last_cell: None,
        path: None,
        title: "New tower".into(),
        modified: false,
        news: String::new(),
        told_props: String::new(),
        told_inspect: String::new(),
        told_ticks: Vec::new(),
        selected: None,
        room: 0,
        centred: false,
        scene: movers::Scene::new(),
        last_frame: Instant::now(),
        ui: Ui::new(),
    };
    match art {
        Ok(a) => e.art = Some(a),
        Err(why) => e.news = format!("The art could not be read: {why}"),
    }
    e.say("hello tower 0.1 tower-engine".into());

    let fd = 3;
    let mut input: Vec<u8> = Vec::new();
    let mut next_tick = Instant::now();
    let mut next_panes = Instant::now();
    'run: loop {
        let now = Instant::now();
        let mut due = next_panes;
        if e.speed > 0 {
            due = due.min(next_tick);
        }
        let mut wait = due.saturating_duration_since(now).as_millis().min(250) as c_int;
        if e.surface && !e.waiting {
            wait = wait.min(33);
        }
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ready = unsafe { libc::poll(&mut pfd, 1, wait) };
        if ready > 0 {
            let mut chunk = [0u8; 4096];
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut c_void, chunk.len()) };
            if n <= 0 {
                break;
            }
            input.extend_from_slice(&chunk[..n as usize]);
            while let Some(nl) = input.iter().position(|&c| c == b'\n') {
                let raw: Vec<u8> = input.drain(..=nl).collect();
                let line: String = raw[..raw.len() - 1].iter().map(|&c| c as char).collect();
                let line = line.trim_end_matches('\r');
                let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
                let nums: Vec<f64> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                let at = |i: usize| nums.get(i).copied().unwrap_or(0.0);
                match cmd {
                    "new" => e.new_tower(),
                    "testgame" => e.test_game(),
                    "unlimited" => e.unlimited_game(),
                    "open" => e.open(rest.trim()),
                    "save" => e.save(rest.trim()),
                    "tower" if nums.len() >= 2 => e.surface_start(at(0) as usize, at(1) as usize),
                    "scale" if !nums.is_empty() => e.set_scale(at(0)),
                    "page" => e.say_text("error", "!Tower is drawn by the GPU presenter, and this box has none."),
                    "press" => e.press(at(0), at(1)),
                    "drag" => e.drag(at(0), at(1)),
                    "release" => {
                        // the stroke to where the button went up, then ended
                        if nums.len() >= 2 && e.last_cell.is_some() {
                            e.drag(at(0), at(1));
                        }
                        e.last_cell = None;
                    }
                    "pointer" => {
                        e.hover = if at(0) < 0.0 { None } else { Some(e.cell_at(at(0), at(1))) };
                    }
                    "tool" => e.set_tool(rest.trim()),
                    "menu" if nums.len() == 2 => e.menu(nums[0] as i64, nums[1] as i64),
                    "prop" => e.prop(rest),
                    "answer" => e.answer(rest.trim()),
                    "hover" => {
                        let t = rest.trim();
                        e.ui.hover_tool = TOOLS.iter().position(|x| x.sprite == t);
                        e.panes();
                    }
                    "room" if !nums.is_empty() => {
                        e.room = at(0) as i32;
                        e.ui.room = e.room;
                        e.told_props.clear();
                        e.panes();
                    }
                    "key" if !nums.is_empty() => e.key(at(0) as u32),
                    "quality" if !nums.is_empty() => {
                        e.look.quality = lighting::quality(at(0));
                        e.dirty = true;
                    }
                    "hour" if !nums.is_empty() => {
                        e.hold = lighting::hold(at(0));
                        e.dirty = true;
                        e.panes();
                    }
                    "shown" => e.waiting = false,
                    "quit" => break 'run,
                    _ => {}
                }
            }
        }
        let now = Instant::now();
        if e.speed > 0 && now >= next_tick {
            let every = Duration::from_millis(1000 / TICKS_PER_SECOND[e.speed] as u64);
            // a few ticks at once when the clock is fast or the engine late
            let mut n = 0;
            while n < 8 && next_tick <= now {
                e.step();
                n += 1;
                next_tick += every;
            }
            if now.duration_since(next_tick) > every * 8 {
                next_tick = now + every;
            }
        }
        if now >= next_panes {
            e.panes();
            e.dump_if_asked();
            for (cmd, v) in lighting::asked(&e.dir) {
                match cmd.as_str() {
                    "quality" => e.look.quality = lighting::quality(v),
                    "speed" => e.speed = (v.max(0.0) as usize).min(SPEEDS.len() - 1),
                    "fixture" => e.fixture(v.max(0.0) as usize),
                    _ => e.hold = lighting::hold(v),
                }
                e.dirty = true;
            }
            e.event_if_asked();
            next_panes = now + Duration::from_millis(500);
        }
        e.render();
    }
}

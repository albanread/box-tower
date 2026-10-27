// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Alban Read
//
// game -- what the player sees of the tower besides the picture: the
// palette's tools and what each one costs, needs and gives; the properties
// pane's pages (Tower, Room, Finances, Evaluation, Options), cut to the
// pane's height; the news, each item a button that goes to where it
// happened; and the game's questions (a fire, a bomb threat, a car taken
// from a busy shaft), asked in a dialogue box and answered back to the
// simulation.  Part of !Tower's engine (main.rs).

use super::*;
use std::collections::VecDeque;
use tower_sim::index::QueryCellResult;
use tower_sim::box_ext::amenities;
use tower_sim::resources::{CARRIER_CAR_CONSTRUCTION_COST, CARRIER_EXTEND_FLOOR_COST, STAR_THRESHOLDS, TILE_COSTS};
use tower_sim::world::{SimNotificationKind, SimPrompt, SimPromptKind};

// ---- the palette -------------------------------------------------------------------

/// What a palette tool does
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Inspect,
    Pan,
    Bulldoze,
    /// a tile laid by a click (0), along a row (1) or up and down a column (2)
    Place(&'static str, u8),
    AddCar,
    RemoveCar,
}

/// One of a tool's kinds (a hotel tool's single room, twin room, suite):
/// its button on the pane, its title, what it does, and the pane's rows
/// about it (what it needs, gives and costs to run), each short enough for
/// the pane's values
pub struct Kind {
    pub label: &'static str,
    pub title: &'static str,
    pub act: Act,
    pub size: &'static str,
    pub needs: &'static str,
    pub gives: &'static str,
    pub income: &'static str,
    pub upkeep: &'static str,
    pub hint: &'static str,
}

const fn k(
    label: &'static str, title: &'static str, act: Act, size: &'static str, needs: &'static str, gives: &'static str,
    income: &'static str, upkeep: &'static str, hint: &'static str,
) -> Kind {
    Kind { label, title, act, size, needs, gives, income, upkeep, hint }
}

/// A palette tool: its sprite (the shell's tool list, in the same order)
/// and its kinds
pub struct Tool {
    pub sprite: &'static str,
    pub kinds: &'static [Kind],
}

use Act::*;

pub const TOOLS: &[Tool] = &[
    Tool { sprite: "tw_inspect", kinds: &[k("", "Inspect", Inspect, "", "", "What a room is", "", "", "Click a room")] },
    Tool { sprite: "rs_pan", kinds: &[k("", "Move the view", Pan, "", "", "", "", "", "Drag the tower")] },
    Tool { sprite: "tw_bulldoze", kinds: &[k("", "Bulldoze", Bulldoze, "", "", "Clears what is built", "No refund", "", "Click to remove")] },
    Tool { sprite: "tw_floor", kinds: &[k("", "Floor", Place("floor", 1), "1 cell", "Floor below it", "Room to build", "", "", "Drag along a row")] },
    Tool {
        sprite: "tw_lobby",
        kinds: &[k("", "Lobby", Place("lobby", 1), "1 cell", "Ground floor", "The way in", "", "", "Sky lobby: 15 up")],
    },
    Tool {
        sprite: "tw_office",
        kinds: &[k("", "Office", Place("office", 0), "9 cells", "Lift or stairs", "6 workers", "$10,000/3 days", "", "Click on a floor")],
    },
    Tool {
        sprite: "tw_condo",
        kinds: &[k("", "Condominium", Place("condo", 0), "16 cells", "Lift or stairs", "3 residents", "Sold: $150,000", "", "Click on a floor")],
    },
    Tool {
        sprite: "tw_hotel",
        kinds: &[
            k("Single", "Single room", Place("hotelSingle", 0), "4 cells", "Housekeeping", "1 guest a night", "$2,000 a stay", "", "Click on a floor"),
            k("Twin", "Twin room", Place("hotelTwin", 0), "6 cells", "Housekeeping", "2 guests a night", "$3,000 a stay", "", "Click on a floor"),
            k("Suite", "Suite", Place("hotelSuite", 0), "10 cells", "Housekeeping", "2 guests a night", "$6,000 a stay", "", "Click on a floor"),
        ],
    },
    Tool {
        sprite: "tw_food",
        kinds: &[
            k("Fast food", "Fast food", Place("fastFood", 0), "16 cells", "Customers", "Lunches, visitors", "Up to $5,000/day", "", "Loses if quiet"),
            k("Restaurant", "Restaurant", Place("restaurant", 0), "24 cells", "Customers", "Evening diners", "Up to $10,000/day", "", "Loses if quiet"),
        ],
    },
    Tool {
        sprite: "tw_shop",
        kinds: &[k("", "Shop", Place("retail", 0), "12 cells", "Shoppers", "Shoppers, visitors", "$15,000/3 days", "", "Click on a floor")],
    },
    Tool {
        sprite: "tw_leisure",
        kinds: &[
            k("Party hall", "Party hall", Place("partyHall", 0), "27 cells, 2 high", "Two clear floors", "Parties, visitors", "From its guests", "", "Click lower floor"),
            k("Cinema", "Cinema", Place("cinema", 0), "31 cells, 2 high", "Two clear floors", "Films, visitors", "From its audience", "", "Click lower floor"),
            k("Cathedral", "Cathedral", Place("cathedral", 0), "28 cells, 5 high", "Floor 103", "The Tower rating", "", "", "Floor 103 only"),
        ],
    },
    Tool {
        sprite: "tw_services",
        kinds: &[
            k("Security", "Security office", Place("security", 0), "16 cells", "", "Fire, bomb cover", "", "$20,000/3 days", "For the 3rd star"),
            k("Housekeeping", "Housekeeping", Place("housekeeping", 0), "15 cells", "A service lift", "Clean hotel rooms", "", "$10,000/3 days", "Hotels need it"),
            k("Medical", "Medical centre", Place("medical", 0), "26 cells", "", "Care, ambulances", "", "", "For the 4th star"),
            k("Recycling", "Recycling centre", Place("recyclingCenter", 0), "25 cells, 2 high", "A service lift", "Takes the rubbish", "", "$50,000/3 days", "For the 4th star"),
        ],
    },
    Tool {
        sprite: "tw_amenity",
        kinds: &[
            k("Laundry", "Laundry", Place("laundry", 0), "12 cells", "BOX rules", "Hotel rooms rate better", "", "$10,000/3 days", "Above the lobby"),
            k("Guest services", "Guest services", Place("guestServices", 0), "10 cells", "BOX rules", "Hotels, condos rate better", "", "$15,000/3 days", "Beside a lobby"),
            k("Lounge", "Premier lounge", Place("premierLounge", 0), "20 cells", "BOX rules", "Offices, suites rate better", "$60 a visit", "$30,000/3 days", "Above the lobby"),
            k("Pool", "Swimming pool", Place("pool", 0), "30 cells", "BOX rules", "Condos, hotels rate better", "$40 a visit", "$20,000/3 days", "Basement floors"),
            k("Gym", "Gym", Place("gym", 0), "16 cells", "BOX rules", "Offices, condos rate better", "$25 a visit", "$10,000/3 days", "Basement floors"),
            k("Convention", "Convention centre", Place("convention", 0), "30 cells, 2 high", "BOX rules", "Hotel rooms rate better", "$20,000 a weekday", "$50,000/3 days", "Click lower floor"),
            k("Kindergarten", "Kindergarten", Place("kindergarten", 0), "12 cells", "BOX rules", "30 places; homes rate better", "$25 a child a day", "$12,000/3 days", "Above the lobby"),
            k("School", "Primary school", Place("school", 0), "30 cells, 2 high", "BOX rules", "150 places; homes rate better", "", "$40,000/3 days", "Click lower floor"),
            k("Sprinklers", "Sprinkler room", Place("sprinklers", 0), "8 cells", "BOX rules", "Fires 5 floors each way slow", "", "$10,000/3 days", "Above the lobby"),
        ],
    },
    Tool {
        sprite: "tw_parking",
        kinds: &[
            k("Space", "Parking space", Place("parking", 0), "4 cells", "Ramp or car lift", "A car's space", "", "From 3 stars", "Basement floors"),
            k("Ramp", "Parking ramp", Place("parkingRamp", 0), "15 cells", "Down from B1", "Cars to spaces", "", "", "Basement floors"),
            k("Car lift", "Car elevator", Place("carElevator", 0), "8 cells", "BOX rules", "Cars to basements", "", "", "Click lowest floor"),
        ],
    },
    Tool {
        sprite: "tw_metro",
        kinds: &[k("", "Metro station", Place("metro", 0), "30 cells, 3 high", "Basement floors", "Visitors by rail", "", "", "For the 5th star")],
    },
    Tool {
        sprite: "tw_stairs",
        kinds: &[
            k("Stairs", "Stairs", Place("stairs", 0), "8 cells", "Floors to join", "Walk up a floor", "", "", "Click lower floor"),
            k("Escalator", "Escalator", Place("escalator", 0), "8 cells", "Floors to join", "Up a floor", "", "$5,000/3 days", "Click lower floor"),
        ],
    },
    Tool {
        sprite: "tw_elevator",
        kinds: &[
            k("Standard", "Elevator", Place("elevator", 2), "4 cells", "Floor to stand on", "Lifts everyone", "", "$10,000/car/3 d", "Drag up or down"),
            k("Express", "Express elevator", Place("elevatorExpress", 2), "6 cells", "Floor to stand on", "Lobby to lobby", "", "$20,000/car/3 d", "Drag up or down"),
            k("Service", "Service elevator", Place("elevatorService", 2), "4 cells", "Floor to stand on", "Staff only", "", "$10,000/car/3 d", "Drag up or down"),
        ],
    },
    Tool {
        sprite: "tw_cars",
        kinds: &[
            k("Add a car", "Add a car", AddCar, "", "A shaft", "Shorter waits", "", "", "Click a shaft"),
            k("Remove a car", "Remove a car", RemoveCar, "", "A shaft", "Lower upkeep", "", "", "Click a shaft"),
        ],
    },
];

pub const T_INSPECT: usize = 0;

/// a tile's width in cells (the car elevator and the amenities are BOX
/// RULES', not upstream's)
pub fn tile_width(tile: &str) -> i64 {
    if tile == "carElevator" {
        8
    } else if let Some(k) = amenities::kind(tile) {
        k.width
    } else {
        publish::width_of(tile) as i64
    }
}

/// a placed tile's name for the player
pub fn tile_title(tile: &str) -> String {
    for t in TOOLS {
        for kd in t.kinds {
            if !kd.tile().is_empty() && kd.tile() == tile {
                return kd.title.to_string();
            }
        }
    }
    match tile {
        "recyclingCenterUpper" | "recyclingCenterLower" => "Recycling centre".into(),
        "empty" | "" => "Nothing built".into(),
        t => t.to_string(),
    }
}

impl Kind {
    pub fn how(&self) -> u8 {
        match self.act {
            Place(_, h) => h,
            _ => 0,
        }
    }
    pub fn tile(&self) -> &'static str {
        match self.act {
            Place(t, _) => t,
            _ => "",
        }
    }
}

// ---- the pages -----------------------------------------------------------------------

pub const PAGES: [&str; 5] = ["Tower", "Room", "Finances", "Evaluation", "Options"];
pub const P_TOWER: usize = 0;
pub const P_ROOM: usize = 1;
const P_MONEY: usize = 2;
const P_EVAL: usize = 3;
const P_OPTIONS: usize = 4;

const RENTS: [&str; 4] = ["High", "Average", "Low", "Very low"];
const CLOCKS: [&str; 3] = ["Runs", "Noon", "Midnight"];
/// the rooms the game rates (an evaluation of 0 to 2)
const RATED: [&str; 9] = ["office", "condo", "hotelSingle", "hotelTwin", "hotelSuite", "retail", "fastFood", "restaurant", "cinema"];
const QUALITIES: [&str; 3] = ["Classic", "Smooth", "Rich"];

/// a question the game asks: shown one at a time in the shell's question
/// box ("question ID|TITLE|TEXT|ACTIONS"); the first action is yes
pub struct Question {
    pub id: String,
    pub title: String,
    pub text: String,
    pub actions: String,
    /// the simulation's prompt (answered with prompt_response), or ours
    pub sim: bool,
    /// the game held while it is asked
    pub holds: bool,
}

/// the front end's state of the game
pub struct Ui {
    pub page: usize,
    pub kind: Vec<usize>, // each tool's kind chosen
    pub hover_tool: Option<usize>,
    pub room: i32,
    pub log: Vec<(String, Option<(i64, i64)>)>, // the news, latest first, and where
    pub questions: VecDeque<Question>,
    pub asking: Option<String>,
    pub held_speed: Option<usize>,
    pub new_box_rules: bool,
    pub flags: i64,         // the event flags last seen (fire 8, bomb 1, found 0x20, blown 0x40)
    pub failures: u32,      // trips that failed since the day began
    pub failures_day: i64,
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            page: P_TOWER,
            kind: vec![0; TOOLS.len()],
            hover_tool: None,
            room: 0,
            log: Vec::new(),
            questions: VecDeque::new(),
            asking: None,
            held_speed: None,
            new_box_rules: true,
            flags: 0,
            failures: 0,
            failures_day: -1,
        }
    }
}

/// a row the pane may leave out when it is short of room: priority 1..9,
/// the higher going first (fit)
fn opt(p: u8, row: String) -> String {
    format!("\u{1}{}{row}", (b'0' + p) as char)
}

/// The pane's rows cut to its height: the properties pane has no scroll
/// bar.  The rows marked by opt() go, the highest priority first and the
/// last of a priority first (the oldest news), until the rest fit.
fn fit(rows: &mut Vec<String>, room: i32) {
    let prio = |r: &str| if r.as_bytes().first() == Some(&1) { r.as_bytes()[1] - b'0' } else { 0 };
    let plain = |r: &String| if prio(r) > 0 { r[2..].to_string() } else { r.clone() };
    while room > 0 && pane_height(&rows.iter().map(plain).collect::<Vec<_>>()) > room {
        let Some(top) = rows.iter().map(|r| prio(r)).max().filter(|&p| p > 0) else { break };
        let i = rows.iter().rposition(|r| prio(r) == top).unwrap();
        rows.remove(i);
    }
    for r in rows.iter_mut() {
        if prio(r) > 0 {
            *r = r[2..].to_string();
        }
    }
}

/// How tall the shell makes the pane for these rows, in OS units: its own
/// layout (rosapp.mojo show_props) with buttons' widths estimated a little
/// wide
fn pane_height(rows: &[String]) -> i32 {
    const RIGHT: i32 = 500 - 12;
    let (mut y, mut bx) = (12, 12);
    for f in rows {
        if f.is_empty() {
            continue;
        }
        if let Some(name) = f.strip_prefix('*') {
            let wide = 40 + 16 * name.trim_start_matches('=').chars().count() as i32;
            if bx > 12 && bx + wide > RIGHT {
                bx = 12;
                y += 56;
            }
            bx += wide + 8;
            continue;
        }
        if bx > 12 {
            bx = 12;
            y += 56;
        }
        y += if f.starts_with('#') { 60 } else { 52 };
    }
    if bx > 12 {
        y += 56;
    }
    y
}

fn short(t: &str, n: usize) -> String {
    let t = t.replace('|', " ");
    if t.chars().count() <= n {
        t
    } else {
        format!("{}...", t.chars().take(n - 3).collect::<String>().trim_end())
    }
}

fn on(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

/// a floor's name, as the game's: G the ground floor, B1 .. under it
pub fn floor_name(floor: i64) -> String {
    match floor - 10 {
        0 => "G".into(),
        n if n < 0 => format!("B{}", -n),
        n => n.to_string(),
    }
}

fn floor_of_row(y: i64) -> i64 {
    GRID_H as i64 - 1 - y
}

/// a room's state in words, from its record (as the publisher reads it)
fn room_state_text(tile: &str, unit_status: i64, occupied: i64) -> &'static str {
    let hotel = tile.starts_with("hotel");
    match tile {
        "office" if unit_status > 0x0f => "For rent",
        "office" => "Let",
        "condo" if unit_status > 0x17 => "For sale",
        "condo" => "Sold",
        _ if hotel && unit_status >= 0x38 => "Needs cleaning",
        _ if hotel && unit_status == 0 => "Vacant",
        _ if hotel => "Guests in",
        "fastFood" | "restaurant" | "retail" | "cinema" | "partyHall" => {
            if occupied != 0 {
                "Open"
            } else {
                "Closed"
            }
        }
        _ => "In use",
    }
}

fn stress_words(score: i64) -> String {
    match score {
        s if s < 0 => "None yet".into(),
        s if s < 80 => format!("Low ({s})"),
        s if s < 150 => format!("Some ({s})"),
        s => format!("High ({s})"),
    }
}

impl Engine {
    pub fn kind(&self) -> &'static Kind {
        &TOOLS[self.tool].kinds[self.ui.kind[self.tool]]
    }

    /// why the chosen tool cannot build here now, or None
    pub fn refusal(&self) -> Option<String> {
        let tile = self.kind().tile();
        if tile.is_empty() || self.sim.free_build {
            return None;
        }
        if (tile == "carElevator" || amenities::kind(tile).is_some()) && !self.sim.box_rules() {
            return Some("BOX rules only".into());
        }
        let need = get_tile_star_requirement(tile);
        if need > self.sim.star_count() {
            return Some(format!("needs {need} stars"));
        }
        None
    }

    // ---- the news ----

    pub fn tell(&mut self, text: String, place: Option<(i64, i64)>) {
        if self.ui.log.first().is_some_and(|(t, _)| *t == text) {
            return;
        }
        self.ui.log.insert(0, (text.clone(), place));
        self.ui.log.truncate(8);
        self.news = text;
    }

    /// the simulation's notifications, in our words, as news
    pub fn notified(&mut self, kind: SimNotificationKind, message: &str) {
        let (text, place) = match kind {
            SimNotificationKind::StarAdvanced => {
                self.scene.celebrate();
                let n: i64 = message.parse().unwrap_or(0);
                let text = format!("The tower has {} stars.", n);
                self.ask(Question {
                    id: "star".into(),
                    title: "A new star".into(),
                    text: format!("{text} {}", self.next_star_text()),
                    actions: "OK".into(),
                    sim: false,
                    holds: false,
                });
                (text, None)
            }
            SimNotificationKind::RouteFailure => {
                self.ui.failures += 1;
                let text = match message {
                    "Office workers demand Parking" => "Office workers want parking.".to_string(),
                    "Medical Center demanded near Lobby" => "Office workers want a medical centre near the lobby.".to_string(),
                    m => m.to_string(),
                };
                (text, None)
            }
            SimNotificationKind::Event => {
                if message == "metro_train_arrival" {
                    return;
                }
                let es = &self.sim.world().event_state;
                let place = if message.contains("fire") {
                    Some((es.fire_tile, GRID_H as i64 - 1 - es.fire_floor))
                } else {
                    None
                };
                (message.to_string(), place)
            }
        };
        self.tell(text, place);
    }

    /// what the event flags say that the notifications do not: a fire has
    /// started or is out, a bomb was found or went off
    pub fn watch_events(&mut self) {
        let f = self.sim.world().event_state.game_state_flags;
        let was = self.ui.flags;
        if f == was {
            return;
        }
        self.ui.flags = f;
        let es = &self.sim.world().event_state;
        let fire = (es.fire_tile, GRID_H as i64 - 1 - es.fire_floor);
        let bomb = (es.bomb_tile, GRID_H as i64 - 1 - es.bomb_floor);
        let (fire_floor, bomb_floor) = (es.fire_floor, es.bomb_floor);
        if f & 8 != 0 && was & 8 == 0 {
            self.tell(format!("Fire on floor {}!", floor_name(fire_floor)), Some(fire));
        } else if f & 8 == 0 && was & 8 != 0 {
            self.tell("The fire is out.".into(), Some(fire));
            self.touched();
        }
        if f & 1 != 0 && was & 1 == 0 {
            // where it is is for security to find, not the player
            self.tell("A bomb threat has been made.".into(), None);
        }
        if f & 0x20 != 0 && was & 0x20 == 0 {
            self.tell("The bomb was found and made safe.".into(), Some(bomb));
        }
        if f & 0x40 != 0 && was & 0x40 == 0 {
            self.tell(format!("The bomb went off on floor {}.", floor_name(bomb_floor)), Some(bomb));
            self.touched();
        }
    }

    // ---- the questions ----

    pub fn prompts(&mut self, prompts: Vec<SimPrompt>) {
        for p in prompts {
            let (title, actions) = match p.prompt_kind {
                SimPromptKind::FireRescue => ("Fire", "Call it,Leave it"),
                SimPromptKind::BombRansom => ("Bomb threat", "Pay,Refuse"),
                SimPromptKind::CarrierEditConfirmation => ("Remove a car", "Remove,Keep"),
            };
            let text = match p.prompt_kind {
                SimPromptKind::FireRescue => format!(
                    "Fire is spreading on floor {}. Call the rescue helicopter for {}? {}",
                    floor_name(self.sim.world().event_state.fire_floor),
                    money(p.cost.unwrap_or(0)),
                    if self.has_security() {
                        "Otherwise the security guards fight it."
                    } else {
                        "The tower has no security office to fight it."
                    }
                ),
                SimPromptKind::BombRansom => format!(
                    "A bomb has been hidden in the tower. Pay the ransom of {} to have it made safe? {}",
                    money(p.cost.unwrap_or(0)),
                    if self.has_security() {
                        "Otherwise security must find it in time."
                    } else {
                        "The tower has no security office to search for it."
                    }
                ),
                SimPromptKind::CarrierEditConfirmation => {
                    "People are riding or waiting for this elevator's cars. Take a car away all the same?".to_string()
                }
            };
            self.ask(Question { id: p.prompt_id, title: title.into(), text, actions: actions.into(), sim: true, holds: true });
        }
    }

    fn has_security(&self) -> bool {
        self.sim.world().placed_objects.values().any(|r| r.object_type_code == 0x0e)
    }

    pub fn ask(&mut self, q: Question) {
        self.ui.questions.push_back(q);
        self.next_question();
    }

    fn next_question(&mut self) {
        if self.ui.asking.is_some() {
            return;
        }
        let Some(q) = self.ui.questions.front() else { return };
        let line = format!(
            "question {}|{}|{}|{}",
            q.id,
            q.title.replace('|', " "),
            q.text.replace('|', " "),
            q.actions
        );
        let (id, holds) = (q.id.clone(), q.holds);
        self.ui.asking = Some(id);
        if holds && self.ui.held_speed.is_none() {
            self.ui.held_speed = Some(self.speed);
            self.speed = 0;
        }
        self.say_text("question", &line[9..]);
        self.panes();
    }

    /// "answer ID ACTION": the first of a question's actions is yes
    pub fn answer(&mut self, rest: &str) {
        let (id, action) = rest.split_once(' ').unwrap_or((rest, ""));
        let action = action.split('|').next().unwrap_or("").trim();
        let Some(pos) = self.ui.questions.iter().position(|q| q.id == id) else { return };
        let q = self.ui.questions.remove(pos).unwrap();
        if self.ui.asking.as_deref() == Some(id) {
            self.ui.asking = None;
        }
        let yes = q.actions.split(',').next() == Some(action);
        if q.sim {
            self.sim.submit_command(&SimCommand::PromptResponse { prompt_id: q.id.clone(), accepted: yes });
            for n in self.sim.drain_notifications() {
                self.notified(n.kind, &n.message);
            }
            // what the answer did to the event is told by the simulation's
            // own notification (the ransom paid, the helicopter sent)
            self.ui.flags = self.sim.world().event_state.game_state_flags;
            if q.id.starts_with("carrier_remove_") {
                self.tell(if yes { "A car was taken away.".into() } else { "The car was kept.".into() }, None);
            }
            if q.id.starts_with("bomb_") && !yes {
                self.tell("The ransom was refused: security is searching.".into(), None);
            }
            if q.id.starts_with("fire_") && !yes {
                self.tell("The security guards are fighting the fire.".into(), None);
            }
            self.touched();
        }
        if !self.ui.questions.iter().any(|q| q.holds) {
            if let Some(s) = self.ui.held_speed.take() {
                self.speed = s;
            }
        }
        self.next_question();
        self.panes();
    }

    // ---- the pane ----

    pub fn panes(&mut self) {
        let mut rows: Vec<String> = PAGES
            .iter()
            .enumerate()
            .map(|(i, p)| format!("*{}{p}", if i == self.ui.page { "=" } else { "" }))
            .collect();
        match self.ui.page {
            P_ROOM => self.room_rows(&mut rows),
            P_MONEY => self.money_rows(&mut rows),
            P_EVAL => self.eval_rows(&mut rows),
            P_OPTIONS => self.option_rows(&mut rows),
            _ => self.tower_rows(&mut rows),
        }
        fit(&mut rows, self.ui.room);
        let line = rows.join("|");
        if line != self.told_props {
            self.told_props = line.clone();
            let mut parts: Vec<String> = Vec::new();
            let mut cur = String::new();
            for r in &rows {
                if !cur.is_empty() && cur.len() + r.len() > 900 {
                    parts.push(std::mem::take(&mut cur));
                }
                if !cur.is_empty() {
                    cur.push('|');
                }
                cur.push_str(r);
            }
            for p in &parts {
                self.say_text("propsmore", p);
            }
            self.say_text("props", &cur);
        }
        // the status bar: the time and the funds on the left; the population
        // and the stars, and the news (or the palette's tool under the pointer)
        let stars = self.sim.star_count().clamp(0, 6) as usize;
        let news = match self.ui.hover_tool {
            Some(t) => {
                let kd = &TOOLS[t].kinds[self.ui.kind[t]];
                let cost = self.cost_text(kd);
                let what = [kd.gives, kd.hint].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join("; ");
                if cost.is_empty() {
                    format!("{}: {what}", kd.title)
                } else {
                    format!("{}, {cost}: {what}", kd.title)
                }
            }
            None => self.news.clone(),
        };
        let inspect = format!(
            "{}|{}|Population {}   {}|{}",
            self.clock_text(),
            money(self.sim.cash()),
            count(self.sim.population()),
            if stars == 6 { "Tower".to_string() } else { format!("{} star{}", stars, if stars == 1 { "" } else { "s" }) },
            news.replace('|', " ")
        );
        if inspect != self.told_inspect {
            self.told_inspect = inspect.clone();
            self.say_text("inspect", &inspect);
        }
        let ticks: Vec<u8> = vec![
            (self.hold.is_none()) as u8,
            (self.hold == Some(12 * 64)) as u8,
            (self.hold == Some(0)) as u8,
        ];
        if ticks != self.told_ticks {
            for (i, t) in ticks.iter().enumerate() {
                self.say(format!("tick 2 {i} {t}"));
            }
            self.told_ticks = ticks;
        }
    }

    fn cost_text(&self, kd: &Kind) -> String {
        let tile = kd.tile();
        let cost = |t: &str| TILE_COSTS.iter().find(|(n, _)| *n == t).map(|(_, c)| *c).unwrap_or(0);
        match kd.act {
            Place("floor", _) | Place("lobby", _) => format!("{} a cell", money(cost(tile))),
            Place("carElevator", _) => "$100,000 to B1".into(),
            Place(t, _) if amenities::kind(t).is_some() => money(amenities::kind(t).map(|k| k.cost).unwrap_or(0)),
            Place(_, _) => money(cost(tile)),
            AddCar => {
                let c = |m: i64| CARRIER_CAR_CONSTRUCTION_COST.iter().find(|(k, _)| *k == m).map(|(_, v)| *v).unwrap_or(0);
                format!("{} a car", money(c(1)))
            }
            _ => String::new(),
        }
    }

    fn tool_rows(&self, rows: &mut Vec<String>) {
        let t = &TOOLS[self.tool];
        let kd = self.kind();
        rows.push(format!("#{}", kd.title));
        if t.kinds.len() > 1 {
            for (i, other) in t.kinds.iter().enumerate() {
                rows.push(format!("*{}{}", if i == self.ui.kind[self.tool] { "=" } else { "" }, other.label));
            }
        }
        let cost = self.cost_text(kd);
        if !cost.is_empty() {
            rows.push(format!("Cost:{cost}"));
        }
        match kd.act {
            Place("carElevator", _) => rows.push("Deeper:$25,000 a floor".into()),
            Place(tile, 2) => {
                let mode = match tile {
                    "elevatorExpress" => 0,
                    "elevatorService" => 2,
                    _ => 1,
                };
                let ext = CARRIER_EXTEND_FLOOR_COST.iter().find(|(m, _)| *m == mode).map(|(_, c)| *c).unwrap_or(0);
                let car = CARRIER_CAR_CONSTRUCTION_COST.iter().find(|(m, _)| *m == mode).map(|(_, c)| *c).unwrap_or(0);
                rows.push(format!("Each floor:{}", money(ext)));
                rows.push(opt(4, format!("Each car:{}", money(car))));
            }
            AddCar => rows.push("Express:$150,000 a car".into()),
            _ => {}
        }
        if !kd.size.is_empty() {
            rows.push(format!("Size:{}", kd.size));
        }
        if !kd.needs.is_empty() {
            rows.push(format!("Needs:{}", kd.needs));
        }
        if !kd.gives.is_empty() {
            rows.push(format!("Gives:{}", kd.gives));
        }
        if !kd.income.is_empty() {
            rows.push(format!("Income:{}", kd.income));
        }
        if !kd.upkeep.is_empty() {
            rows.push(format!("Upkeep:{}", kd.upkeep));
        }
        let tile = kd.tile();
        if !tile.is_empty() {
            let need = get_tile_star_requirement(tile);
            rows.push(format!("Stars:{}", if need <= 1 { "any".to_string() } else { format!("{need} or more") }));
        }
        match self.refusal() {
            Some(why) => rows.push(format!("Refused:{why}")),
            None => {
                if !kd.hint.is_empty() {
                    rows.push(format!("Note:{}", kd.hint));
                }
            }
        }
    }

    fn news_rows(&self, rows: &mut Vec<String>, first: u8) {
        if self.ui.log.is_empty() {
            return;
        }
        rows.push(opt(first, "#News".into()));
        for (i, (t, _)) in self.ui.log.iter().enumerate() {
            rows.push(opt(if i == 0 { first } else { 9 }, format!("*{}. {}", i + 1, short(t, 30))));
        }
    }

    fn next_star_text(&self) -> String {
        let s = self.sim.star_count();
        match s {
            1 => "The next needs 300 people.".into(),
            2 => "The next needs 1,000 people and a security office.".into(),
            3 => "The next needs 5,000 people, offices, a medical centre and recycling.".into(),
            4 => "The next needs 10,000 people and a metro station.".into(),
            5 => "The Tower rating needs 15,000 people and a cathedral.".into(),
            _ => String::new(),
        }
    }

    fn tower_rows(&self, rows: &mut Vec<String>) {
        if !matches!(self.kind().act, Inspect) {
            self.tool_rows(rows);
        }
        rows.push("#Tower".into());
        rows.push(format!("Name:{}", self.title.replace('|', " ")));
        rows.push(opt(6, format!("Time:{}", self.clock_text())));
        rows.push(opt(7, format!("Date:{}", self.date_text())));
        rows.push(opt(6, format!("Funds:{}", money(self.sim.cash()))));
        rows.push(opt(6, format!("Population:{}", count(self.sim.population()))));
        let stars = self.sim.star_count().clamp(0, 6) as usize;
        rows.push(format!("Stars:{}", if stars == 0 { "none" } else { STARS[stars] }));
        rows.push(format!("Speed?{}?{}", SPEEDS[self.speed], SPEEDS.join(",")));
        // what the next star needs, and which of it is done
        let s = self.sim.star_count();
        if (1..=5).contains(&s) {
            let g = &self.sim.world().gate_flags;
            let want = STAR_THRESHOLDS[(s - 1) as usize];
            rows.push(opt(3, format!("#For {}", if s == 5 { "the Tower rating".to_string() } else { format!("{} stars", s + 1) })));
            rows.push(opt(3, format!("People:{} of {}", count(self.sim.population()), count(want))));
            let yes = |b: bool| if b { "done" } else { "not yet" };
            match s {
                2 => rows.push(opt(3, format!("Security:{}", yes(g.security_placed != 0)))),
                3 => {
                    rows.push(opt(3, format!("Medical:{}", yes(g.office_service_ok_medical != 0))));
                    rows.push(opt(3, format!("Recycling:{}", yes(g.recycling_adequate != 0))));
                    rows.push(opt(4, format!("Offices served:{}", yes(g.office_service_ok != 0))));
                }
                4 => {
                    rows.push(opt(3, format!("Metro:{}", yes(g.metro_placed != 0))));
                    rows.push(opt(3, format!("Recycling:{}", yes(g.recycling_adequate != 0))));
                }
                5 => rows.push(opt(3, "Cathedral:floor 103".into())),
                _ => {}
            }
        }
        // BOX RULES: a fire's evacuation (box_ext::fire)
        let fire = tower_sim::box_ext::fire::view(self.sim.world());
        if fire.alarm || !fire.out.is_empty() || !fire.closed.is_empty() {
            rows.push(if fire.alarm { "#Fire alarm" } else { "#After the fire" }.into());
            rows.push(format!("Out safely:{}", count(fire.out.len() as i64)));
            if !fire.closed.is_empty() {
                let mut closed = fire.closed.clone();
                closed.sort();
                let floors: Vec<String> = closed.iter().map(|&f| floor_name(f)).collect();
                rows.push(format!("Closed today:{}", short(&floors.join(" "), 22)));
            }
            rows.push(
                if fire.alarm { "Note:Everyone walks out" } else { "Note:All back in soon" }.into(),
            );
        }
        // BOX RULES: who lives and works here (box_ext::people)
        let r = tower_sim::box_ext::people::register(self.sim.world());
        if r.households > 0 || r.jobs_total > 0 {
            rows.push(opt(4, "#People".into()));
            if r.households > 0 {
                rows.push(opt(4, format!("Living here:{} in {} homes", count(r.adults + r.children), count(r.households))));
                rows.push(opt(5, format!("Children:{}", count(r.children))));
            }
            if r.guests > 0 {
                rows.push(opt(5, format!("Hotel guests:{}", count(r.guests))));
            }
            rows.push(opt(4, format!("Jobs here:{}", count(r.jobs_total))));
            rows.push(opt(5, format!("By residents:{}", count(r.held_by_residents))));
            rows.push(opt(5, format!("Commute in:{}", count(r.commuters))));
            // BOX RULES: office workers who come by metro (box_ext::metro)
            let w = self.sim.world();
            let by_train = (0..w.sims.len() as i64).filter(|&i| tower_sim::box_ext::metro::arrival_floor(w, i).is_some()).count();
            if by_train > 0 {
                rows.push(opt(5, format!("By train:{}", count(by_train as i64))));
            }
            if r.households > 0 {
                rows.push(opt(6, format!("Work outside:{}", count(r.working_outside))));
            }
            // BOX RULES: households' cars in the car park tonight
            let cars = self.sim.box_ext().vehicles.iter().filter(|v| v.resident).count();
            if cars > 0 {
                rows.push(opt(5, format!("Cars home:{}", count(cars as i64))));
            }
            if r.children > 0 {
                rows.push(opt(5, format!(
                    "School here:{} of {}",
                    count(r.kindergarten_here + r.school_here),
                    count(r.children)
                )));
            }
            for (trade, n) in tower_sim::box_ext::people::TRADES.iter().zip(r.jobs) {
                if n > 0 {
                    rows.push(opt(7, format!("{trade}:{} jobs", count(n))));
                }
            }
        }
        self.news_rows(rows, 5);
    }

    fn room_rows(&self, rows: &mut Vec<String>) {
        if matches!(self.kind().act, AddCar | RemoveCar) {
            self.tool_rows(rows);
        }
        let Some((x, y)) = self.selected else {
            rows.push("#Room".into());
            rows.push("Note:Inspect a room".into());
            return;
        };
        let q: QueryCellResult = self.sim.query_cell(x, y);
        if let Some(c) = &q.carrier_info {
            let kind = match c.carrier_mode {
                0 => "Express elevator",
                2 => "Service elevator",
                _ => "Elevator",
            };
            rows.push(format!("#{kind}"));
            rows.push(format!("Floors:{} to {}", floor_name(c.bottom_served_floor), floor_name(c.top_served_floor)));
            let served: Vec<String> = c.served_floors.iter().map(|&f| floor_name(f)).collect();
            rows.push(opt(5, format!("Stops at:{}", short(&served.join(" "), 22))));
            rows.push(format!("Cars:{} of {}", c.car_count, c.max_cars));
            for (i, car) in c.car_infos.iter().enumerate().filter(|(_, c)| c.active).take(4) {
                rows.push(opt(6, format!("Car {}:home floor {}", i + 1, floor_name(car.home_floor))));
            }
            if let Some(kk) = self.sim.live_carriers().iter().position(|cr| cr.carrier_id == c.carrier_id) {
                if let Some(w) = self.scene.waiting.get(kk) {
                    let (up, down) = w.iter().fold((0u32, 0u32), |a, b| (a.0 + b.0 as u32, a.1 + b.1 as u32));
                    rows.push(format!("Waiting:{up} up, {down} down"));
                }
            }
            rows.push("*Go there".into());
            return;
        }
        let tile = q.tile_type.clone();
        rows.push(format!("#{}", tile_title(&tile)));
        rows.push(format!("Floor:{} (cell {})", floor_name(floor_of_row(y)), q.anchor_x));
        if let Some(ce) = &q.car_elevator_info {
            let _ = ce;
            rows.push("Note:Cars to the basements".into());
        }
        if let Some(o) = &q.object_info {
            let key = format!("{},{}", q.anchor_x, y);
            let rec = self.sim.world().placed_objects.get(&key);
            let occupied = rec.map(|r| r.occupied_flag).unwrap_or(0);
            rows.push(format!("State:{}", room_state_text(&tile, o.unit_status, occupied)));
            let adjustable = matches!(
                tile.as_str(),
                "office" | "hotelSingle" | "hotelTwin" | "hotelSuite" | "retail" | "fastFood" | "restaurant"
            ) || (tile == "condo" && o.unit_status >= 0x18);
            if adjustable && (0..4).contains(&o.rent_level) {
                let what = if tile == "condo" { "Price" } else { "Rent" };
                rows.push(format!("{what}?{}?{}", RENTS[o.rent_level as usize], RENTS.join(",")));
            }
            rows.push(format!(
                "Evaluation:{}",
                ["Bad", "Fair", "Good"].get(o.eval_level as usize).copied().unwrap_or("Not yet")
            ));
            // BOX RULES: a hotel room's bookings (box_ext::hotels)
            if let Some(rate) = tower_sim::box_ext::hotels::booking_rate(self.sim.world(), &key) {
                rows.push(format!("Booked:{rate}% of nights"));
            }
            // the people whose home or post it is
            let floor = floor_of_row(y);
            let people = self
                .sim
                .live_sims()
                .iter()
                .filter(|s| s.floor_anchor == floor && s.home_column == q.anchor_x)
                .count();
            if people > 0 {
                rows.push(format!("People:{people}"));
            }
            if let Some(r) = rec {
                rows.push(format!("Stress:{}", stress_words(r.eval_score)));
            }
            // BOX RULES: how the evaluation is made up
            if let Some(b) = tower_sim::box_ext::rating::of(self.sim.world(), &key) {
                rows.push("#Rating".into());
                rows.push(format!("Trip stress:{}", b.trip_stress));
                match b.walk_cells {
                    Some(c) if b.walk_cost > 0 => rows.push(format!("Walk to lift:{c} cells, +{} a trip", b.walk_cost)),
                    Some(c) => rows.push(format!("Walk to lift:{c} cells")),
                    None => rows.push("Walk to lift:No lift or stairs".into()),
                }
                if b.rent != 0 {
                    rows.push(format!("Rent:{:+}", b.rent));
                }
                if b.noise != 0 {
                    rows.push(format!("Noisy neighbour:+{}", b.noise));
                }
                if b.relief != 0 {
                    rows.push(format!("Amenities:-{}", b.relief));
                }
                rows.push(format!("Score:{}", b.score));
                rows.push(format!("Good under:{}", b.fair));
                rows.push(format!("Fair under:{}", b.bad));
            }
        }
        if tile != "empty" {
            rows.push("*Bulldoze it".into());
        }
        rows.push("*Go there".into());
    }

    fn money_rows(&self, rows: &mut Vec<String>) {
        let l = self.sim.ledger();
        let inc = |i: i64| tower_sim::js::slot_at(&l.income_ledger, i).to_number().unwrap_or(0);
        let ex = |c: usize| l.expense_ledger.get(c).copied().unwrap_or(0);
        rows.push("#Funds".into());
        rows.push(format!("Funds:{}", money(self.sim.cash())));
        let delta = self.sim.cash() - l.cash_balance_cycle_base;
        rows.push(format!("This period:{}{}", if delta > 0 { "+" } else { "" }, money(delta)));
        rows.push(opt(8, "Note:A period is 3 days".into()));
        rows.push("#Income".into());
        let income = [
            ("Offices", inc(0)),
            ("Hotel rooms", inc(1) + inc(2) + inc(3)),
            ("Shops", inc(4)),
            ("Fast food", inc(5)),
            ("Restaurants", inc(6)),
            ("Party halls", inc(7)),
            ("Cinemas", inc(8)),
            ("Condominiums", inc(9)),
            // BOX RULES' own
            ("Parking fees", self.sim.box_ext().period_parking),
            ("Amenities", self.sim.box_ext().period_amenity_income),
        ];
        let mut any = false;
        for (name, v) in income {
            if v != 0 {
                rows.push(format!("{name}:{}", money(v)));
                any = true;
            }
        }
        if !any {
            rows.push("Note:None this period".into());
        }
        rows.push(format!("Total:{}", money(income.iter().map(|(_, v)| v).sum())));
        rows.push("#Expenses".into());
        let spend = [
            ("Elevators", ex(1) + ex(42) + ex(43)),
            ("Escalators", ex(27)),
            ("Security", ex(14)),
            ("Housekeeping", ex(15)),
            ("Recycling", ex(20)),
            ("Parking", ex(0x18)),
            ("Amenities", self.sim.box_ext().period_amenity_upkeep),
        ];
        let mut any = false;
        for (name, v) in spend {
            if v != 0 {
                rows.push(format!("{name}:{}", money(v)));
                any = true;
            }
        }
        if !any {
            rows.push("Note:None this period".into());
        }
        rows.push(format!("Total:{}", money(spend.iter().map(|(_, v)| v).sum())));
    }

    /// the rooms rated bad, the worst first: (title, cell, row)
    fn worst_rooms(&self) -> Vec<(String, i64, i64, i64)> {
        let mut v: Vec<(String, i64, i64, i64)> = Vec::new();
        for (key, r) in self.sim.world().placed_objects.iter() {
            if r.eval_level != 0 || !RATED.contains(&self.sim.world().cells.get(key).map(String::as_str).unwrap_or("")) {
                continue;
            }
            let Some((x, y)) = key.split_once(',').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))) else {
                continue;
            };
            let tile = self.sim.world().cells.get(key).cloned().unwrap_or_default();
            v.push((tile_title(&tile), x, y, r.eval_score));
        }
        v.sort_by(|a, b| b.3.cmp(&a.3));
        v
    }

    fn eval_rows(&self, rows: &mut Vec<String>) {
        rows.push(format!("Rating view!{}", on(self.look.view == 1)));
        let (mut good, mut fair, mut bad, mut none) = (0, 0, 0, 0);
        for (key, r) in self.sim.world().placed_objects.iter() {
            if !RATED.contains(&self.sim.world().cells.get(key).map(String::as_str).unwrap_or("")) {
                continue;
            }
            match r.eval_level {
                2 => good += 1,
                1 => fair += 1,
                0 => bad += 1,
                _ => none += 1,
            }
        }
        rows.push("#Rooms".into());
        rows.push(format!("Good:{good}"));
        rows.push(format!("Fair:{fair}"));
        rows.push(format!("Bad:{bad}"));
        rows.push(opt(6, format!("Not rated:{none}")));
        rows.push("#Complaints".into());
        let waiting: u32 = self.scene.waiting.iter().flat_map(|w| w.iter()).map(|(u, d)| *u as u32 + *d as u32).sum();
        rows.push(format!("Waiting:{waiting} people"));
        rows.push(opt(5, format!("Failed trips:{} today", self.ui.failures)));
        let worst = self.worst_rooms();
        if worst.is_empty() {
            rows.push("Note:No room rated bad".into());
        }
        for (i, (t, x, y, _)) in worst.iter().take(3).enumerate() {
            rows.push(opt(6 + i as u8, format!("*Bad: {t}, floor {} ({x})", floor_name(floor_of_row(*y)))));
        }
        self.news_rows(rows, 7);
    }

    fn option_rows(&self, rows: &mut Vec<String>) {
        rows.push("#Game".into());
        rows.push(format!("Speed?{}?{}", SPEEDS[self.speed], SPEEDS.join(",")));
        rows.push(format!("Build anything!{}", on(self.sim.free_build)));
        rows.push(format!("This tower:{}", if self.sim.box_rules() { "BOX rules" } else { "Classic rules" }));
        rows.push("#New towers".into());
        rows.push(format!("BOX rules!{}", on(self.ui.new_box_rules)));
        rows.push("#View".into());
        rows.push(format!("Rating view!{}", on(self.look.view == 1)));
        let clock = match self.hold {
            None => 0,
            Some(0) => 2,
            _ => 1,
        };
        rows.push(format!("Clock?{}?{}", CLOCKS[clock], CLOCKS.join(",")));
        // the quality the presenters draw (graphics design 7.1)
        let q = (self.look.quality as usize).min(QUALITIES.len() - 1);
        rows.push(format!("Graphics?{}?{}", QUALITIES[q], QUALITIES.join(",")));
    }

    /// "prop NAME [VALUE]" from the pane
    pub fn prop(&mut self, rest: &str) {
        if let Some(p) = PAGES.iter().position(|p| rest == *p) {
            self.ui.page = p;
        } else if let Some(i) = TOOLS[self.tool].kinds.iter().position(|kd| kd.label == rest && !rest.is_empty()) {
            self.ui.kind[self.tool] = i;
            self.dirty = true;
        } else if let Some(v) = arg(rest, "Speed") {
            if let Some(i) = SPEEDS.iter().position(|s| *s == v) {
                if self.ui.held_speed.is_some() {
                    self.ui.held_speed = Some(i); // the question first; then this speed
                } else {
                    self.speed = i;
                }
            }
        } else if let Some(v) = arg(rest, "Rent").or_else(|| arg(rest, "Price")) {
            if let (Some(lv), Some((x, y))) = (RENTS.iter().position(|r| *r == v), self.selected) {
                let r = self.sim.submit_command(&SimCommand::SetRentLevel { x, y, rent_level: lv as i64 });
                if r.accepted {
                    self.touched();
                } else if let Some(why) = r.reason {
                    self.news = why;
                }
            }
        } else if let Some(v) = arg(rest, "Clock") {
            self.hold = match v {
                "Noon" => Some(12 * 64),
                "Midnight" => Some(0),
                _ => None,
            };
            self.dirty = true;
        } else if let Some(v) = arg(rest, "Graphics") {
            if let Some(q) = QUALITIES.iter().position(|x| *x == v) {
                self.look.quality = q as u8;
                self.dirty = true;
            }
        } else if arg(rest, "Build anything").is_some() {
            self.sim.free_build = !self.sim.free_build;
        } else if arg(rest, "BOX rules").is_some() {
            self.ui.new_box_rules = !self.ui.new_box_rules;
        } else if arg(rest, "Rating view").is_some() {
            self.look.view = if self.look.view == 1 { 0 } else { 1 };
            self.dirty = true;
        } else if rest == "Go there" {
            if let Some(c) = self.selected {
                self.go_to(c);
            }
        } else if rest == "Bulldoze it" {
            if let Some((x, y)) = self.selected {
                let r = self.sim.submit_command(&SimCommand::RemoveTile { x, y });
                if r.accepted {
                    self.touched();
                    self.news = "Bulldozed.".into();
                } else if let Some(why) = r.reason {
                    self.news = why;
                }
            }
        } else if let Some(t) = rest.strip_prefix("Bad: ") {
            let _ = t;
            let worst = self.worst_rooms();
            if let Some((_, x, y, _)) = worst.iter().find(|(t, x, y, _)| {
                rest == format!("Bad: {t}, floor {} ({x})", floor_name(floor_of_row(*y)))
            }) {
                self.selected = Some((*x, *y));
                self.go_to((*x, *y));
                self.ui.page = P_ROOM;
            }
        } else if let Some((n, _)) = rest.split_once(". ") {
            // a news item: where it happened
            if let Some((_, Some(c))) = n.parse::<usize>().ok().and_then(|i| self.ui.log.get(i.wrapping_sub(1))).cloned() {
                self.selected = Some(c);
                self.go_to(c);
                self.dirty = true;
            }
        }
        self.panes();
    }

    /// the view to a cell
    pub fn go_to(&mut self, (x, y): (i64, i64)) {
        let kz = self.zoom;
        self.say(format!(
            "centre {} {}",
            ((x as f64 + 0.5) * UNIT_W as f64 * kz) as i64,
            ((y as f64 + 0.5) * UNIT_H as f64 * kz) as i64
        ));
        self.dirty = true;
    }

    /// For the play tests: when a file event.request is beside the engine,
    /// the event it names ("fire" or "bomb") starts now, or ("collapse")
    /// someone in the tower collapses, or ("star") a new star's celebration
    /// is drawn, and the request is removed (as
    /// dump.request)
    pub fn event_if_asked(&mut self) {
        let req = self.dir.join("event.request");
        let Ok(what) = std::fs::read_to_string(&req) else { return };
        let _ = std::fs::remove_file(&req);
        if what.trim() == "star" {
            // the celebration alone, as a new star has it
            self.scene.celebrate();
            self.dirty = true;
            return;
        }
        if what.trim() == "collapse" {
            // incidents are BOX RULES': a fixture tower is a classic one
            self.sim.set_box_rules(true);
            let n = self.sim.live_sims().len() as i64;
            // the sims in turn, from the middle of the list, until one is in
            // the tower to collapse
            if !(0..n).any(|k| self.sim.box_collapse_sim((n / 2 + k) % n)) {
                self.news = "No one could collapse.".into();
            }
            self.dirty = true;
            return;
        }
        let fire = what.trim() == "fire";
        if !self.sim.box_test_event(fire) {
            self.news = "The test event could not start.".into();
        }
        let p = self.sim.drain_prompts();
        self.prompts(p);
        self.watch_events();
        self.dirty = true;
    }
}

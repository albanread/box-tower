//! tower-sim: tower-together's simulation (github.com/phulin/tower-together,
//! apps/worker/src/sim at 073c0e4, MIT), translated to Rust file for file.
//! TRANSLATION.md has the rules; upstream's index.ts (the TowerSim class)
//! is `index.rs`.

pub mod carriers;
pub mod cathedral;
pub mod commands;
pub mod daily;
pub mod entertainment;
pub mod events;
pub mod families;
pub mod index;
pub mod js;
pub mod ledger;
pub mod progression;
pub mod queue;
pub mod reachability;
pub mod recycling;
pub mod resources;
pub mod ring_buffer;
pub mod route_scoring;
pub mod scheduler;
pub mod sim_access;
pub mod sim_office;
pub mod sim_refresh;
pub mod sims;
pub mod snapshot;
pub mod stress;
pub mod tick;
pub mod time;
pub mod world;

// BOX: additions of our own (car elevator); not a translation.
pub mod box_ext;

pub use index::TowerSim;

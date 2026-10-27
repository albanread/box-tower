//! tower-surface: the tower's GPU surface (ROS_SURF_TOWER), shared by the
//! engine that publishes it and the tools that prove it.
//!
//! - `block`: rosgd/include/rosgd/tower.h in Rust (`#[repr(C)]`, byte for
//!   byte; tests/layout.rs checks it against the C compiler).
//! - `signs`: the signs' sprite numbers (sprites 1-255).
//! - `pack`: the art packer, `towerpack`'s library (design 10.1).
//! - `twin`: the CPU twin of the presenters' shaders (design 8.5).
//!
//! The design is rosgd/ports/tower/docs/graphics-design.md.

pub mod block;
pub mod pack;
pub mod signs;
pub mod twin;

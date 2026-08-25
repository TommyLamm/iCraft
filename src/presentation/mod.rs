//! Desktop-only presentation modules extracted from `state.rs`.
//!
//! Declared from the desktop binary (`main.rs`). Do not add `pub mod presentation`
//! to `lib.rs` — that would pull wgpu GPU types into icraft-server.
//!
//! `legacy_sim.rs`, `legacy_systems.rs`, `legacy_interaction.rs`,
//! `frame.rs`, `embedded_runtime.rs`, and `network_event.rs` live in this
//! directory but are loaded as children of `state` (`#[path]`) so leftover
//! presentation, tick, render, and bridge methods can see private `State` fields.
//! Leftover simulation and interaction modules compile only under `cfg(test)` or
//! feature `legacy_owner`.

pub(crate) mod bootstrap;
pub(crate) mod gpu_terrain;
pub(crate) mod interpolation;
pub(crate) mod network_inbound;

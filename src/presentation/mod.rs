//! Desktop-only presentation modules extracted from `state.rs`.
//!
//! Declared from the desktop binary (`main.rs`). Do not add `pub mod presentation`
//! to `lib.rs` — that would pull wgpu GPU types into icraft-server.
//!
//! `legacy_sim.rs` and `frame.rs` live in this directory but are loaded as
//! children of `state` (`#[path]`) so leftover tick / render methods can see
//! private `State` fields.

pub(crate) mod bootstrap;
pub(crate) mod gpu_terrain;
pub(crate) mod interpolation;
pub(crate) mod network_inbound;

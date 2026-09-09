//! Desktop-only presentation modules extracted from `state.rs`.
//!
//! Declared from the desktop binary (`main.rs`). Do not add `pub mod presentation`
//! to `lib.rs` — that would pull wgpu GPU types into icraft-server.
//!
//! `frame.rs`, `embedded_runtime.rs`, and `network_event.rs` live in this
//! directory but are loaded as children of `state` (`#[path]`) so tick,
//! render, and bridge methods can see private `State` fields.

pub(crate) mod bootstrap;
pub(crate) mod gpu_terrain;
pub(crate) mod interpolation;
pub(crate) mod network_inbound;

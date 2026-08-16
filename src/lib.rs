//! iCraft shared library.
//!
//! The desktop binary still owns the winit/wgpu application loop in
//! `main.rs`. Keeping the simulation/network modules in a library target lets
//! the dedicated server reuse the authoritative code without constructing a
//! window, audio device, or GPU surface.
//!
//! # Server / tests contract
//!
//! `icraft-server` and `tests/` may `use icraft::…` only the `pub` modules
//! below: authority, world, network, persistence, and the thin
//! `presentation_inventory_policy` cut. That set is the live contract.
//!
//! # Crate-internal / desktop-adjacent
//!
//! Everything else is `pub(crate)`. Those modules still compile into the
//! library (mesh leftover, audio hooks, GPU frame bookkeeping) but are not
//! a server or integration-test API. `sim_harness`, `final_acceptance`, and
//! `microbench` compile only under `cfg(test)` or feature `harness`.
//!
//! `src/presentation/` is the Plan 10 desktop fence and **must not** be
//! added to this library. GPU menu, terrain arenas, and frame encode stay
//! out of `icraft-server`.

// Server / tests contract. Keep `pub` only for modules that `tests/` or
// `src/bin/icraft-server.rs` actually `use icraft::…`.
pub mod authority;
pub mod block_entity;
pub mod brewing;
pub mod chunk_manager;
pub mod container_sessions;
pub mod dimension;
pub mod enchantment;
pub mod entity;
pub mod fishing;
pub mod game_rules;
pub mod inventory;
pub mod network;
pub mod passive_mob;
pub mod player;
pub mod presentation_inventory_policy;
pub mod redstone;
pub mod save;
pub mod server_runtime;
pub mod server_world;
pub mod structure;
pub mod world;

// Crate-internal / desktop-adjacent. Still compiled (except harness cfg).
pub(crate) mod accessibility;
pub(crate) mod advancements;
pub(crate) mod ai;
pub(crate) mod audio;
pub(crate) mod block_model;
pub(crate) mod boss;
pub(crate) mod chunk_render;
pub(crate) mod chunk_schedule;
pub(crate) mod commands;
pub(crate) mod crafting;
pub(crate) mod culling;
#[cfg(any(test, feature = "harness"))]
pub(crate) mod final_acceptance;
pub(crate) mod fluid;
pub(crate) mod gpu_frame_resources;
pub(crate) mod interaction;
pub(crate) mod lighting;
pub(crate) mod localization;
pub(crate) mod loot;
#[cfg(any(test, feature = "harness"))]
pub(crate) mod microbench;
pub(crate) mod mob;
pub(crate) mod navigation;
pub(crate) mod perf;
pub(crate) mod physics;
pub(crate) mod presentation_click;
pub(crate) mod rail;
pub(crate) mod recipes;
pub(crate) mod resources;
#[cfg(any(test, feature = "harness"))]
pub(crate) mod sim_harness;
pub(crate) mod spawning;
pub(crate) mod vehicle;
pub(crate) mod village;
pub(crate) mod voxel_shape;
pub(crate) mod weather;
pub(crate) mod world_mutation;
pub(crate) mod world_tick;
pub(crate) mod worldgen;

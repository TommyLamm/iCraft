//! iCraft shared library.
//!
//! The desktop binary owns the winit/wgpu application loop in `main.rs`
//! and re-exports these modules (`pub use icraft::{world, …}`) so existing
//! `crate::world` paths in the desktop tree still resolve. Shared source
//! therefore compiles once, through this library.
//!
//! # Server / tests contract
//!
//! `icraft-server` and `tests/` may `use icraft::…` only the contract
//! modules below: authority, world, network, persistence, and the thin
//! `presentation_inventory_policy` cut. That set is the live server API.
//!
//! # Desktop-shared modules
//!
//! Additional modules are `pub` so the desktop binary crate can re-export
//! them. They are not a dedicated-server or integration-test API.
//! `sim_harness`, `final_acceptance`, and `microbench` compile only under
//! `cfg(test)` or feature `harness`.
//!
//! GPU frame pooling (`gpu_frame_resources`) and presentation click policy
//! (`presentation_click`) are desktop binary modules in `main.rs`. Do not
//! add them here — that would compile them into `icraft-server`.
//!
//! `src/presentation/` is the Plan 10 desktop fence and **must not** be
//! added to this library. GPU menu, terrain arenas, and frame encode stay
//! out of `icraft-server`.

// Server / tests contract. Keep this set aligned with `tests/` and
// `src/bin/icraft-server.rs` `use icraft::…` imports.
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

// Desktop-shared. `pub` so `src/main.rs` can `pub use icraft::…` without
// compiling these files a second time into the binary crate.
pub mod accessibility;
pub mod advancements;
pub mod block_model;
pub mod boss;
pub mod chunk_render;
pub mod chunk_schedule;
pub mod commands;
pub mod culling;
pub mod interaction;
pub mod lighting;
pub mod localization;
pub mod navigation;
pub mod perf;
pub mod physics;
pub mod resources;
pub mod vehicle;
pub mod village;
pub mod weather;

// Shared simulation used by authority / ServerWorld. Not a dedicated-server
// or integration-test import; desktop files do not `use crate::` these.
pub(crate) mod fluid;
pub(crate) mod mob;
pub(crate) mod rail;
pub(crate) mod world_tick;

// Still crate-internal. Desktop-only files do not `use crate::` these.
#[cfg(any(test, feature = "harness"))]
pub(crate) mod final_acceptance;
pub(crate) mod loot;
// `pub` because desktop `State` and `ServerWorld` expose `RecipeManager`.
pub mod recipes;
#[cfg(any(test, feature = "harness"))]
pub(crate) mod microbench;
#[cfg(any(test, feature = "harness"))]
pub(crate) mod sim_harness;
pub(crate) mod voxel_shape;
pub(crate) mod worldgen;

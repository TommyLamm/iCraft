//! iCraft shared library.
//!
//! The desktop binary still owns the winit/wgpu application loop in
//! `main.rs`.  Keeping the simulation/network modules in a library target lets
//! the dedicated server reuse the authoritative code without constructing a
//! window, audio device, or GPU surface.

pub mod accessibility;
pub mod advancements;
pub mod ai;
pub mod audio;
pub mod authority;
pub mod block_entity;
pub mod block_model;
pub mod boss;
pub mod brewing;
pub mod camera;
pub mod chunk_manager;
pub mod chunk_render;
pub mod chunk_schedule;
pub mod commands;
pub mod container_sessions;
pub mod crafting;
pub mod culling;
pub mod dimension;
pub mod enchantment;
pub mod entity;
pub mod final_acceptance;
pub mod fishing;
pub mod fluid;
pub mod game_rules;
pub mod gpu_frame_resources;
pub mod interaction;
pub mod inventory;
pub mod lighting;
pub mod localization;
pub mod loot;
pub mod menu;
pub mod microbench;
pub mod mob;
pub mod navigation;
pub mod network;
pub mod passive_mob;
pub mod perf;
pub mod physics;
pub mod player;
pub mod rail;
pub mod recipes;
pub mod redstone;
pub mod resources;
pub mod save;
pub mod server_world;
pub mod sim_harness;
pub mod spawning;
pub mod structure;
pub mod texture;
pub mod vehicle;
pub mod village;
pub mod voxel_shape;
pub mod weather;
pub mod world;
pub mod world_mutation;
pub mod world_tick;
pub mod worldgen;

pub mod server_runtime;

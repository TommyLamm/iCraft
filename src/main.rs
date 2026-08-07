pub mod advancements;
pub mod ai;
mod app;
pub mod audio;
pub mod block_entity;
mod block_model;
mod boss;
mod brewing;
mod camera;
pub(crate) mod chunk_manager;
mod chunk_render;
mod chunk_schedule;
pub mod commands;
mod container_sessions;
mod crafting;
mod culling;
mod dimension;
mod enchantment;
mod entity;
pub mod fishing;
mod fluid;
pub mod game_rules;
pub mod gpu_frame_resources;
mod hand_renderer;
mod interaction;
mod inventory;
mod lighting;
pub mod loot;
mod menu;
pub(crate) mod microbench;
mod mob;
mod mob_renderer;
pub mod navigation;
pub(crate) mod network;
mod particles;
mod passive_mob;
mod perf;
pub(crate) mod physics;
mod player;
pub mod rail;
pub mod recipes;
mod redstone;
pub mod save;
pub mod sim_harness;
pub mod spawning;
mod state;
pub mod structure;
mod texture;
pub mod vehicle;
pub mod village;
pub mod voxel_shape;
mod weather;
pub(crate) mod world;
pub mod world_mutation;
pub mod world_tick;
mod worldgen;

use app::App;
use winit::event_loop::EventLoop;

fn wants_microbench<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == "--microbench")
}

fn main() {
    if wants_microbench(std::env::args()) {
        let _ = microbench::run();
        return;
    }

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    let _ = event_loop.run_app(&mut app);
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::wants_microbench;

    #[test]
    fn microbench_flag_is_selected_without_affecting_other_args() {
        assert!(wants_microbench(["mc", "--microbench"]));
        assert!(!wants_microbench(["mc", "--help"]));
    }
}

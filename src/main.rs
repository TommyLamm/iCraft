pub mod accessibility;
pub mod advancements;
pub mod ai;
mod app;
pub mod audio;
pub mod authority;
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
#[allow(dead_code)]
mod dynamic_resolution;
mod enchantment;
mod entity;
#[cfg(any(test, feature = "harness"))]
mod final_acceptance;
pub mod fishing;
mod fluid;
pub mod game_rules;
pub mod gpu_frame_resources;
mod hand_renderer;
mod interaction;
mod inventory;
mod lighting;
pub mod localization;
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
mod presentation;
mod presentation_click;
mod presentation_inventory_policy;
pub mod rail;
pub mod recipes;
mod redstone;
pub mod resources;
pub mod save;
mod server_runtime;
pub mod server_world;
#[cfg(any(test, feature = "harness"))]
mod sim_harness;
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

#[global_allocator]
static GLOBAL_ALLOCATOR: perf::AllocTracker = perf::AllocTracker;

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

    #[test]
    fn thread_alloc_count_is_local_to_calling_thread() {
        let handle = std::thread::spawn(|| {
            let _allocation = Box::new([0u8; 64]);
        });
        let caller_after_spawn = crate::perf::thread_alloc_count();
        handle.join().unwrap();
        assert_eq!(crate::perf::thread_alloc_count(), caller_after_spawn);

        let before = crate::perf::thread_alloc_count();
        let _allocation = Box::new([0u8; 64]);
        assert!(crate::perf::thread_alloc_count() > before);
    }
}

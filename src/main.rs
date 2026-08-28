//! Desktop entrypoint.
//!
//! Shared gameplay/network modules come from the `icraft` library via
//! `pub use` so desktop files can keep `crate::world` (and friends) without
//! compiling those sources a second time. Desktop-only GPU/menu modules
//! stay declared here and must not be added to `lib.rs`.

pub use icraft::{
    accessibility, advancements, authority, block_entity, block_model, boss, brewing,
    chunk_manager, chunk_render, chunk_schedule, commands, container_sessions, culling, dimension,
    enchantment, entity, fishing, fluid, game_rules, gpu_frame_resources, interaction, inventory,
    lighting, localization, microbench, mob, navigation, network, passive_mob, perf, physics,
    player, presentation_click, presentation_inventory_policy, rail, recipes, redstone, resources,
    save, server_runtime, server_world, structure, vehicle, village, weather, world,
    world_mutation, world_tick,
};

mod app;
mod audio;
mod camera;
#[allow(dead_code)]
mod dynamic_resolution;
mod hand_renderer;
mod menu;
mod mob_renderer;
mod particles;
mod presentation;
mod state;
mod texture;

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

//! Desktop entrypoint.
//!
//! Shared gameplay/network modules come from the `icraft` library via
//! `pub use` so desktop files can keep `crate::world` (and friends) without
//! compiling those sources a second time. Desktop-only GPU/menu modules
//! stay declared here and must not be added to `lib.rs`.
//! `--microbench` uses this crate's `mod microbench` behind feature
//! `microbench` (`cargo run --features microbench -- --microbench`).

pub use icraft::{
    authority, block_entity, block_model, boss, brewing, chunk_manager, chunk_render,
    chunk_schedule, commands, dimension, enchantment, entity, fishing, game_rules, interaction,
    inventory, lighting, navigation, network, passive_mob, perf, physics, player,
    presentation_inventory_policy, recipes, redstone, resources, save, server_runtime, server_world,
    structure, village, world,
};

// Desktop-only (Wave 10 Plan 06): keep GPU/UI/lang/LOS worker out of icraft-server.
mod accessibility;
mod advancements;
mod localization;
mod weather;

/// Section visibility + entity LOS worker (desktop-only; not in `icraft` lib).
#[path = "presentation/visibility.rs"]
mod culling_visibility;

/// Lib LOS/connectivity plus desktop section-visibility / entity LOS worker.
pub mod culling {
    pub use icraft::culling::*;
    pub use icraft::culling::{connectivity, los};
    pub use crate::culling_visibility::*;
}

mod app;
mod audio;
mod camera;
mod gpu_frame_resources;
mod hand_renderer;
mod menu;
#[cfg(feature = "microbench")]
mod microbench;
mod mob_renderer;
mod particles;
mod presentation;
mod presentation_click;
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
        #[cfg(feature = "microbench")]
        {
            let _ = microbench::run();
            return;
        }
        #[cfg(not(feature = "microbench"))]
        {
            eprintln!(
                "--microbench requires feature `microbench`: cargo run --features microbench -- --microbench"
            );
            std::process::exit(2);
        }
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

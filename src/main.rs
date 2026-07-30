pub mod advancements;
mod app;
pub mod audio;
mod boss;
mod brewing;
mod camera;
pub(crate) mod chunk_manager;
mod chunk_render;
mod chunk_schedule;
mod crafting;
mod culling;
mod dimension;
mod enchantment;
mod entity;
mod fluid;
pub mod gpu_frame_resources;
mod hand_renderer;
mod interaction;
mod inventory;
mod lighting;
mod menu;
pub(crate) mod microbench;
mod mob;
mod mob_renderer;
pub(crate) mod network;
mod particles;
mod passive_mob;
mod perf;
pub(crate) mod physics;
mod player;
mod redstone;
pub mod save;
pub mod sim_harness;
mod state;
mod texture;
mod weather;
pub(crate) mod world;

use app::App;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    let _ = event_loop.run_app(&mut app);
    std::process::exit(0);
}

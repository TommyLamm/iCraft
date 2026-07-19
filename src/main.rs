mod app;
pub mod audio;
mod camera;
mod chunk_manager;
mod crafting;
mod entity;
mod fluid;
mod interaction;
mod inventory;
mod lighting;
mod mob;
mod mob_renderer;
mod passive_mob;
mod physics;
mod player;
mod state;
mod texture;
mod world;

use app::App;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    let _ = event_loop.run_app(&mut app);
    std::process::exit(0);
}

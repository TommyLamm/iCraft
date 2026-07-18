mod app;
mod state;
mod camera;
mod world;
mod physics;
mod interaction;
mod texture;
mod chunk_manager;
mod lighting;
mod inventory;
mod crafting;
mod player;
mod entity;
mod mob;


use app::App;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    let _ = event_loop.run_app(&mut app);
    std::process::exit(0);
}


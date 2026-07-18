use crate::state::State;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

pub struct App {
    state: Option<State>,
    last_render_time: Instant,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: None,
            last_render_time: Instant::now(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            let window = event_loop
                .create_window(Window::default_attributes().with_title("Minecraft wgpu Clone"))
                .unwrap();
            let state = pollster::block_on(State::new(window));
            self.state = Some(state);
            self.last_render_time = Instant::now();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state,
                        physical_key,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if let Some(state) = &mut self.state {
                    match physical_key {
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyW) => {
                            state.keys.w = pressed;
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyS) => {
                            state.keys.s = pressed;
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyA) => {
                            state.keys.a = pressed;
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyD) => {
                            state.keys.d = pressed;
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Space) => {
                            state.keys.space = pressed;
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape) => {
                            if pressed {
                                event_loop.exit();
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(state) = &mut self.state {
                    state.resize(physical_size);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(state) = &mut self.state {
                    let now = Instant::now();
                    let dt = now.duration_since(self.last_render_time).as_secs_f32();
                    self.last_render_time = now;

                    // Cap delta time to prevent physics anomalies
                    let dt = dt.min(0.1);

                    state.update(dt);
                    state.window.request_redraw();

                    match state.render() {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => eprintln!("{:?}", e),
                    }
                }
            }
            _ => {}
        }
    }
}

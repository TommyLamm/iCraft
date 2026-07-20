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
            let window_size = winit::dpi::PhysicalSize::new(2560, 1440);
            let window = event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Minecraft wgpu Clone")
                        .with_inner_size(window_size),
                )
                .unwrap();

            // Center the window on the screen
            if let Some(monitor) = window.primary_monitor() {
                let monitor_size = monitor.size();
                let x = (monitor_size.width as i32 - window_size.width as i32) / 2;
                let y = (monitor_size.height as i32 - window_size.height as i32) / 2;
                window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
            }

            // Grab and hide the cursor for first-person controls
            let _ = window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(winit::window::CursorGrabMode::Confined));
            window.set_cursor_visible(false);

            let state = pollster::block_on(State::new(window));
            self.state = Some(state);
            self.last_render_time = Instant::now();
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if let Some(state) = &mut self.state {
                if !state.is_paused && state.window.has_focus() {
                    let sensitivity = state.sensitivity;
                    state.camera.yaw -= (delta.0 * sensitivity as f64) as f32;
                    state.camera.pitch -= (delta.1 * sensitivity as f64) as f32;

                    // Clamp pitch to prevent flipping upside down
                    let max_pitch = f32::to_radians(89.0);
                    state.camera.pitch = state.camera.pitch.clamp(-max_pitch, max_pitch);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(state) = &mut self.state {
                    state.is_saving = true;
                    let _ = state.render();
                    state.save_synchronously();
                }
                event_loop.exit();
            }
            WindowEvent::Focused(focused) => {
                println!("[Debug] Window focus changed to: {}", focused);
                if !focused {
                    if let Some(state) = &mut self.state {
                        state.set_paused(true);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(state) = &mut self.state {
                    state.handle_mouse_move(position.x, position.y);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state,
                        physical_key,
                        logical_key,
                        repeat,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if let Some(state) = &mut self.state {
                    if pressed
                        && state.active_station == Some(crate::state::StationKind::Anvil)
                        && state.inventory.is_open
                    {
                        match &logical_key {
                            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => {
                                state.anvil.rename.pop();
                                state.anvil.refresh();
                                return;
                            }
                            winit::keyboard::Key::Character(text) if !repeat => {
                                for ch in text
                                    .chars()
                                    .filter(|ch| ch.is_ascii_alphanumeric() || *ch == ' ')
                                {
                                    if state.anvil.rename.len() < 24 {
                                        state.anvil.rename.push(ch);
                                    }
                                }
                                state.anvil.refresh();
                                return;
                            }
                            _ => {}
                        }
                    }
                    match physical_key {
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape) => {
                            if pressed {
                                if state.inventory.is_open {
                                    state.close_inventory();
                                } else {
                                    let new_paused = !state.is_paused;
                                    state.set_paused(new_paused);
                                }
                            }
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyE) => {
                            if pressed {
                                if state.inventory.is_open {
                                    state.close_inventory();
                                } else if !state.is_paused {
                                    state.open_inventory();
                                }
                            }
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::F3) => {
                            if pressed && !repeat {
                                state.show_debug = !state.show_debug;
                            }
                        }
                        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyT) => {
                            state.keys.t = pressed;
                        }
                        _ => {
                            if !state.is_paused && !state.inventory.is_open {
                                match physical_key {
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::KeyW,
                                    ) => {
                                        state.keys.w = pressed;
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::KeyS,
                                    ) => {
                                        state.keys.s = pressed;
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::KeyA,
                                    ) => {
                                        state.keys.a = pressed;
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::KeyD,
                                    ) => {
                                        state.keys.d = pressed;
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Space,
                                    ) => {
                                        state.keys.space = pressed;
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::ControlLeft,
                                    ) => {
                                        state.keys.ctrl = pressed;
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::ShiftLeft,
                                    ) => {
                                        state.keys.shift = pressed;
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit1,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 0;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit2,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 1;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit3,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 2;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit4,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 3;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit5,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 4;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit6,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 5;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit7,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 6;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit8,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 7;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::Digit9,
                                    ) => {
                                        if pressed {
                                            state.inventory.selected = 8;
                                        }
                                    }
                                    winit::keyboard::PhysicalKey::Code(
                                        winit::keyboard::KeyCode::KeyG,
                                    ) => {
                                        if pressed {
                                            state.game_mode = match state.game_mode {
                                                crate::inventory::GameMode::Creative => {
                                                    crate::inventory::GameMode::Survival
                                                }
                                                crate::inventory::GameMode::Survival => {
                                                    crate::inventory::GameMode::Creative
                                                }
                                            };
                                            println!(
                                                "[Debug] Game mode changed to: {:?}",
                                                state.game_mode
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                if let Some(state) = &mut self.state {
                    let pressed = element_state == ElementState::Pressed;
                    if state.player_state.is_dead {
                        if pressed && button == MouseButton::Left {
                            state.handle_death_click();
                        }
                    } else if state.is_paused {
                        if pressed && button == MouseButton::Left {
                            state.handle_menu_click(event_loop);
                        }
                    } else if state.inventory.is_open {
                        if pressed && (button == MouseButton::Left || button == MouseButton::Right)
                        {
                            state.handle_inventory_click(button == MouseButton::Left);
                        }
                    } else {
                        match button {
                            MouseButton::Left => {
                                state.left_mouse_pressed = pressed;
                                if pressed {
                                    // Initial click triggers instant check for Creative mode
                                    if state.game_mode == crate::inventory::GameMode::Creative {
                                        state.handle_click(true);
                                    }
                                }
                            }
                            MouseButton::Right => {
                                if pressed {
                                    state.handle_click(false);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(state) = &mut self.state {
                    if !state.is_paused && !state.inventory.is_open {
                        let scroll_dir = match delta {
                            winit::event::MouseScrollDelta::LineDelta(_, y) => {
                                if y > 0.0 {
                                    -1
                                } else if y < 0.0 {
                                    1
                                } else {
                                    0
                                }
                            }
                            winit::event::MouseScrollDelta::PixelDelta(pos) => {
                                if pos.y > 0.0 {
                                    -1
                                } else if pos.y < 0.0 {
                                    1
                                } else {
                                    0
                                }
                            }
                        };
                        if scroll_dir != 0 {
                            let new_sel =
                                (state.inventory.selected as i32 + scroll_dir).rem_euclid(9);
                            state.inventory.selected = new_sel as usize;
                        }
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
                    if !state.window.has_focus() && !state.is_paused {
                        state.set_paused(true);
                    }
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

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.state = None;
        std::process::exit(0);
    }
}

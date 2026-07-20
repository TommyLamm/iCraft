use crate::menu::{GameSettings, Menu, MenuAction};
use crate::state::State;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

enum Runtime {
    Menu(Menu),
    Game(State),
}

pub struct App {
    runtime: Option<Runtime>,
    window: Option<Arc<Window>>,
    last_render_time: Instant,
}

impl App {
    pub fn new() -> Self {
        Self {
            runtime: None,
            window: None,
            last_render_time: Instant::now(),
        }
    }

    fn handle_menu_action(&mut self, action: MenuAction, event_loop: &ActiveEventLoop) {
        match action {
            MenuAction::None => {}
            MenuAction::Quit => event_loop.exit(),
            MenuAction::Launch(launch, settings) => {
                let Some(window) = self.window.clone() else {
                    return;
                };
                self.runtime.take();
                let mut state = pollster::block_on(State::new(window, launch, settings));
                state.set_paused(false);
                self.runtime = Some(Runtime::Game(state));
                self.last_render_time = Instant::now();
            }
        }
    }

    fn return_to_main_menu(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        self.runtime.take();
        let settings = GameSettings::load();
        let menu = pollster::block_on(Menu::new(window, settings));
        self.runtime = Some(Runtime::Menu(menu));
        self.last_render_time = Instant::now();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.runtime.is_some() {
            return;
        }
        let window_size = winit::dpi::PhysicalSize::new(1280, 720);
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("iCraft")
                        .with_inner_size(window_size),
                )
                .expect("Could not create window"),
        );
        if let Some(monitor) = window.primary_monitor() {
            let monitor_size = monitor.size();
            let x = (monitor_size.width as i32 - window_size.width as i32) / 2;
            let y = (monitor_size.height as i32 - window_size.height as i32) / 2;
            window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        let settings = GameSettings::load();
        let menu = pollster::block_on(Menu::new(window.clone(), settings));
        self.window = Some(window);
        self.runtime = Some(Runtime::Menu(menu));
        self.last_render_time = Instant::now();
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        let Some(Runtime::Game(state)) = &mut self.runtime else {
            return;
        };
        if let DeviceEvent::MouseMotion { delta } = event {
            if !state.is_paused && state.window.has_focus() {
                let sensitivity = state.sensitivity;
                state.camera.yaw -= (delta.0 * sensitivity as f64) as f32;
                state.camera.pitch -= (delta.1 * sensitivity as f64) as f32;
                let max_pitch = f32::to_radians(89.0);
                state.camera.pitch = state.camera.pitch.clamp(-max_pitch, max_pitch);
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
                if let Some(Runtime::Game(state)) = &mut self.runtime {
                    state.is_saving = true;
                    let _ = state.render();
                    state.save_synchronously();
                }
                event_loop.exit();
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    if let Some(Runtime::Game(state)) = &mut self.runtime {
                        state.set_paused(true);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => match &mut self.runtime {
                Some(Runtime::Menu(menu)) => menu.handle_mouse_move(position.x, position.y),
                Some(Runtime::Game(state)) => state.handle_mouse_move(position.x, position.y),
                None => {}
            },
            WindowEvent::KeyboardInput { event, .. } => {
                let action = match &mut self.runtime {
                    Some(Runtime::Menu(menu)) => menu.handle_key(
                        event.state,
                        event.physical_key,
                        &event.logical_key,
                        event.repeat,
                    ),
                    Some(Runtime::Game(state)) => {
                        handle_game_keyboard(state, &event);
                        MenuAction::None
                    }
                    None => MenuAction::None,
                };
                self.handle_menu_action(action, event_loop);
            }
            WindowEvent::MouseInput {
                state: element_state,
                button,
                ..
            } => {
                let mut action = MenuAction::None;
                let mut return_to_menu = false;
                match &mut self.runtime {
                    Some(Runtime::Menu(menu)) => {
                        if element_state == ElementState::Pressed && button == MouseButton::Left {
                            action = menu.handle_click();
                        }
                    }
                    Some(Runtime::Game(state)) => {
                        let pressed = element_state == ElementState::Pressed;
                        if state.player_state.is_dead {
                            if pressed && button == MouseButton::Left {
                                state.handle_death_click();
                            }
                        } else if state.is_paused {
                            if pressed && button == MouseButton::Left {
                                return_to_menu = state.handle_menu_click();
                            }
                        } else if state.inventory.is_open {
                            if pressed
                                && (button == MouseButton::Left || button == MouseButton::Right)
                            {
                                state.handle_inventory_click(button == MouseButton::Left);
                            }
                        } else {
                            match button {
                                MouseButton::Left => {
                                    state.left_mouse_pressed = pressed;
                                    if pressed
                                        && state.game_mode == crate::inventory::GameMode::Creative
                                    {
                                        state.handle_click(true);
                                    }
                                }
                                MouseButton::Right if pressed => state.handle_click(false),
                                _ => {}
                            }
                        }
                    }
                    None => {}
                }
                if return_to_menu {
                    self.return_to_main_menu();
                } else {
                    self.handle_menu_action(action, event_loop);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
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
                    winit::event::MouseScrollDelta::PixelDelta(position) => {
                        if position.y > 0.0 {
                            -1
                        } else if position.y < 0.0 {
                            1
                        } else {
                            0
                        }
                    }
                };
                match &mut self.runtime {
                    Some(Runtime::Menu(menu)) => menu.handle_scroll(scroll_dir),
                    Some(Runtime::Game(state)) if !state.is_paused && !state.inventory.is_open => {
                        if scroll_dir != 0 {
                            state.inventory.selected =
                                (state.inventory.selected as i32 + scroll_dir).rem_euclid(9)
                                    as usize;
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::Resized(size) => match &mut self.runtime {
                Some(Runtime::Menu(menu)) => menu.resize(size),
                Some(Runtime::Game(state)) => state.resize(size),
                None => {}
            },
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now
                    .duration_since(self.last_render_time)
                    .as_secs_f32()
                    .min(0.1);
                self.last_render_time = now;
                match &mut self.runtime {
                    Some(Runtime::Menu(menu)) => {
                        menu.update(dt);
                        menu.window.request_redraw();
                        match menu.render() {
                            Ok(()) => {}
                            Err(wgpu::SurfaceError::Lost) => menu.resize(menu.window.inner_size()),
                            Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                            Err(error) => eprintln!("{error:?}"),
                        }
                    }
                    Some(Runtime::Game(state)) => {
                        if !state.window.has_focus() && !state.is_paused {
                            state.set_paused(true);
                        }
                        state.update(dt);
                        state.window.request_redraw();
                        match state.render() {
                            Ok(()) => {}
                            Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                            Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                            Err(error) => eprintln!("{error:?}"),
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.runtime = None;
    }
}

fn handle_game_keyboard(state: &mut State, event: &KeyEvent) {
    let pressed = event.state == ElementState::Pressed;
    if pressed
        && state.active_station == Some(crate::state::StationKind::Anvil)
        && state.inventory.is_open
    {
        match &event.logical_key {
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => {
                state.anvil.rename.pop();
                state.anvil.refresh();
                return;
            }
            winit::keyboard::Key::Character(text) if !event.repeat => {
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

    let PhysicalKey::Code(code) = event.physical_key else {
        return;
    };
    if code == KeyCode::Escape && pressed {
        if state.inventory.is_open {
            state.close_inventory();
        } else {
            state.set_paused(!state.is_paused);
        }
        return;
    }
    if code == state.settings.controls.inventory && pressed {
        if state.inventory.is_open {
            state.close_inventory();
        } else if !state.is_paused {
            state.open_inventory();
        }
        return;
    }
    if code == KeyCode::F3 && pressed && !event.repeat {
        state.show_debug = !state.show_debug;
        return;
    }
    if code == KeyCode::KeyT {
        state.keys.t = pressed;
        return;
    }
    if state.is_paused || state.inventory.is_open {
        return;
    }

    let controls = &state.settings.controls;
    if code == controls.forward {
        state.keys.w = pressed;
    } else if code == controls.backward {
        state.keys.s = pressed;
    } else if code == controls.left {
        state.keys.a = pressed;
    } else if code == controls.right {
        state.keys.d = pressed;
    } else if code == controls.jump {
        state.keys.space = pressed;
    } else if code == controls.sprint {
        state.keys.ctrl = pressed;
    } else if code == controls.sneak {
        state.keys.shift = pressed;
    } else if pressed {
        match code {
            KeyCode::Digit1 => state.inventory.selected = 0,
            KeyCode::Digit2 => state.inventory.selected = 1,
            KeyCode::Digit3 => state.inventory.selected = 2,
            KeyCode::Digit4 => state.inventory.selected = 3,
            KeyCode::Digit5 => state.inventory.selected = 4,
            KeyCode::Digit6 => state.inventory.selected = 5,
            KeyCode::Digit7 => state.inventory.selected = 6,
            KeyCode::Digit8 => state.inventory.selected = 7,
            KeyCode::Digit9 => state.inventory.selected = 8,
            KeyCode::KeyG => {
                state.game_mode = match state.game_mode {
                    crate::inventory::GameMode::Creative => crate::inventory::GameMode::Survival,
                    crate::inventory::GameMode::Survival => crate::inventory::GameMode::Creative,
                };
            }
            _ => {}
        }
    }
}

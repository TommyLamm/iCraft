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

enum PendingRuntimeTransition {
    MenuAction(MenuAction),
    ReturnToMainMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameWheelTarget {
    CreativeCatalog,
    Hotbar,
    None,
}

fn game_wheel_target(
    is_paused: bool,
    inventory_open: bool,
    creative_catalog_open: bool,
) -> GameWheelTarget {
    if !is_paused && creative_catalog_open {
        GameWheelTarget::CreativeCatalog
    } else if !is_paused && !inventory_open {
        GameWheelTarget::Hotbar
    } else {
        GameWheelTarget::None
    }
}

fn camera_angles_after_mouse_motion(
    allowed: bool,
    yaw: f32,
    pitch: f32,
    delta: (f64, f64),
    sensitivity: f32,
) -> Option<(f32, f32)> {
    if !allowed {
        return None;
    }

    let yaw = yaw - (delta.0 * sensitivity as f64) as f32;
    let pitch = pitch - (delta.1 * sensitivity as f64) as f32;
    let max_pitch = f32::to_radians(89.0);
    Some((yaw, pitch.clamp(-max_pitch, max_pitch)))
}

fn inventory_toggle_requested(pressed: bool, repeat: bool) -> bool {
    pressed && !repeat
}

pub struct App {
    runtime: Option<Runtime>,
    window: Option<Arc<Window>>,
    last_render_time: Instant,
    pending_transition: Option<PendingRuntimeTransition>,
    /// Latest keyboard modifier state, tracked so Shift+Q can be detected
    /// even while UI screens swallow plain key events.
    modifiers: winit::event::Modifiers,
}

impl App {
    pub fn new() -> Self {
        Self {
            runtime: None,
            window: None,
            last_render_time: Instant::now(),
            pending_transition: None,
            modifiers: winit::event::Modifiers::default(),
        }
    }

    fn defer_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::None => {}
            action => self.pending_transition = Some(PendingRuntimeTransition::MenuAction(action)),
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

    fn apply_pending_transition(&mut self, event_loop: &ActiveEventLoop) {
        let Some(transition) = self.pending_transition.take() else {
            return;
        };

        // Do not destroy one wgpu surface and create another for the same
        // window from inside a WindowEvent callback.  On Windows that can
        // re-enter the native window procedure while its swapchain is still
        // active, which manifests as STATUS_FATAL_USER_CALLBACK_EXCEPTION.
        match transition {
            PendingRuntimeTransition::MenuAction(action) => {
                self.handle_menu_action(action, event_loop)
            }
            PendingRuntimeTransition::ReturnToMainMenu => self.return_to_main_menu(),
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
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
            if let Some((yaw, pitch)) = camera_angles_after_mouse_motion(
                state.camera_look_allowed(),
                state.camera.yaw,
                state.camera.pitch,
                delta,
                state.sensitivity,
            ) {
                state.camera.yaw = yaw;
                state.camera.pitch = pitch;
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
                    state.shutdown_network();
                    state.save_synchronously();
                }
                event_loop.exit();
            }
            WindowEvent::Focused(focused) => {
                if let Some(Runtime::Game(state)) = &mut self.runtime {
                    if !focused {
                        if state.is_chat_open {
                            state.close_chat();
                        }
                        state.clear_movement_input();
                    }
                    state.sync_cursor_mode();
                }
            }
            WindowEvent::CursorMoved { position, .. } => match &mut self.runtime {
                Some(Runtime::Menu(menu)) => menu.handle_mouse_move(position.x, position.y),
                Some(Runtime::Game(state)) => state.handle_mouse_move(position.x, position.y),
                None => {}
            },
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let mut return_to_menu = false;
                let shift_held = self.modifiers.state().shift_key();
                let action = match &mut self.runtime {
                    Some(Runtime::Menu(menu)) => menu.handle_key(
                        event.state,
                        event.physical_key,
                        &event.logical_key,
                        event.repeat,
                    ),
                    Some(Runtime::Game(state)) => {
                        return_to_menu = handle_game_keyboard(state, &event, shift_held);
                        MenuAction::None
                    }
                    None => MenuAction::None,
                };
                if return_to_menu {
                    self.pending_transition = Some(PendingRuntimeTransition::ReturnToMainMenu);
                } else {
                    self.defer_menu_action(action);
                }
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
                        if !pressed && button == MouseButton::Left {
                            state.left_mouse_pressed = false;
                        }
                        if state.connection_lost {
                            if pressed && button == MouseButton::Left {
                                return_to_menu = state.handle_connection_lost_click();
                            }
                        } else if state.is_chat_open {
                            state.left_mouse_pressed = false;
                        } else if state.player_state.is_dead {
                            if pressed && button == MouseButton::Left {
                                state.handle_death_click();
                            }
                        } else if state.is_paused {
                            if pressed && button == MouseButton::Left {
                                return_to_menu = state.handle_menu_click();
                            }
                        } else if state.advancement_gui.is_open {
                            if button == MouseButton::Left {
                                state.handle_advancements_click(pressed);
                            }
                        } else if state.inventory.is_open {
                            if pressed
                                && (button == MouseButton::Left || button == MouseButton::Right)
                            {
                                state.handle_inventory_click(button == MouseButton::Left);
                            }
                        } else {
                            match button {
                                MouseButton::Left if pressed => {
                                    state.left_mouse_pressed = state.handle_primary_press();
                                }
                                MouseButton::Right if pressed => state.handle_click(false),
                                _ => {}
                            }
                        }
                    }
                    None => {}
                }
                if return_to_menu {
                    self.pending_transition = Some(PendingRuntimeTransition::ReturnToMainMenu);
                } else {
                    self.defer_menu_action(action);
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
                    Some(Runtime::Game(state)) if state.is_chat_open || state.connection_lost => {}
                    Some(Runtime::Game(state)) if state.advancement_gui.is_open => {
                        if scroll_dir != 0 {
                            state.advancement_gui.zoom = (state.advancement_gui.zoom
                                - scroll_dir as f32 * 0.1)
                                .clamp(0.5, 2.0);
                        }
                    }
                    Some(Runtime::Game(state)) => {
                        match game_wheel_target(
                            state.is_paused,
                            state.inventory.is_open,
                            state.is_creative_catalog_open(),
                        ) {
                            GameWheelTarget::CreativeCatalog if scroll_dir != 0 => {
                                state.inventory.scroll_creative(scroll_dir);
                            }
                            GameWheelTarget::Hotbar if scroll_dir != 0 => {
                                state.inventory.selected =
                                    (state.inventory.selected as i32 + scroll_dir).rem_euclid(9)
                                        as usize;
                            }
                            GameWheelTarget::CreativeCatalog
                            | GameWheelTarget::Hotbar
                            | GameWheelTarget::None => {}
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.apply_pending_transition(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.pending_transition = None;
        self.runtime = None;
    }
}

fn handle_game_keyboard(state: &mut State, event: &KeyEvent, shift_held: bool) -> bool {
    let pressed = event.state == ElementState::Pressed;

    if state.connection_lost {
        if pressed && event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
            state.shutdown_network();
            return true;
        }
        return false;
    }

    if state.is_chat_open {
        if pressed {
            match &event.logical_key {
                winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) => {
                    state.close_chat();
                }
                winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter) => {
                    state.submit_chat();
                }
                winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => {
                    state.chat_input.pop();
                }
                winit::keyboard::Key::Character(text) => {
                    for ch in text.chars().filter(|ch| !ch.is_control()) {
                        if state.chat_input.chars().count() >= 256 {
                            break;
                        }
                        state.chat_input.push(ch);
                    }
                }
                _ => {}
            }
        }
        return false;
    }

    if pressed
        && state.active_station == Some(crate::state::StationKind::Anvil)
        && state.inventory.is_open
    {
        match &event.logical_key {
            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => {
                state.anvil.rename.pop();
                state.anvil.refresh();
                return false;
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
                return false;
            }
            _ => {}
        }
    }

    let PhysicalKey::Code(code) = event.physical_key else {
        return false;
    };
    if code == KeyCode::Escape && pressed {
        if state.advancement_gui.is_open {
            state.close_advancements_ui();
        } else if state.inventory.is_open {
            state.close_inventory();
        } else {
            state.set_paused(!state.is_paused);
        }
        return false;
    }
    if code == KeyCode::KeyL && pressed {
        if state.advancement_gui.is_open {
            state.close_advancements_ui();
        } else if !state.is_paused {
            state.open_advancements_ui();
        }
        return false;
    }
    if code == state.settings.controls.inventory
        && inventory_toggle_requested(pressed, event.repeat)
    {
        if state.inventory.is_open {
            state.close_inventory();
        } else if !state.is_paused {
            state.open_inventory();
        }
        return false;
    }
    if code == KeyCode::F3 && pressed && !event.repeat {
        state.show_debug = !state.show_debug;
        return false;
    }
    if code == KeyCode::F5 && pressed && !event.repeat {
        state.third_person = !state.third_person;
        return false;
    }
    // Q throws items onto the ground: the held hotbar stack while playing, or
    // the stack under the mouse cursor while the inventory is open. Holding
    // Shift throws the whole stack instead of a single item. This runs before
    // the gameplay gate below so it stays reachable with the inventory open.
    if code == KeyCode::KeyQ && pressed && !event.repeat {
        if state.is_paused || state.player_state.is_dead {
            return false;
        }
        if state.inventory.is_open {
            state.drop_hovered_item(shift_held);
        } else if !state.advancement_gui.is_open {
            state.drop_held_item(shift_held);
        }
        return false;
    }
    if code == KeyCode::KeyT && pressed && !event.repeat {
        if !state.is_paused
            && !state.inventory.is_open
            && !state.advancement_gui.is_open
            && !state.player_state.is_dead
        {
            state.open_chat();
            state.left_mouse_pressed = false;
        }
        return false;
    }
    if state.is_paused
        || state.inventory.is_open
        || state.advancement_gui.is_open
        || state.player_state.is_dead
    {
        return false;
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
        if pressed {
            state.handle_jump_pressed(Instant::now(), event.repeat);
        }
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
            KeyCode::KeyG if !event.repeat => {
                let game_mode = match state.game_mode {
                    crate::inventory::GameMode::Creative => crate::inventory::GameMode::Survival,
                    crate::inventory::GameMode::Survival => crate::inventory::GameMode::Creative,
                };
                state.set_game_mode(game_mode);
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_target_keeps_creative_catalog_separate_from_hotbar() {
        assert_eq!(
            game_wheel_target(false, true, true),
            GameWheelTarget::CreativeCatalog
        );
        assert_eq!(game_wheel_target(false, true, false), GameWheelTarget::None);
        assert_eq!(
            game_wheel_target(false, false, false),
            GameWheelTarget::Hotbar
        );
        assert_eq!(game_wheel_target(true, false, false), GameWheelTarget::None);
    }

    #[test]
    fn blocked_mouse_motion_does_not_produce_camera_angles() {
        assert_eq!(
            camera_angles_after_mouse_motion(false, 1.0, 0.5, (40.0, -20.0), 0.25),
            None
        );
    }

    #[test]
    fn mouse_motion_applies_sensitivity_and_clamps_pitch() {
        let (yaw, pitch) =
            camera_angles_after_mouse_motion(true, 1.0, 0.5, (4.0, -2.0), 0.25).unwrap();
        assert_eq!(yaw, 0.0);
        assert_eq!(pitch, 1.0);

        let max_pitch = f32::to_radians(89.0);
        let (_, upper) =
            camera_angles_after_mouse_motion(true, 0.0, 0.0, (0.0, -1000.0), 1.0).unwrap();
        let (_, lower) =
            camera_angles_after_mouse_motion(true, 0.0, 0.0, (0.0, 1000.0), 1.0).unwrap();
        assert_eq!(upper, max_pitch);
        assert_eq!(lower, -max_pitch);
    }

    #[test]
    fn inventory_toggle_ignores_repeated_and_release_events() {
        assert!(inventory_toggle_requested(true, false));
        assert!(!inventory_toggle_requested(true, true));
        assert!(!inventory_toggle_requested(false, false));
        assert!(!inventory_toggle_requested(false, true));
    }
}

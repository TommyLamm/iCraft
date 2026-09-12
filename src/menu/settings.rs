//! Game settings and control bindings persistence.

use crate::accessibility::AccessibilitySettings;
use crate::game_rules::Difficulty;
pub use crate::localization::Language;
use std::fs;
use winit::keyboard::KeyCode;

const SETTINGS_FILE: &str = "settings.txt";
const CONTROLS_FILE: &str = "controls.config";

fn clamp_setting_volume(value: f32, fallback: f32) -> f32 {
    finite_clamped_setting(value, fallback, 0.0, 1.0)
}

fn finite_clamped_setting(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

#[derive(Debug, Clone)]
pub struct ControlBindings {
    pub forward: KeyCode,
    pub backward: KeyCode,
    pub left: KeyCode,
    pub right: KeyCode,
    pub jump: KeyCode,
    pub sprint: KeyCode,
    pub sneak: KeyCode,
    pub inventory: KeyCode,
    pub chat: KeyCode,
    pub time_speed: KeyCode,
    pub advancements: KeyCode,
    pub debug: KeyCode,
    pub perspective: KeyCode,
    pub gamemode: KeyCode,
    pub pause: KeyCode,
    pub hotbar_1: KeyCode,
    pub hotbar_2: KeyCode,
    pub hotbar_3: KeyCode,
    pub hotbar_4: KeyCode,
    pub hotbar_5: KeyCode,
    pub hotbar_6: KeyCode,
    pub hotbar_7: KeyCode,
    pub hotbar_8: KeyCode,
    pub hotbar_9: KeyCode,
}

impl Default for ControlBindings {
    fn default() -> Self {
        Self {
            forward: KeyCode::KeyW,
            backward: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
            jump: KeyCode::Space,
            sprint: KeyCode::ControlLeft,
            sneak: KeyCode::ShiftLeft,
            inventory: KeyCode::KeyE,
            chat: KeyCode::KeyT,
            time_speed: KeyCode::KeyF,
            advancements: KeyCode::KeyL,
            debug: KeyCode::F3,
            perspective: KeyCode::F5,
            gamemode: KeyCode::KeyG,
            pause: KeyCode::Escape,
            hotbar_1: KeyCode::Digit1,
            hotbar_2: KeyCode::Digit2,
            hotbar_3: KeyCode::Digit3,
            hotbar_4: KeyCode::Digit4,
            hotbar_5: KeyCode::Digit5,
            hotbar_6: KeyCode::Digit6,
            hotbar_7: KeyCode::Digit7,
            hotbar_8: KeyCode::Digit8,
            hotbar_9: KeyCode::Digit9,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameSettings {
    pub fov: f32,
    pub sensitivity: f32,
    pub render_distance: i32,
    pub fullscreen: bool,
    pub vsync: bool,
    /// Maximum redraw rate; zero means uncapped.
    pub fps_cap: u32,
    pub master_volume: f32,
    pub music_volume: f32,
    pub sound_volume: f32,
    pub weather_volume: f32,
    pub difficulty: Difficulty,
    pub language: Language,
    pub controls: ControlBindings,
    pub mp_host_port: String,
    pub mp_server_address: String,
    pub mp_join_port: String,
    pub mp_username: String,
    pub entity_distance_scale: f32,
    pub accessibility: AccessibilitySettings,
    pub resource_packs: Vec<String>,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            fov: 70.0,
            sensitivity: 0.002,
            render_distance: 8,
            fullscreen: false,
            vsync: true,
            fps_cap: 0,
            master_volume: 1.0,
            music_volume: 0.7,
            sound_volume: 1.0,
            weather_volume: 0.4,
            difficulty: Difficulty::Normal,
            language: Language::English,
            controls: ControlBindings::default(),
            mp_host_port: "25565".to_string(),
            mp_server_address: "127.0.0.1".to_string(),
            mp_join_port: "25565".to_string(),
            mp_username: "PLAYER".to_string(),
            entity_distance_scale: 1.0,
            accessibility: AccessibilitySettings::default(),
            resource_packs: Vec::new(),
        }
    }
}

impl GameSettings {
    pub fn load() -> Self {
        let mut settings = Self::default();
        if let Ok(contents) = fs::read_to_string(SETTINGS_FILE) {
            settings.apply_file_contents(&contents);
        }
        if let Ok(contents) = fs::read_to_string(CONTROLS_FILE) {
            settings.apply_file_contents(&contents);
        }
        settings.sanitize_view_settings();
        settings.render_distance = settings.render_distance.clamp(2, 16);
        settings.clamp_audio_volumes();
        settings.accessibility.sanitize();
        settings
    }

    #[allow(dead_code)]
    pub fn from_file_contents(contents: &str) -> Self {
        let mut settings = Self::default();
        settings.apply_file_contents(contents);
        settings.sanitize_view_settings();
        settings.render_distance = settings.render_distance.clamp(2, 16);
        settings.clamp_audio_volumes();
        settings.accessibility.sanitize();
        settings
    }

    pub(super) fn apply_file_contents(&mut self, contents: &str) {
        for line in contents.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with("//") || line.is_empty() {
                continue;
            }
            let delimiter = if line.contains('=') { '=' } else { ':' };
            let Some((key, value)) = line.split_once(delimiter) else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "fov" => self.fov = value.parse().unwrap_or(self.fov),
                "sensitivity" => self.sensitivity = value.parse().unwrap_or(self.sensitivity),
                "render_distance" => {
                    self.render_distance = value.parse().unwrap_or(self.render_distance)
                }
                "fullscreen" => self.fullscreen = parse_bool(value, self.fullscreen),
                "vsync" => self.vsync = parse_bool(value, self.vsync),
                "fps_cap" => self.fps_cap = value.parse::<u32>().unwrap_or(self.fps_cap),
                "volume" | "master_volume" => {
                    self.master_volume = value.parse().unwrap_or(self.master_volume)
                }
                "music_volume" => self.music_volume = value.parse().unwrap_or(self.music_volume),
                "sound_volume" => self.sound_volume = value.parse().unwrap_or(self.sound_volume),
                "weather_volume" => {
                    self.weather_volume = value.parse().unwrap_or(self.weather_volume)
                }
                "difficulty" => self.difficulty = Difficulty::parse(value),
                "language" => self.language = Language::parse(value),
                "key_forward" => set_key(&mut self.controls.forward, value),
                "key_backward" => set_key(&mut self.controls.backward, value),
                "key_left" => set_key(&mut self.controls.left, value),
                "key_right" => set_key(&mut self.controls.right, value),
                "key_jump" => set_key(&mut self.controls.jump, value),
                "key_sprint" => set_key(&mut self.controls.sprint, value),
                "key_sneak" => set_key(&mut self.controls.sneak, value),
                "key_inventory" => set_key(&mut self.controls.inventory, value),
                "key_chat" => set_key(&mut self.controls.chat, value),
                "key_time_speed" => set_key(&mut self.controls.time_speed, value),
                "key_advancements" => set_key(&mut self.controls.advancements, value),
                "key_debug" => set_key(&mut self.controls.debug, value),
                "key_perspective" => set_key(&mut self.controls.perspective, value),
                "key_gamemode" => set_key(&mut self.controls.gamemode, value),
                "key_pause" => set_key(&mut self.controls.pause, value),
                "key_hotbar_1" => set_key(&mut self.controls.hotbar_1, value),
                "key_hotbar_2" => set_key(&mut self.controls.hotbar_2, value),
                "key_hotbar_3" => set_key(&mut self.controls.hotbar_3, value),
                "key_hotbar_4" => set_key(&mut self.controls.hotbar_4, value),
                "key_hotbar_5" => set_key(&mut self.controls.hotbar_5, value),
                "key_hotbar_6" => set_key(&mut self.controls.hotbar_6, value),
                "key_hotbar_7" => set_key(&mut self.controls.hotbar_7, value),
                "key_hotbar_8" => set_key(&mut self.controls.hotbar_8, value),
                "key_hotbar_9" => set_key(&mut self.controls.hotbar_9, value),
                "mp_host_port" => self.mp_host_port = value.to_string(),
                "mp_server_address" => self.mp_server_address = value.to_string(),
                "mp_join_port" => self.mp_join_port = value.to_string(),
                "mp_username" => self.mp_username = value.to_string(),
                "entity_distance_scale" => {
                    self.entity_distance_scale = value
                        .parse::<f32>()
                        .unwrap_or(self.entity_distance_scale)
                        .clamp(0.5, 2.0)
                }
                "ui_scale" => {
                    self.accessibility.ui_scale =
                        value.parse().unwrap_or(self.accessibility.ui_scale)
                }
                "chat_scale" => {
                    self.accessibility.chat_scale =
                        value.parse().unwrap_or(self.accessibility.chat_scale)
                }
                "chat_opacity" => {
                    self.accessibility.chat_opacity =
                        value.parse().unwrap_or(self.accessibility.chat_opacity)
                }
                "subtitles" => {
                    self.accessibility.subtitles = parse_bool(value, self.accessibility.subtitles)
                }
                "high_contrast" => {
                    self.accessibility.high_contrast =
                        parse_bool(value, self.accessibility.high_contrast)
                }
                "reduce_flashing" => {
                    self.accessibility.reduce_flashing =
                        parse_bool(value, self.accessibility.reduce_flashing)
                }
                "toggle_sprint" => {
                    self.accessibility.toggle_sprint =
                        parse_bool(value, self.accessibility.toggle_sprint)
                }
                "toggle_sneak" => {
                    self.accessibility.toggle_sneak =
                        parse_bool(value, self.accessibility.toggle_sneak)
                }
                "camera_bobbing" => {
                    self.accessibility.camera_bobbing =
                        parse_bool(value, self.accessibility.camera_bobbing)
                }
                "damage_tilt" => {
                    self.accessibility.damage_tilt =
                        parse_bool(value, self.accessibility.damage_tilt)
                }
                "resource_packs" => {
                    self.resource_packs = value
                        .split(',')
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .take(32)
                        .map(str::to_string)
                        .collect()
                }
                _ => {}
            }
        }
    }

    pub fn save(&self) {
        if let Err(error) =
            crate::save::atomic_write(SETTINGS_FILE, self.to_file_contents().as_bytes())
        {
            eprintln!("[Settings] Could not save settings: {error}");
        }
        if let Err(error) =
            crate::save::atomic_write(CONTROLS_FILE, self.to_controls_file_contents().as_bytes())
        {
            eprintln!("[Settings] Could not save controls config: {error}");
        }
    }

    pub(super) fn to_controls_file_contents(&self) -> String {
        format!(
            concat!(
                "# =====================================================================\n",
                "# iCraft 玩家按鍵設定檔 (Keybindings Configuration)\n",
                "# =====================================================================\n",
                "# 本檔案供玩家自由修改遊戲內的所有按鍵綁定。\n",
                "# 修改存檔後，啟動遊戲將自動載入最新按鍵設定。\n",
                "#\n",
                "# 【支援的按鍵名稱 (Supported Key Names)】:\n",
                "#   - 字母鍵: A, B, C, ..., Z\n",
                "#   - 數字鍵: 0, 1, 2, ..., 9\n",
                "#   - 方向鍵: UP, DOWN, LEFT, RIGHT\n",
                "#   - 修飾鍵: SPACE, LCTRL, RCTRL, LSHIFT, RSHIFT\n",
                "#   - 控制鍵: ESC, ENTER, TAB, BACKSPACE, F1 ~ F12\n",
                "# =====================================================================\n\n",
                "# ---------------------------------------------------------------------\n",
                "# 1. 角色移動與基本操作 (Movement & Basic Actions)\n",
                "# ---------------------------------------------------------------------\n\n",
                "# 前進 (Move Forward)\n",
                "key_forward = {}\n\n",
                "# 後退 (Move Backward)\n",
                "key_backward = {}\n\n",
                "# 向左平移 (Move Left)\n",
                "key_left = {}\n\n",
                "# 向右平移 (Move Right)\n",
                "key_right = {}\n\n",
                "# 跳躍 / 創造模式向上飛行 (Jump / Ascend)\n",
                "key_jump = {}\n\n",
                "# 疾跑 (Sprint)\n",
                "key_sprint = {}\n\n",
                "# 潛行 / 創造模式向下滑行 (Sneak / Descend)\n",
                "key_sneak = {}\n\n",
                "# 開啟 / 關閉背包 (Toggle Inventory)\n",
                "key_inventory = {}\n\n",
                "# ---------------------------------------------------------------------\n",
                "# 2. 系統功能與模式切換快捷鍵 (System & Gameplay Hotkeys)\n",
                "# ---------------------------------------------------------------------\n\n",
                "# 開啟 / 關閉聊天框 (Open Chat)\n",
                "key_chat = {}\n\n",
                "# 時間加速 (Accelerate Time)\n",
                "key_time_speed = {}\n\n",
                "# 開啟 / 關閉成就樹 (Advancements Screen)\n",
                "key_advancements = {}\n\n",
                "# 開啟 / 關閉 F3 偵錯 Overlay (Debug Info)\n",
                "key_debug = {}\n\n",
                "# 切換第一人稱 / 第三人稱視角 (Toggle Camera View)\n",
                "key_perspective = {}\n\n",
                "# 切換生存 / 創造遊戲模式 (Toggle Game Mode)\n",
                "key_gamemode = {}\n\n",
                "# 暫停選單 / 關閉界面 (Pause Menu / Close UI)\n",
                "key_pause = {}\n\n",
                "# ---------------------------------------------------------------------\n",
                "# 3. 快捷欄物品選擇 1 - 9 (Hotbar Item Selection 1-9)\n",
                "# ---------------------------------------------------------------------\n\n",
                "key_hotbar_1 = {}\n",
                "key_hotbar_2 = {}\n",
                "key_hotbar_3 = {}\n",
                "key_hotbar_4 = {}\n",
                "key_hotbar_5 = {}\n",
                "key_hotbar_6 = {}\n",
                "key_hotbar_7 = {}\n",
                "key_hotbar_8 = {}\n",
                "key_hotbar_9 = {}\n"
            ),
            key_name(self.controls.forward),
            key_name(self.controls.backward),
            key_name(self.controls.left),
            key_name(self.controls.right),
            key_name(self.controls.jump),
            key_name(self.controls.sprint),
            key_name(self.controls.sneak),
            key_name(self.controls.inventory),
            key_name(self.controls.chat),
            key_name(self.controls.time_speed),
            key_name(self.controls.advancements),
            key_name(self.controls.debug),
            key_name(self.controls.perspective),
            key_name(self.controls.gamemode),
            key_name(self.controls.pause),
            key_name(self.controls.hotbar_1),
            key_name(self.controls.hotbar_2),
            key_name(self.controls.hotbar_3),
            key_name(self.controls.hotbar_4),
            key_name(self.controls.hotbar_5),
            key_name(self.controls.hotbar_6),
            key_name(self.controls.hotbar_7),
            key_name(self.controls.hotbar_8),
            key_name(self.controls.hotbar_9),
        )
    }

    pub(super) fn to_file_contents(&self) -> String {
        let mut settings = self.clone();
        settings.sanitize_view_settings();
        settings.clamp_audio_volumes();
        settings.accessibility.sanitize();
        format!(
            concat!(
                "fov:{}\n",
                "sensitivity:{}\n",
                "render_distance:{}\n",
                "fullscreen:{}\n",
                "vsync:{}\n",
                "fps_cap:{}\n",
                "master_volume:{}\n",
                "music_volume:{}\n",
                "sound_volume:{}\n",
                "weather_volume:{}\n",
                "difficulty:{}\n",
                "language:{}\n",
                "key_forward:{}\n",
                "key_backward:{}\n",
                "key_left:{}\n",
                "key_right:{}\n",
                "key_jump:{}\n",
                "key_sprint:{}\n",
                "key_sneak:{}\n",
                "key_inventory:{}\n",
                "key_chat:{}\n",
                "key_advancements:{}\n",
                "key_debug:{}\n",
                "key_perspective:{}\n",
                "key_gamemode:{}\n",
                "key_pause:{}\n",
                "mp_host_port:{}\n",
                "mp_server_address:{}\n",
                "mp_join_port:{}\n",
                "mp_username:{}\n",
                "entity_distance_scale:{}\n",
                "ui_scale:{}\n",
                "chat_scale:{}\n",
                "chat_opacity:{}\n",
                "subtitles:{}\n",
                "high_contrast:{}\n",
                "reduce_flashing:{}\n",
                "toggle_sprint:{}\n",
                "toggle_sneak:{}\n",
                "camera_bobbing:{}\n",
                "damage_tilt:{}\n",
                "resource_packs:{}\n"
            ),
            settings.fov,
            settings.sensitivity,
            settings.render_distance,
            settings.fullscreen,
            settings.vsync,
            settings.fps_cap,
            settings.master_volume,
            settings.music_volume,
            settings.sound_volume,
            settings.weather_volume,
            settings.difficulty.as_str(),
            settings.language.as_str(),
            key_name(settings.controls.forward),
            key_name(settings.controls.backward),
            key_name(settings.controls.left),
            key_name(settings.controls.right),
            key_name(settings.controls.jump),
            key_name(settings.controls.sprint),
            key_name(settings.controls.sneak),
            key_name(settings.controls.inventory),
            key_name(settings.controls.chat),
            key_name(settings.controls.advancements),
            key_name(settings.controls.debug),
            key_name(settings.controls.perspective),
            key_name(settings.controls.gamemode),
            key_name(settings.controls.pause),
            settings.mp_host_port,
            settings.mp_server_address,
            settings.mp_join_port,
            settings.mp_username,
            settings.entity_distance_scale,
            settings.accessibility.ui_scale,
            settings.accessibility.chat_scale,
            settings.accessibility.chat_opacity,
            settings.accessibility.subtitles,
            settings.accessibility.high_contrast,
            settings.accessibility.reduce_flashing,
            settings.accessibility.toggle_sprint,
            settings.accessibility.toggle_sneak,
            settings.accessibility.camera_bobbing,
            settings.accessibility.damage_tilt,
            settings.resource_packs.join(","),
        )
    }

    pub fn clamp_audio_volumes(&mut self) {
        self.master_volume = clamp_setting_volume(self.master_volume, 1.0);
        self.music_volume = clamp_setting_volume(self.music_volume, 0.7);
        self.sound_volume = clamp_setting_volume(self.sound_volume, 1.0);
        self.weather_volume = clamp_setting_volume(self.weather_volume, 0.4);
    }

    fn sanitize_view_settings(&mut self) {
        self.fov = finite_clamped_setting(self.fov, 70.0, 30.0, 120.0);
        self.sensitivity = finite_clamped_setting(self.sensitivity, 0.002, 0.0002, 0.006);
        self.fps_cap = self.fps_cap.min(240);
    }

    pub fn effective_sound_volume(&self) -> f32 {
        clamp_setting_volume(self.master_volume, 1.0) * clamp_setting_volume(self.sound_volume, 1.0)
    }
}

pub(super) fn parse_bool(value: &str, fallback: bool) -> bool {
    crate::game_rules::parse_bool_or(value, fallback)
}

const FPS_CAPS: [u32; 4] = [0, 30, 60, 144];

pub(super) fn cycle_fps_cap(current: u32, delta: i32) -> u32 {
    let index = FPS_CAPS.iter().position(|&cap| cap == current).unwrap_or(0) as i32;
    FPS_CAPS[(index + delta).rem_euclid(FPS_CAPS.len() as i32) as usize]
}

pub(super) fn fps_cap_label(cap: u32) -> String {
    if cap == 0 {
        "UNCAPPED".to_string()
    } else {
        format!("{cap}")
    }
}

fn set_key(target: &mut KeyCode, value: &str) {
    if let Some(code) = parse_key(value) {
        *target = code;
    }
}

pub(super) fn key_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::KeyA => "A",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyD => "D",
        KeyCode::KeyE => "E",
        KeyCode::KeyF => "F",
        KeyCode::KeyG => "G",
        KeyCode::KeyH => "H",
        KeyCode::KeyI => "I",
        KeyCode::KeyJ => "J",
        KeyCode::KeyK => "K",
        KeyCode::KeyL => "L",
        KeyCode::KeyM => "M",
        KeyCode::KeyN => "N",
        KeyCode::KeyO => "O",
        KeyCode::KeyP => "P",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyR => "R",
        KeyCode::KeyS => "S",
        KeyCode::KeyT => "T",
        KeyCode::KeyU => "U",
        KeyCode::KeyV => "V",
        KeyCode::KeyW => "W",
        KeyCode::KeyX => "X",
        KeyCode::KeyY => "Y",
        KeyCode::KeyZ => "Z",
        KeyCode::Digit0 => "0",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Space => "SPACE",
        KeyCode::ControlLeft => "LCTRL",
        KeyCode::ControlRight => "RCTRL",
        KeyCode::ShiftLeft => "LSHIFT",
        KeyCode::ShiftRight => "RSHIFT",
        KeyCode::ArrowUp => "UP",
        KeyCode::ArrowDown => "DOWN",
        KeyCode::ArrowLeft => "LEFT",
        KeyCode::ArrowRight => "RIGHT",
        KeyCode::Escape => "ESC",
        KeyCode::Enter => "ENTER",
        KeyCode::Tab => "TAB",
        KeyCode::Backspace => "BACKSPACE",
        KeyCode::F1 => "F1",
        KeyCode::F2 => "F2",
        KeyCode::F3 => "F3",
        KeyCode::F4 => "F4",
        KeyCode::F5 => "F5",
        KeyCode::F6 => "F6",
        KeyCode::F7 => "F7",
        KeyCode::F8 => "F8",
        KeyCode::F9 => "F9",
        KeyCode::F10 => "F10",
        KeyCode::F11 => "F11",
        KeyCode::F12 => "F12",
        _ => "KEY",
    }
}

pub(super) fn parse_key(value: &str) -> Option<KeyCode> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() == 1 {
        let ch = value.as_bytes()[0];
        if ch.is_ascii_alphabetic() {
            return Some(match ch {
                b'A' => KeyCode::KeyA,
                b'B' => KeyCode::KeyB,
                b'C' => KeyCode::KeyC,
                b'D' => KeyCode::KeyD,
                b'E' => KeyCode::KeyE,
                b'F' => KeyCode::KeyF,
                b'G' => KeyCode::KeyG,
                b'H' => KeyCode::KeyH,
                b'I' => KeyCode::KeyI,
                b'J' => KeyCode::KeyJ,
                b'K' => KeyCode::KeyK,
                b'L' => KeyCode::KeyL,
                b'M' => KeyCode::KeyM,
                b'N' => KeyCode::KeyN,
                b'O' => KeyCode::KeyO,
                b'P' => KeyCode::KeyP,
                b'Q' => KeyCode::KeyQ,
                b'R' => KeyCode::KeyR,
                b'S' => KeyCode::KeyS,
                b'T' => KeyCode::KeyT,
                b'U' => KeyCode::KeyU,
                b'V' => KeyCode::KeyV,
                b'W' => KeyCode::KeyW,
                b'X' => KeyCode::KeyX,
                b'Y' => KeyCode::KeyY,
                b'Z' => KeyCode::KeyZ,
                _ => return None,
            });
        }
        if ch.is_ascii_digit() {
            return Some(match ch {
                b'0' => KeyCode::Digit0,
                b'1' => KeyCode::Digit1,
                b'2' => KeyCode::Digit2,
                b'3' => KeyCode::Digit3,
                b'4' => KeyCode::Digit4,
                b'5' => KeyCode::Digit5,
                b'6' => KeyCode::Digit6,
                b'7' => KeyCode::Digit7,
                b'8' => KeyCode::Digit8,
                b'9' => KeyCode::Digit9,
                _ => return None,
            });
        }
    }
    match value.as_str() {
        "SPACE" => Some(KeyCode::Space),
        "LCTRL" => Some(KeyCode::ControlLeft),
        "RCTRL" => Some(KeyCode::ControlRight),
        "LSHIFT" => Some(KeyCode::ShiftLeft),
        "RSHIFT" => Some(KeyCode::ShiftRight),
        "UP" => Some(KeyCode::ArrowUp),
        "DOWN" => Some(KeyCode::ArrowDown),
        "LEFT" => Some(KeyCode::ArrowLeft),
        "RIGHT" => Some(KeyCode::ArrowRight),
        "ESC" | "ESCAPE" => Some(KeyCode::Escape),
        "ENTER" | "RETURN" => Some(KeyCode::Enter),
        "TAB" => Some(KeyCode::Tab),
        "BACKSPACE" => Some(KeyCode::Backspace),
        "F1" => Some(KeyCode::F1),
        "F2" => Some(KeyCode::F2),
        "F3" => Some(KeyCode::F3),
        "F4" => Some(KeyCode::F4),
        "F5" => Some(KeyCode::F5),
        "F6" => Some(KeyCode::F6),
        "F7" => Some(KeyCode::F7),
        "F8" => Some(KeyCode::F8),
        "F9" => Some(KeyCode::F9),
        "F10" => Some(KeyCode::F10),
        "F11" => Some(KeyCode::F11),
        "F12" => Some(KeyCode::F12),
        _ => None,
    }
}

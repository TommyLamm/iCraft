use crate::game_rules::{WorldCreationOptions, WorldType};
use crate::inventory::GameMode;
use crate::{
    accessibility::AccessibilitySettings, localization::TranslationCatalog,
    resources::ResourcePackManager,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wgpu::util::DeviceExt;
use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Fullscreen, Window};

const UI_VERTEX_CAPACITY: usize = 65_536;
const SETTINGS_FILE: &str = "settings.txt";
const CONTROLS_FILE: &str = "controls.config";
const SAVES_DIR: &str = "saves";
const META_FILE: &str = "world.meta";
const CURRENT_WORLD_FORMAT_VERSION: u32 = 3;
const OPTIONS_ROW_TOPS: [f32; 6] = [0.58, 0.38, 0.18, -0.02, -0.22, -0.42];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Peaceful => "PEACEFUL",
            Self::Easy => "EASY",
            Self::Normal => "NORMAL",
            Self::Hard => "HARD",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "peaceful" => Self::Peaceful,
            "easy" => Self::Easy,
            "hard" => Self::Hard,
            _ => Self::Normal,
        }
    }

    fn step(self, delta: i32) -> Self {
        let values = [Self::Peaceful, Self::Easy, Self::Normal, Self::Hard];
        let index = values.iter().position(|value| *value == self).unwrap_or(2) as i32;
        values[(index + delta).rem_euclid(values.len() as i32) as usize]
    }
}

pub use crate::localization::Language;

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
    pub render_scale: f32,
    pub dynamic_resolution: bool,
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
            render_scale: 1.0,
            dynamic_resolution: false,
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

    fn apply_file_contents(&mut self, contents: &str) {
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
                "render_scale" => {
                    self.render_scale = value
                        .parse::<f32>()
                        .unwrap_or(self.render_scale)
                        .clamp(0.5, 1.0)
                }
                "dynamic_resolution" => {
                    self.dynamic_resolution = parse_bool(value, self.dynamic_resolution)
                }
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
        if let Err(error) = fs::write(SETTINGS_FILE, self.to_file_contents()) {
            eprintln!("[Settings] Could not save settings: {error}");
        }
        if let Err(error) = fs::write(CONTROLS_FILE, self.to_controls_file_contents()) {
            eprintln!("[Settings] Could not save controls config: {error}");
        }
    }

    fn to_controls_file_contents(&self) -> String {
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

    fn to_file_contents(&self) -> String {
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
                "render_scale:{}\n",
                "dynamic_resolution:{}\n",
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
            settings.render_scale,
            settings.dynamic_resolution,
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

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" => true,
        "false" | "0" | "off" => false,
        _ => fallback,
    }
}

const FPS_CAPS: [u32; 4] = [0, 30, 60, 144];

fn cycle_fps_cap(current: u32, delta: i32) -> u32 {
    let index = FPS_CAPS.iter().position(|&cap| cap == current).unwrap_or(0) as i32;
    FPS_CAPS[(index + delta).rem_euclid(FPS_CAPS.len() as i32) as usize]
}

fn fps_cap_label(cap: u32) -> String {
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

fn key_name(code: KeyCode) -> &'static str {
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

fn parse_key(value: &str) -> Option<KeyCode> {
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

#[derive(Debug, Clone)]
pub enum MultiplayerRole {
    Singleplayer,
    Host {
        port: u16,
    },
    Client {
        server_addr: String,
        port: u16,
        username: String,
    },
}

/// Addresses shown by the multiplayer screen.  Keeping the address book
/// independent from the winit menu state lets headless clients persist a
/// useful recent-server list and record the result of a server-list ping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddressBook {
    addresses: Vec<String>,
    recent_results: Vec<ServerPingResult>,
    capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPingResult {
    pub address: String,
    pub version: String,
    pub motd: String,
    pub online_players: u16,
    pub max_players: u16,
    pub error: Option<String>,
}

impl ServerAddressBook {
    pub fn new(capacity: usize) -> Self {
        Self {
            addresses: Vec::new(),
            recent_results: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn addresses(&self) -> &[String] {
        &self.addresses
    }

    pub fn recent_results(&self) -> &[ServerPingResult] {
        &self.recent_results
    }

    pub fn remember(&mut self, address: impl Into<String>) {
        let address = address.into();
        if address.trim().is_empty() {
            return;
        }
        self.addresses.retain(|existing| existing != &address);
        self.addresses.insert(0, address);
        self.addresses.truncate(self.capacity);
    }

    pub fn record_ping(&mut self, result: ServerPingResult) {
        self.remember(result.address.clone());
        self.recent_results
            .retain(|existing| existing.address != result.address);
        self.recent_results.insert(0, result);
        self.recent_results.truncate(self.capacity);
    }
}

#[derive(Debug, Clone)]
pub struct WorldLaunch {
    pub world_dir: PathBuf,
    pub seed: u32,
    pub game_mode: GameMode,
    pub difficulty: Difficulty,
    pub role: MultiplayerRole,
}

#[derive(Debug, Clone)]
struct WorldMetadata {
    name: String,
    seed: u32,
    game_mode: GameMode,
    difficulty: Difficulty,
    last_played: u64,
    world_type: WorldType,
    generate_structures: bool,
    bonus_chest: bool,
    cheats_enabled: bool,
    hardcore: bool,
    version: u32,
    needs_upgrade: bool,
}

impl WorldMetadata {
    fn load(world_dir: &Path) -> Option<Self> {
        let contents = fs::read_to_string(world_dir.join(META_FILE)).ok()?;
        let mut name = None;
        let mut seed = 12345;
        let mut game_mode = GameMode::Survival;
        let mut difficulty = Difficulty::Normal;
        let mut last_played = 0;
        let mut world_type = WorldType::Default;
        let mut generate_structures = true;
        let mut bonus_chest = false;
        let mut cheats_enabled = false;
        let mut hardcore = false;
        let mut version = 0;
        let mut needs_upgrade = false;
        for line in contents.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key.trim() {
                "name" => name = Some(value.trim().to_string()),
                "seed" => seed = value.trim().parse().unwrap_or(seed),
                "game_mode" => game_mode = parse_game_mode(value),
                "difficulty" => difficulty = Difficulty::parse(value),
                "last_played" => last_played = value.trim().parse().unwrap_or(0),
                "world_type" => world_type = WorldType::parse(value),
                "generate_structures" => {
                    generate_structures = parse_bool(value, generate_structures)
                }
                "bonus_chest" => bonus_chest = parse_bool(value, bonus_chest),
                "cheats" | "cheats_enabled" => cheats_enabled = parse_bool(value, cheats_enabled),
                "hardcore" => hardcore = parse_bool(value, hardcore),
                "version" | "format_version" => version = value.trim().parse().unwrap_or(version),
                "needs_upgrade" => needs_upgrade = parse_bool(value, needs_upgrade),
                _ => {}
            }
        }
        needs_upgrade |= version < CURRENT_WORLD_FORMAT_VERSION;
        Some(Self {
            name: name?,
            seed,
            game_mode,
            difficulty,
            last_played,
            world_type,
            generate_structures,
            bonus_chest,
            cheats_enabled,
            hardcore,
            version,
            needs_upgrade,
        })
    }

    fn save(&self, world_dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(world_dir.join("regions"))?;
        crate::save::atomic_write(
            world_dir.join(META_FILE),
            format!(
                "name:{}\nseed:{}\ngame_mode:{}\ndifficulty:{}\nlast_played:{}\nworld_type:{}\ngenerate_structures:{}\nbonus_chest:{}\ncheats_enabled:{}\nhardcore:{}\nversion:{}\nneeds_upgrade:{}\n",
                self.name,
                self.seed,
                game_mode_name(self.game_mode),
                self.difficulty.as_str(),
                self.last_played,
                self.world_type.as_str(),
                self.generate_structures,
                self.bonus_chest,
                self.cheats_enabled,
                self.hardcore,
                self.version,
                self.needs_upgrade,
            )
            .as_bytes(),
        )
    }
}

/// Load creation-only options without exposing the menu's text metadata type.
/// Legacy worlds use the documented defaults until their next authoritative
/// level save writes the richer binary fields.
pub fn load_world_creation_options(world_dir: &Path) -> WorldCreationOptions {
    WorldMetadata::load(world_dir)
        .map(|metadata| WorldCreationOptions {
            world_type: metadata.world_type,
            generate_structures: metadata.generate_structures,
            bonus_chest: metadata.bonus_chest,
            cheats_enabled: metadata.cheats_enabled,
            hardcore: metadata.hardcore,
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
struct WorldEntry {
    directory: PathBuf,
    metadata: WorldMetadata,
}

fn game_mode_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "SURVIVAL",
        GameMode::Creative => "CREATIVE",
        GameMode::Adventure => "ADVENTURE",
        GameMode::Spectator => "SPECTATOR",
    }
}

fn parse_game_mode(value: &str) -> GameMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "creative" => GameMode::Creative,
        "adventure" => GameMode::Adventure,
        "spectator" => GameMode::Spectator,
        _ => GameMode::Survival,
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn discover_worlds() -> Vec<WorldEntry> {
    let mut worlds = Vec::new();
    let Ok(entries) = fs::read_dir(SAVES_DIR) else {
        return worlds;
    };
    for entry in entries.flatten() {
        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }
        let metadata = WorldMetadata::load(&directory).or_else(|| legacy_metadata(&directory));
        if let Some(metadata) = metadata {
            worlds.push(WorldEntry {
                directory: fs::canonicalize(&directory).unwrap_or(directory),
                metadata,
            });
        }
    }
    worlds.sort_by_key(|world| std::cmp::Reverse(world.metadata.last_played));
    worlds
}

fn world_index_by_directory(worlds: &[WorldEntry], directory: &Path) -> Option<usize> {
    worlds.iter().position(|world| world.directory == directory)
}

pub fn update_world_metadata(
    world_dir: &Path,
    seed: u32,
    game_mode: GameMode,
    difficulty: Difficulty,
) -> std::io::Result<()> {
    let mut metadata = WorldMetadata::load(world_dir)
        .or_else(|| legacy_metadata(world_dir))
        .unwrap_or_else(|| WorldMetadata {
            name: world_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("WORLD")
                .replace('_', " ")
                .to_ascii_uppercase(),
            seed,
            game_mode,
            difficulty,
            last_played: 0,
            world_type: WorldType::Default,
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: false,
            hardcore: false,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        });
    if metadata.needs_upgrade {
        let base = world_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(slugify)
            .unwrap_or_else(|| "world".to_string());
        let mut backup_path = Path::new(SAVES_DIR).join(format!("{base}_backup_{}", unix_now()));
        let mut suffix = 2;
        while backup_path.exists() {
            backup_path =
                Path::new(SAVES_DIR).join(format!("{base}_backup_{}_{}", unix_now(), suffix));
            suffix += 1;
        }
        backup_world(world_dir, &backup_path)?;
    }
    metadata.seed = seed;
    metadata.game_mode = game_mode;
    metadata.difficulty = difficulty;
    metadata.last_played = unix_now();
    metadata.version = CURRENT_WORLD_FORMAT_VERSION;
    metadata.needs_upgrade = false;
    metadata.save(world_dir)
}

fn legacy_metadata(directory: &Path) -> Option<WorldMetadata> {
    if !directory.join("level.dat").is_file() || !directory.join("player.dat").is_file() {
        return None;
    }
    let manager = crate::save::SaveManager::new(directory);
    let (level, player) = manager.load_player_and_level().ok()?;
    let modified = fs::metadata(directory.join("player.dat"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("WORLD")
        .replace('_', " ")
        .to_ascii_uppercase();
    Some(WorldMetadata {
        name,
        seed: level.seed,
        game_mode: player.game_mode,
        difficulty: Difficulty::Normal,
        last_played: modified,
        world_type: WorldType::Default,
        generate_structures: true,
        bonus_chest: false,
        cheats_enabled: false,
        hardcore: false,
        version: level.version,
        needs_upgrade: level.version < CURRENT_WORLD_FORMAT_VERSION,
    })
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_'))
        .take(24)
        .collect::<String>()
        .trim()
        .to_string()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "new_world".to_string()
    } else {
        slug.to_string()
    }
}

fn unique_world_dir(name: &str) -> PathBuf {
    let base = slugify(name);
    let mut candidate = Path::new(SAVES_DIR).join(&base);
    let mut suffix = 2;
    while candidate.exists() {
        candidate = Path::new(SAVES_DIR).join(format!("{base}_{suffix}"));
        suffix += 1;
    }
    candidate
}

fn canonical_saves_root() -> std::io::Result<PathBuf> {
    let root = Path::new(SAVES_DIR);
    if !root.exists() {
        fs::create_dir_all(root)?;
    }
    fs::canonicalize(root)
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    candidate != root && candidate.starts_with(root)
}

/// Resolve a world directory while rejecting the saves root itself, traversal,
/// and symlink escapes. All destructive/copy operations use this guard.
pub fn validated_world_path(path: &Path) -> std::io::Result<PathBuf> {
    let root = canonical_saves_root()?;
    let candidate = fs::canonicalize(path)?;
    if path_is_within(&root, &candidate) {
        Ok(candidate)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "world path must remain inside the saves directory",
        ))
    }
}

pub fn delete_world(path: &Path) -> std::io::Result<()> {
    let directory = validated_world_path(path)?;
    fs::remove_dir_all(directory)
}

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "symlink entries are not valid world data",
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }
    Ok(())
}

pub fn copy_world(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = validated_world_path(source)?;
    let root = canonical_saves_root()?;
    let destination = if destination.exists() {
        fs::canonicalize(destination)?
    } else {
        let parent = destination.parent().unwrap_or(Path::new(SAVES_DIR));
        let parent = fs::canonicalize(parent)?;
        parent.join(destination.file_name().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination has no name")
        })?)
    };
    if !path_is_within(&root, &destination) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "destination must remain inside the saves directory",
        ));
    }
    if destination.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination world already exists",
        ));
    }
    copy_tree(&source, &destination)
}

pub fn backup_world(source: &Path, destination: &Path) -> std::io::Result<()> {
    copy_world(source, destination)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PanoramaUniform {
    time: f32,
    width: f32,
    height: f32,
    _padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct UiVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl UiVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuScreen {
    Main,
    Multiplayer,
    Worlds,
    CreateWorld,
    Options,
    Controls,
    Accessibility,
    ResourcePacks,
    ConfirmDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextField {
    WorldName,
    Seed,
    HostPort,
    ServerAddress,
    JoinPort,
    Username,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultiplayerMode {
    Host,
    Join,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlAction {
    Forward,
    Backward,
    Left,
    Right,
    Jump,
    Sprint,
    Sneak,
    Inventory,
}

impl ControlAction {
    fn label(self) -> &'static str {
        match self {
            Self::Forward => "FORWARD",
            Self::Backward => "BACKWARD",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::Jump => "JUMP",
            Self::Sprint => "SPRINT",
            Self::Sneak => "SNEAK",
            Self::Inventory => "INVENTORY",
        }
    }
}

fn back_transition(
    screen: MenuScreen,
    _active_field: Option<TextField>,
    _rebinding: Option<ControlAction>,
) -> (MenuScreen, Option<TextField>, Option<ControlAction>) {
    let screen = match screen {
        MenuScreen::Main => MenuScreen::Main,
        MenuScreen::Multiplayer | MenuScreen::Worlds | MenuScreen::Options => MenuScreen::Main,
        MenuScreen::CreateWorld | MenuScreen::ConfirmDelete => MenuScreen::Worlds,
        MenuScreen::Controls | MenuScreen::Accessibility | MenuScreen::ResourcePacks => {
            MenuScreen::Options
        }
    };
    (screen, None, None)
}

pub enum MenuAction {
    None,
    Launch(WorldLaunch, GameSettings),
    Quit,
}

pub struct Menu {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    panorama_pipeline: wgpu::RenderPipeline,
    panorama_buffer: wgpu::Buffer,
    panorama_bind_group: wgpu::BindGroup,
    ui_pipeline: wgpu::RenderPipeline,
    ui_buffer: wgpu::Buffer,
    elapsed: f32,
    mouse_ndc: [f32; 2],
    screen: MenuScreen,
    worlds: Vec<WorldEntry>,
    /// Store the selected save identity, not its current list index. The list
    /// is sorted by last-played time, so an index can refer to another world
    /// after a refresh.
    selected_world: Option<PathBuf>,
    world_scroll: usize,
    create_name: String,
    create_seed: String,
    create_mode: GameMode,
    create_difficulty: Difficulty,
    create_world_type: WorldType,
    create_generate_structures: bool,
    create_bonus_chest: bool,
    create_cheats: bool,
    create_hardcore: bool,
    multiplayer_mode: MultiplayerMode,
    selected_role: MultiplayerRole,
    host_port: String,
    server_address: String,
    join_port: String,
    username: String,
    active_field: Option<TextField>,
    rebinding: Option<ControlAction>,
    message: Option<String>,
    pub settings: GameSettings,
    resource_packs: ResourcePackManager,
    catalog: TranslationCatalog,
    focus_index: usize,
    supported_present_modes: Vec<wgpu::PresentMode>,
}

impl Menu {
    fn refresh_catalog(&mut self) {
        self.catalog = TranslationCatalog::from_resource_packs_mut(
            &mut self.resource_packs,
            self.settings.language,
        );
    }

    fn tr(&self, key: &str) -> String {
        self.catalog.lookup(key)
    }

    pub async fn new(window: Arc<Window>, settings: GameSettings) -> Self {
        window.set_cursor_visible(true);
        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
        apply_fullscreen(&window, settings.fullscreen);
        let size = window.inner_size();
        // On this Windows/NVIDIA setup the Vulkan ICD crashes while the game
        // surface is created. `PRIMARY` still prefers Vulkan, so explicitly
        // select DX12 on Windows and use the normal primary backends elsewhere.
        let backends = if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("No compatible graphics adapter found");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .expect("Could not create graphics device");
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(caps.formats[0]);
        let supported_present_modes = caps.present_modes.clone();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: present_mode(settings.vsync, &caps.present_modes),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let panorama_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Menu Panorama Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(PANORAMA_SHADER)),
        });
        let panorama_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Menu Panorama Uniform"),
            contents: bytemuck::bytes_of(&PanoramaUniform {
                time: 0.0,
                width: size.width as f32,
                height: size.height as f32,
                _padding: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let panorama_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Menu Panorama Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let panorama_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Menu Panorama Bind Group"),
            layout: &panorama_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: panorama_buffer.as_entire_binding(),
            }],
        });
        let panorama_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Menu Panorama Pipeline Layout"),
                bind_group_layouts: &[&panorama_layout],
                push_constant_ranges: &[],
            });
        let panorama_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Menu Panorama Pipeline"),
            layout: Some(&panorama_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &panorama_shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &panorama_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Menu UI Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(UI_SHADER)),
        });
        let ui_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Menu UI Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Menu UI Pipeline"),
            layout: Some(&ui_layout),
            vertex: wgpu::VertexState {
                module: &ui_shader,
                entry_point: "vs_main",
                buffers: &[UiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let ui_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Menu UI Vertex Buffer"),
            size: (UI_VERTEX_CAPACITY * std::mem::size_of::<UiVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut resource_packs = ResourcePackManager::discover_default();
        if !settings.resource_packs.is_empty() {
            let _ = resource_packs.apply_enabled_order(&settings.resource_packs);
        }
        let catalog =
            TranslationCatalog::from_resource_packs_mut(&mut resource_packs, settings.language);
        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            panorama_pipeline,
            panorama_buffer,
            panorama_bind_group,
            ui_pipeline,
            ui_buffer,
            elapsed: 0.0,
            mouse_ndc: [0.0, 0.0],
            screen: MenuScreen::Main,
            worlds: discover_worlds(),
            selected_world: None,
            world_scroll: 0,
            create_name: "NEW WORLD".to_string(),
            create_seed: String::new(),
            create_mode: GameMode::Survival,
            create_difficulty: settings.difficulty,
            create_world_type: WorldType::Default,
            create_generate_structures: true,
            create_bonus_chest: false,
            create_cheats: false,
            create_hardcore: false,
            multiplayer_mode: MultiplayerMode::Host,
            selected_role: MultiplayerRole::Singleplayer,
            host_port: settings.mp_host_port.clone(),
            server_address: settings.mp_server_address.clone(),
            join_port: settings.mp_join_port.clone(),
            username: settings.mp_username.clone(),
            active_field: None,
            rebinding: None,
            message: None,
            settings,
            resource_packs,
            catalog,
            focus_index: 0,
            supported_present_modes,
        }
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.size = size;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn handle_mouse_move(&mut self, x: f64, y: f64) {
        let ui_scale = self.settings.accessibility.ui_scale.max(0.5) as f64;
        self.mouse_ndc = [
            (x as f32 / self.size.width.max(1) as f32 * 2.0 - 1.0) / ui_scale as f32,
            (1.0 - y as f32 / self.size.height.max(1) as f32 * 2.0) / ui_scale as f32,
        ];
    }

    pub fn handle_scroll(&mut self, direction: i32) {
        if self.screen != MenuScreen::Worlds || self.worlds.len() <= 5 {
            return;
        }
        let max_scroll = self.worlds.len() - 5;
        self.world_scroll =
            (self.world_scroll as i32 + direction).clamp(0, max_scroll as i32) as usize;
    }

    pub fn handle_key(
        &mut self,
        state: ElementState,
        physical_key: PhysicalKey,
        logical_key: &Key,
        repeat: bool,
        shift_held: bool,
    ) -> MenuAction {
        if state != ElementState::Pressed {
            return MenuAction::None;
        }
        if let Some(action) = self.rebinding.take() {
            if let PhysicalKey::Code(code) = physical_key {
                if code != KeyCode::Escape {
                    *self.control_mut(action) = code;
                    self.settings.save();
                }
            }
            return MenuAction::None;
        }
        if let Some(field) = self.active_field {
            match logical_key {
                Key::Named(NamedKey::Escape) => self.active_field = None,
                Key::Named(NamedKey::Backspace) => match field {
                    TextField::WorldName => {
                        self.create_name.pop();
                    }
                    TextField::Seed => {
                        self.create_seed.pop();
                    }
                    TextField::HostPort => {
                        self.host_port.pop();
                    }
                    TextField::ServerAddress => {
                        self.server_address.pop();
                    }
                    TextField::JoinPort => {
                        self.join_port.pop();
                    }
                    TextField::Username => {
                        self.username.pop();
                    }
                },
                Key::Named(NamedKey::Enter) => self.active_field = None,
                Key::Character(text) if !repeat => {
                    for ch in text.chars() {
                        match field {
                            TextField::WorldName
                                if self.create_name.len() < 24
                                    && (ch.is_ascii_alphanumeric()
                                        || matches!(ch, ' ' | '-' | '_')) =>
                            {
                                self.create_name.push(ch.to_ascii_uppercase())
                            }
                            TextField::Seed
                                if self.create_seed.len() < 10
                                    && (ch.is_ascii_digit()
                                        || (ch == '-' && self.create_seed.is_empty())) =>
                            {
                                self.create_seed.push(ch)
                            }
                            TextField::HostPort
                                if self.host_port.len() < 5 && ch.is_ascii_digit() =>
                            {
                                self.host_port.push(ch)
                            }
                            TextField::ServerAddress
                                if self.server_address.len() < 64
                                    && (ch.is_ascii_alphanumeric()
                                        || matches!(ch, '.' | '-' | ':' | '_')) =>
                            {
                                self.server_address.push(ch)
                            }
                            TextField::JoinPort
                                if self.join_port.len() < 5 && ch.is_ascii_digit() =>
                            {
                                self.join_port.push(ch)
                            }
                            TextField::Username
                                if self.username.len() < 16
                                    && (ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')) =>
                            {
                                self.username.push(ch.to_ascii_uppercase())
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return MenuAction::None;
        }
        if matches!(logical_key, Key::Named(NamedKey::Tab)) {
            let direction = if shift_held {
                crate::accessibility::FocusDirection::Backward
            } else {
                crate::accessibility::FocusDirection::Forward
            };
            self.move_focus(direction);
            return MenuAction::None;
        }
        if matches!(logical_key, Key::Named(NamedKey::Enter)) {
            return self.activate_focused();
        }
        if matches!(logical_key, Key::Named(NamedKey::Escape)) {
            self.back();
        }
        MenuAction::None
    }

    fn move_focus(&mut self, direction: crate::accessibility::FocusDirection) {
        let mut focus = crate::accessibility::FocusNavigator::new(self.focus_count());
        focus.set_count(self.focus_count());
        for _ in 0..self.focus_index {
            focus.move_by(crate::accessibility::FocusDirection::Forward);
        }
        focus.move_by(direction);
        self.focus_index = focus.index();
    }

    fn activate_focused(&mut self) -> MenuAction {
        let Some([x0, x1, y0, y1]) = self.focus_rect() else {
            return MenuAction::None;
        };
        self.mouse_ndc = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
        self.handle_click()
    }

    fn focus_count(&self) -> usize {
        match self.screen {
            MenuScreen::Main => 4,
            MenuScreen::Options => 15,
            MenuScreen::Controls => 10,
            MenuScreen::Accessibility => 11,
            MenuScreen::ResourcePacks => self.resource_packs.available().len() + 3,
            MenuScreen::Multiplayer => {
                if self.multiplayer_mode == MultiplayerMode::Join {
                    7
                } else {
                    5
                }
            }
            MenuScreen::Worlds => (self.worlds.len().saturating_sub(self.world_scroll)).min(5) + 6,
            MenuScreen::CreateWorld => 11,
            MenuScreen::ConfirmDelete => 2,
        }
    }

    fn focus_rect(&self) -> Option<[f32; 4]> {
        let rects = match self.screen {
            MenuScreen::Main => vec![
                [-0.34, 0.34, 0.21, 0.34],
                [-0.34, 0.34, 0.03, 0.16],
                [-0.34, 0.34, -0.15, -0.02],
                [-0.34, 0.34, -0.33, -0.20],
            ],
            MenuScreen::Options => {
                let mut rects = Vec::with_capacity(14);
                for row in 0..6 {
                    let top = OPTIONS_ROW_TOPS[row];
                    rects.push([-0.82, -0.05, top - 0.13, top]);
                }
                for row in 0..6 {
                    let top = OPTIONS_ROW_TOPS[row];
                    rects.push([0.05, 0.82, top - 0.13, top]);
                }
                rects.push([-0.82, -0.30, -0.78, -0.64]);
                rects.push([-0.25, 0.25, -0.78, -0.64]);
                rects.push([0.30, 0.82, -0.78, -0.64]);
                rects
            }
            MenuScreen::Controls => {
                let mut rects = vec![[-0.48, 0.48, 0.49, 0.62]];
                for index in 0..8 {
                    let column = index / 4;
                    let row = index % 4;
                    let (x0, x1) = if column == 0 {
                        (-0.78, -0.04)
                    } else {
                        (0.04, 0.78)
                    };
                    let top = 0.38 - row as f32 * 0.19;
                    rects.push([x0, x1, top - 0.14, top]);
                }
                rects.push([-0.25, 0.25, -0.78, -0.64]);
                rects
            }
            MenuScreen::Accessibility => {
                let mut rects = Vec::with_capacity(11);
                for index in 0..10 {
                    let column = index / 5;
                    let row = index % 5;
                    let (x0, x1) = if column == 0 {
                        (-0.82, -0.05)
                    } else {
                        (0.05, 0.82)
                    };
                    let top = 0.56 - row as f32 * 0.18;
                    rects.push([x0, x1, top - 0.13, top]);
                }
                rects.push([-0.25, 0.25, -0.78, -0.64]);
                rects
            }
            MenuScreen::ResourcePacks => {
                let mut rects = self
                    .resource_packs
                    .available()
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        let top = 0.56 - index as f32 * 0.14;
                        [-0.78, 0.78, top - 0.11, top]
                    })
                    .collect::<Vec<_>>();
                rects.extend([
                    [-0.78, -0.28, -0.78, -0.64],
                    [-0.22, 0.22, -0.78, -0.64],
                    [0.28, 0.78, -0.78, -0.64],
                ]);
                rects
            }
            MenuScreen::Multiplayer => {
                let mut rects = vec![[-0.52, -0.02, 0.45, 0.58], [0.02, 0.52, 0.45, 0.58]];
                if self.multiplayer_mode == MultiplayerMode::Host {
                    rects.push([-0.52, 0.52, 0.17, 0.30]);
                } else {
                    rects.extend([
                        [-0.52, 0.52, 0.17, 0.30],
                        [-0.52, 0.52, -0.04, 0.09],
                        [-0.52, 0.52, -0.25, -0.12],
                    ]);
                }
                rects.extend([[-0.52, -0.02, -0.58, -0.45], [0.02, 0.52, -0.58, -0.45]]);
                rects
            }
            MenuScreen::Worlds => {
                let visible = (self.worlds.len().saturating_sub(self.world_scroll)).min(5);
                let mut rects = (0..visible)
                    .map(|index| {
                        let top = 0.58 - index as f32 * 0.19;
                        [-0.72, 0.72, top - 0.15, top]
                    })
                    .collect::<Vec<_>>();
                rects.extend([
                    [-0.72, -0.27, -0.64, -0.51],
                    [-0.23, 0.23, -0.64, -0.51],
                    [0.27, 0.72, -0.64, -0.51],
                    [-0.72, -0.27, -0.84, -0.72],
                    [-0.23, 0.23, -0.84, -0.72],
                    [0.27, 0.72, -0.84, -0.72],
                ]);
                rects
            }
            MenuScreen::CreateWorld => vec![
                [-0.52, 0.52, 0.34, 0.47],
                [-0.52, 0.52, 0.13, 0.26],
                [-0.52, 0.52, -0.08, 0.05],
                [-0.52, 0.52, -0.29, -0.16],
                [-0.52, 0.52, -0.42, -0.30],
                [-0.52, -0.02, -0.54, -0.43],
                [0.02, 0.52, -0.54, -0.43],
                [-0.52, -0.02, -0.69, -0.58],
                [0.02, 0.52, -0.69, -0.58],
                [-0.52, -0.02, -0.84, -0.71],
                [0.02, 0.52, -0.84, -0.71],
            ],
            MenuScreen::ConfirmDelete => {
                vec![[-0.48, -0.02, -0.16, -0.02], [0.02, 0.48, -0.16, -0.02]]
            }
        };
        rects
            .get(self.focus_index.min(rects.len().saturating_sub(1)))
            .copied()
    }

    pub fn handle_click(&mut self) -> MenuAction {
        let [x, y] = self.mouse_ndc;
        self.message = None;
        match self.screen {
            MenuScreen::Main => {
                if hit(x, y, -0.34, 0.34, 0.21, 0.34) {
                    self.selected_role = MultiplayerRole::Singleplayer;
                    self.worlds = discover_worlds();
                    self.selected_world = self.worlds.first().map(|world| world.directory.clone());
                    self.world_scroll = 0;
                    self.screen = MenuScreen::Worlds;
                } else if hit(x, y, -0.34, 0.34, 0.03, 0.16) {
                    self.active_field = None;
                    self.screen = MenuScreen::Multiplayer;
                } else if hit(x, y, -0.34, 0.34, -0.15, -0.02) {
                    self.screen = MenuScreen::Options;
                } else if hit(x, y, -0.34, 0.34, -0.33, -0.20) {
                    return MenuAction::Quit;
                }
            }
            MenuScreen::Multiplayer => {
                if hit(x, y, -0.52, -0.02, 0.45, 0.58) {
                    self.multiplayer_mode = MultiplayerMode::Host;
                    self.active_field = None;
                } else if hit(x, y, 0.02, 0.52, 0.45, 0.58) {
                    self.multiplayer_mode = MultiplayerMode::Join;
                    self.active_field = None;
                } else if self.multiplayer_mode == MultiplayerMode::Host
                    && hit(x, y, -0.52, 0.52, 0.17, 0.30)
                {
                    self.active_field = Some(TextField::HostPort);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && hit(x, y, -0.52, 0.52, 0.17, 0.30)
                {
                    self.active_field = Some(TextField::ServerAddress);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && hit(x, y, -0.52, 0.52, -0.04, 0.09)
                {
                    self.active_field = Some(TextField::JoinPort);
                } else if self.multiplayer_mode == MultiplayerMode::Join
                    && hit(x, y, -0.52, 0.52, -0.25, -0.12)
                {
                    self.active_field = Some(TextField::Username);
                } else if hit(x, y, -0.52, -0.02, -0.58, -0.45) {
                    let role = match self.multiplayer_mode {
                        MultiplayerMode::Host => self
                            .host_port
                            .parse::<u16>()
                            .ok()
                            .filter(|p| *p > 0)
                            .map(|port| MultiplayerRole::Host { port }),
                        MultiplayerMode::Join => {
                            let address = self.server_address.trim();
                            let username = self.username.trim();
                            self.join_port
                                .parse::<u16>()
                                .ok()
                                .filter(|port| {
                                    *port > 0 && !address.is_empty() && !username.is_empty()
                                })
                                .map(|port| MultiplayerRole::Client {
                                    server_addr: address.to_string(),
                                    port,
                                    username: username.to_string(),
                                })
                        }
                    };
                    let Some(role) = role else {
                        self.message = Some(self.tr("menu.enter_valid_multiplayer"));
                        return MenuAction::None;
                    };
                    let is_client = matches!(role, MultiplayerRole::Client { .. });
                    self.selected_role = role;
                    self.sync_and_save_multiplayer_settings();
                    self.active_field = None;
                    if is_client {
                        return self.launch_client();
                    }
                    self.worlds = discover_worlds();
                    self.selected_world = self.worlds.first().map(|world| world.directory.clone());
                    self.world_scroll = 0;
                    self.screen = MenuScreen::Worlds;
                } else if hit(x, y, 0.02, 0.52, -0.58, -0.45) {
                    self.sync_and_save_multiplayer_settings();
                    self.active_field = None;
                    self.screen = MenuScreen::Main;
                }
            }
            MenuScreen::Worlds => {
                for visible_index in 0..(self.worlds.len() - self.world_scroll).min(5) {
                    let index = self.world_scroll + visible_index;
                    let top = 0.58 - visible_index as f32 * 0.19;
                    if hit(x, y, -0.72, 0.72, top - 0.15, top) {
                        self.selected_world = Some(self.worlds[index].directory.clone());
                        return MenuAction::None;
                    }
                }
                if hit(x, y, -0.72, -0.27, -0.64, -0.51) {
                    if let Some(directory) = self.selected_world.clone() {
                        return self.launch_existing(&directory);
                    }
                } else if hit(x, y, -0.23, 0.23, -0.64, -0.51) {
                    self.create_name = "NEW WORLD".to_string();
                    self.create_seed.clear();
                    self.create_mode = GameMode::Survival;
                    self.create_difficulty = self.settings.difficulty;
                    self.create_world_type = WorldType::Default;
                    self.create_generate_structures = true;
                    self.create_bonus_chest = false;
                    self.create_cheats = false;
                    self.create_hardcore = false;
                    self.screen = MenuScreen::CreateWorld;
                } else if hit(x, y, 0.27, 0.72, -0.64, -0.51) {
                    if self.selected_world.is_some() {
                        self.screen = MenuScreen::ConfirmDelete;
                    }
                } else if hit(x, y, -0.72, -0.27, -0.84, -0.72) {
                    if let Some(directory) = self.selected_world.clone() {
                        let base = directory
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("world");
                        let destination = unique_world_dir(&format!("{base}_copy"));
                        match copy_world(&directory, &destination) {
                            Ok(()) => {
                                self.worlds = discover_worlds();
                                self.message = Some(self.tr("menu.world_copied"));
                            }
                            Err(error) => self.message = Some(format!("COPY FAILED: {error}")),
                        }
                    }
                } else if hit(x, y, -0.23, 0.23, -0.84, -0.72) {
                    if let Some(directory) = self.selected_world.clone() {
                        let base = directory
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("world");
                        let destination = unique_world_dir(&format!("{base}_backup"));
                        match backup_world(&directory, &destination) {
                            Ok(()) => {
                                self.worlds = discover_worlds();
                                self.message = Some(self.tr("menu.world_backed_up"));
                            }
                            Err(error) => self.message = Some(format!("BACKUP FAILED: {error}")),
                        }
                    }
                } else if hit(x, y, 0.27, 0.72, -0.84, -0.72) {
                    self.screen = MenuScreen::Main;
                }
            }
            MenuScreen::CreateWorld => {
                if hit(x, y, -0.52, 0.52, 0.34, 0.47) {
                    self.active_field = Some(TextField::WorldName);
                } else if hit(x, y, -0.52, 0.52, 0.13, 0.26) {
                    self.active_field = Some(TextField::Seed);
                } else if hit(x, y, -0.52, 0.52, -0.08, 0.05) {
                    self.create_mode = match self.create_mode {
                        GameMode::Survival => GameMode::Creative,
                        GameMode::Creative => GameMode::Adventure,
                        GameMode::Adventure => GameMode::Spectator,
                        GameMode::Spectator => GameMode::Survival,
                    };
                } else if hit(x, y, -0.52, 0.52, -0.29, -0.16) {
                    self.create_difficulty =
                        self.create_difficulty.step(if x < 0.0 { -1 } else { 1 });
                } else if hit(x, y, -0.52, 0.52, -0.42, -0.30) {
                    self.create_world_type = match self.create_world_type {
                        WorldType::Default => WorldType::Superflat,
                        WorldType::Superflat => WorldType::Default,
                    };
                } else if hit(x, y, -0.52, -0.02, -0.54, -0.43) {
                    self.create_generate_structures = !self.create_generate_structures;
                } else if hit(x, y, 0.02, 0.52, -0.54, -0.43) {
                    self.create_hardcore = !self.create_hardcore;
                    if self.create_hardcore {
                        self.create_mode = GameMode::Survival;
                        self.create_difficulty = Difficulty::Hard;
                    }
                } else if hit(x, y, -0.52, -0.02, -0.69, -0.58) {
                    self.create_bonus_chest = !self.create_bonus_chest;
                } else if hit(x, y, 0.02, 0.52, -0.69, -0.58) {
                    self.create_cheats = !self.create_cheats;
                } else if hit(x, y, -0.52, -0.02, -0.84, -0.71) {
                    return self.create_world();
                } else if hit(x, y, 0.02, 0.52, -0.84, -0.71) {
                    self.active_field = None;
                    self.screen = MenuScreen::Worlds;
                }
            }
            MenuScreen::Options => self.handle_options_click(x, y),
            MenuScreen::Controls => {
                if hit(x, y, -0.48, 0.48, 0.49, 0.62) {
                    let delta = if x < 0.0 { -0.0002 } else { 0.0002 };
                    self.settings.sensitivity =
                        (self.settings.sensitivity + delta).clamp(0.0002, 0.006);
                    self.settings.save();
                    return MenuAction::None;
                }
                let actions = [
                    ControlAction::Forward,
                    ControlAction::Backward,
                    ControlAction::Left,
                    ControlAction::Right,
                    ControlAction::Jump,
                    ControlAction::Sprint,
                    ControlAction::Sneak,
                    ControlAction::Inventory,
                ];
                for (index, action) in actions.into_iter().enumerate() {
                    let column = index / 4;
                    let row = index % 4;
                    let (x0, x1) = if column == 0 {
                        (-0.78, -0.04)
                    } else {
                        (0.04, 0.78)
                    };
                    let top = 0.38 - row as f32 * 0.19;
                    if hit(x, y, x0, x1, top - 0.14, top) {
                        self.rebinding = Some(action);
                    }
                }
                if hit(x, y, -0.25, 0.25, -0.78, -0.64) {
                    self.back();
                }
            }
            MenuScreen::Accessibility => self.handle_accessibility_click(x, y),
            MenuScreen::ResourcePacks => self.handle_resource_pack_click(x, y),
            MenuScreen::ConfirmDelete => {
                if hit(x, y, -0.48, -0.02, -0.16, -0.02) {
                    if let Some(directory) = self.selected_world.as_deref() {
                        if let Some(world) = world_index_by_directory(&self.worlds, directory)
                            .and_then(|index| self.worlds.get(index))
                        {
                            if let Err(error) = delete_world(&world.directory) {
                                self.message = Some(format!("DELETE FAILED: {error}"));
                                self.screen = MenuScreen::Worlds;
                                return MenuAction::None;
                            }
                        }
                    }
                    self.worlds = discover_worlds();
                    self.selected_world = self.worlds.first().map(|world| world.directory.clone());
                    self.world_scroll = self.world_scroll.min(self.worlds.len().saturating_sub(5));
                    self.screen = MenuScreen::Worlds;
                } else if hit(x, y, 0.02, 0.48, -0.16, -0.02) {
                    self.screen = MenuScreen::Worlds;
                }
            }
        }
        self.focus_index = self.focus_index.min(self.focus_count().saturating_sub(1));
        MenuAction::None
    }

    fn sync_and_save_multiplayer_settings(&mut self) {
        self.settings.mp_host_port = self.host_port.clone();
        self.settings.mp_server_address = self.server_address.clone();
        self.settings.mp_join_port = self.join_port.clone();
        self.settings.mp_username = self.username.clone();
        self.settings.save();
    }

    fn back(&mut self) {
        if self.screen == MenuScreen::Multiplayer {
            self.sync_and_save_multiplayer_settings();
        }
        let (screen, active_field, rebinding) =
            back_transition(self.screen, self.active_field, self.rebinding);
        self.screen = screen;
        self.active_field = active_field;
        self.rebinding = rebinding;
    }

    fn launch_existing(&mut self, directory: &Path) -> MenuAction {
        let Some(index) = world_index_by_directory(&self.worlds, directory) else {
            return MenuAction::None;
        };
        let world = &mut self.worlds[index];
        world.metadata.last_played = unix_now();
        let _ = world.metadata.save(&world.directory);
        let world_dir =
            fs::canonicalize(&world.directory).unwrap_or_else(|_| world.directory.clone());
        MenuAction::Launch(
            WorldLaunch {
                world_dir,
                seed: world.metadata.seed,
                game_mode: world.metadata.game_mode,
                difficulty: world.metadata.difficulty,
                role: self.selected_role.clone(),
            },
            self.settings.clone(),
        )
    }

    fn create_world(&mut self) -> MenuAction {
        let name = sanitize_name(&self.create_name);
        if name.is_empty() {
            self.message = Some("ENTER A WORLD NAME".to_string());
            return MenuAction::None;
        }
        let seed = if self.create_seed.trim().is_empty() {
            (unix_now() as u32)
                .wrapping_mul(747_796_405)
                .wrapping_add(2_891_336_453)
        } else {
            self.create_seed
                .trim()
                .parse::<i64>()
                .map(|seed| seed as u32)
                .unwrap_or_else(|_| hash_seed(&self.create_seed))
        };
        let world_dir = unique_world_dir(&name);
        let metadata = WorldMetadata {
            name,
            seed,
            game_mode: self.create_mode,
            difficulty: self.create_difficulty,
            last_played: unix_now(),
            world_type: self.create_world_type,
            generate_structures: self.create_generate_structures,
            bonus_chest: self.create_bonus_chest,
            cheats_enabled: self.create_cheats,
            hardcore: self.create_hardcore,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        };
        if let Err(error) = metadata.save(&world_dir) {
            self.message = Some(format!("CREATE FAILED: {error}"));
            return MenuAction::None;
        }
        let world_dir = fs::canonicalize(&world_dir).unwrap_or(world_dir);
        MenuAction::Launch(
            WorldLaunch {
                world_dir,
                seed,
                game_mode: self.create_mode,
                difficulty: self.create_difficulty,
                role: self.selected_role.clone(),
            },
            self.settings.clone(),
        )
    }

    fn launch_client(&mut self) -> MenuAction {
        let world_dir = std::env::temp_dir().join("icraft_multiplayer_client");
        MenuAction::Launch(
            WorldLaunch {
                world_dir,
                seed: 0,
                game_mode: GameMode::Survival,
                difficulty: self.settings.difficulty,
                role: self.selected_role.clone(),
            },
            self.settings.clone(),
        )
    }

    fn handle_options_click(&mut self, x: f32, y: f32) {
        let left = x >= -0.82 && x <= -0.05;
        let right = x >= 0.05 && x <= 0.82;
        let row = options_row_at(y);
        let delta = if x < -0.43 || (x > 0.05 && x < 0.43) {
            -1.0
        } else {
            1.0
        };
        match (left, right, row) {
            (true, _, Some(0)) => {
                self.settings.fov = (self.settings.fov + delta * 5.0).clamp(30.0, 120.0)
            }
            (true, _, Some(1)) => {
                self.settings.render_distance =
                    (self.settings.render_distance + delta as i32).clamp(2, 16)
            }
            (true, _, Some(2)) => {
                self.settings.fullscreen = !self.settings.fullscreen;
                apply_fullscreen(&self.window, self.settings.fullscreen);
            }
            (true, _, Some(3)) => {
                if self.settings.vsync {
                    let has_uncapped = self
                        .supported_present_modes
                        .contains(&wgpu::PresentMode::Mailbox)
                        || self
                            .supported_present_modes
                            .contains(&wgpu::PresentMode::Immediate);
                    if !has_uncapped {
                        self.message = Some("VSYNC REQUIRED ON THIS DISPLAY".to_string());
                        return;
                    }
                    self.settings.vsync = false;
                    self.config.present_mode = present_mode(false, &self.supported_present_modes);
                } else {
                    self.settings.vsync = true;
                    self.config.present_mode = wgpu::PresentMode::Fifo;
                }
                self.surface.configure(&self.device, &self.config);
            }
            (true, _, Some(4)) => {
                self.settings.difficulty = self.settings.difficulty.step(delta as i32)
            }
            (true, _, Some(5)) => {
                self.settings.fps_cap = cycle_fps_cap(self.settings.fps_cap, delta as i32);
            }
            (_, true, Some(0)) => {
                self.settings.master_volume =
                    (self.settings.master_volume + delta * 0.1).clamp(0.0, 1.0)
            }
            (_, true, Some(1)) => {
                self.settings.music_volume =
                    (self.settings.music_volume + delta * 0.1).clamp(0.0, 1.0)
            }
            (_, true, Some(2)) => {
                self.settings.sound_volume =
                    (self.settings.sound_volume + delta * 0.1).clamp(0.0, 1.0)
            }
            (_, true, Some(3)) => {
                self.settings.weather_volume =
                    (self.settings.weather_volume + delta * 0.1).clamp(0.0, 1.0)
            }
            (_, true, Some(4)) => {
                self.settings.language = self.settings.language.toggle();
                self.refresh_catalog();
            }
            (_, true, Some(5)) => {
                self.screen = MenuScreen::Accessibility;
                return;
            }
            _ if hit(x, y, -0.82, -0.30, -0.78, -0.64) => {
                self.screen = MenuScreen::ResourcePacks;
                self.focus_index = 0;
                return;
            }
            _ if hit(x, y, -0.25, 0.25, -0.78, -0.64) => {
                self.screen = MenuScreen::Controls;
                self.focus_index = 0;
                return;
            }
            _ if hit(x, y, 0.30, 0.82, -0.78, -0.64) => {
                self.screen = MenuScreen::Main;
                self.focus_index = 0;
                return;
            }
            _ => return,
        }
        self.settings.save();
    }

    fn control_mut(&mut self, action: ControlAction) -> &mut KeyCode {
        match action {
            ControlAction::Forward => &mut self.settings.controls.forward,
            ControlAction::Backward => &mut self.settings.controls.backward,
            ControlAction::Left => &mut self.settings.controls.left,
            ControlAction::Right => &mut self.settings.controls.right,
            ControlAction::Jump => &mut self.settings.controls.jump,
            ControlAction::Sprint => &mut self.settings.controls.sprint,
            ControlAction::Sneak => &mut self.settings.controls.sneak,
            ControlAction::Inventory => &mut self.settings.controls.inventory,
        }
    }

    pub fn update(&mut self, dt: f32) {
        let motion_scale = if self.settings.accessibility.reduce_flashing {
            0.25
        } else {
            1.0
        };
        self.elapsed += dt.min(0.1) * motion_scale;
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let uniform = PanoramaUniform {
            time: self.elapsed,
            width: self.size.width as f32,
            height: self.size.height as f32,
            _padding: 0.0,
        };
        self.queue
            .write_buffer(&self.panorama_buffer, 0, bytemuck::bytes_of(&uniform));

        let mut vertices = Vec::with_capacity(8192);
        self.build_ui(&mut vertices);
        let count = vertices.len().min(UI_VERTEX_CAPACITY);
        self.queue
            .write_buffer(&self.ui_buffer, 0, bytemuck::cast_slice(&vertices[..count]));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Menu Render Encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Menu Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.panorama_pipeline);
            pass.set_bind_group(0, &self.panorama_bind_group, &[]);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
            pass.draw(0..count as u32, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }

    fn build_ui(&self, vertices: &mut Vec<UiVertex>) {
        let aspect = self.size.height.max(1) as f32 / self.size.width.max(1) as f32;
        let hovered = |x0, x1, y0, y1| hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, y0, y1);
        draw_rect(vertices, -1.0, 1.0, -1.0, 1.0, [0.02, 0.03, 0.04, 0.30]);
        let ui_start = vertices.len();
        match self.screen {
            MenuScreen::Main => {
                draw_logo(vertices, aspect);
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    0.21,
                    0.34,
                    hovered(-0.34, 0.34, 0.21, 0.34),
                );
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    0.03,
                    0.16,
                    hovered(-0.34, 0.34, 0.03, 0.16),
                );
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    -0.15,
                    -0.02,
                    hovered(-0.34, 0.34, -0.15, -0.02),
                );
                draw_button(
                    vertices,
                    -0.34,
                    0.34,
                    -0.33,
                    -0.20,
                    hovered(-0.34, 0.34, -0.33, -0.20),
                );
                draw_centered_text(
                    vertices,
                    &self.tr("menu.singleplayer"),
                    0.248,
                    0.010,
                    aspect,
                    [1.0; 4],
                );
                draw_centered_text(vertices, "MULTIPLAYER", 0.068, 0.010, aspect, [1.0; 4]);
                draw_centered_text(
                    vertices,
                    &self.tr("menu.options"),
                    -0.112,
                    0.010,
                    aspect,
                    [1.0; 4],
                );
                draw_centered_text(
                    vertices,
                    &self.tr("menu.quit_game"),
                    -0.292,
                    0.010,
                    aspect,
                    [1.0; 4],
                );
                draw_text(
                    vertices,
                    "JAVA-FREE EDITION",
                    -0.96,
                    -0.94,
                    0.006,
                    aspect,
                    [0.8, 0.84, 0.86, 1.0],
                );
            }
            MenuScreen::Multiplayer => self.draw_multiplayer(vertices, aspect),
            MenuScreen::Worlds => self.draw_worlds(vertices, aspect),
            MenuScreen::CreateWorld => self.draw_create(vertices, aspect),
            MenuScreen::Options => self.draw_options(vertices, aspect),
            MenuScreen::Controls => self.draw_controls(vertices, aspect),
            MenuScreen::Accessibility => self.draw_accessibility(vertices, aspect),
            MenuScreen::ResourcePacks => self.draw_resource_packs(vertices, aspect),
            MenuScreen::ConfirmDelete => self.draw_delete_confirmation(vertices, aspect),
        }
        if let Some([x0, x1, y0, y1]) = self.focus_rect() {
            draw_focus_ring(vertices, x0, x1, y0, y1);
        }
        if let Some(message) = &self.message {
            draw_centered_text(
                vertices,
                message,
                -0.94,
                0.007,
                aspect,
                [1.0, 0.35, 0.25, 1.0],
            );
        }
        let ui_scale = self.settings.accessibility.ui_scale.clamp(0.75, 2.0);
        for vertex in vertices.iter_mut().skip(ui_start) {
            vertex.position[0] = (vertex.position[0] * ui_scale).clamp(-1.0, 1.0);
            vertex.position[1] = (vertex.position[1] * ui_scale).clamp(-1.0, 1.0);
            if self.settings.accessibility.high_contrast {
                let is_focus =
                    vertex.color[0] > 0.9 && vertex.color[1] > 0.6 && vertex.color[2] < 0.35;
                if !is_focus {
                    let luminance = vertex.color[0] * 0.2126
                        + vertex.color[1] * 0.7152
                        + vertex.color[2] * 0.0722;
                    let value = if luminance > 0.45 { 1.0 } else { 0.02 };
                    vertex.color[0] = value;
                    vertex.color[1] = value;
                    vertex.color[2] = value;
                }
            }
        }
    }

    fn draw_multiplayer(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.64, 0.64, -0.72, 0.78);
        draw_centered_text(vertices, "MULTIPLAYER", 0.67, 0.012, aspect, [1.0; 4]);

        for (x0, x1, label, selected) in [
            (
                -0.52,
                -0.02,
                "HOST GAME",
                self.multiplayer_mode == MultiplayerMode::Host,
            ),
            (
                0.02,
                0.52,
                "JOIN GAME",
                self.multiplayer_mode == MultiplayerMode::Join,
            ),
        ] {
            let hover = hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, 0.45, 0.58);
            draw_button_state(vertices, x0, x1, 0.45, 0.58, hover, selected);
            draw_centered_text_in(vertices, label, x0, x1, 0.488, 0.007, aspect, [1.0; 4]);
        }

        match self.multiplayer_mode {
            MultiplayerMode::Host => draw_field(
                vertices,
                "PORT",
                &self.host_port,
                -0.52,
                0.52,
                0.17,
                0.30,
                self.active_field == Some(TextField::HostPort),
                aspect,
            ),
            MultiplayerMode::Join => {
                draw_field(
                    vertices,
                    "SERVER ADDRESS",
                    &self.server_address,
                    -0.52,
                    0.52,
                    0.17,
                    0.30,
                    self.active_field == Some(TextField::ServerAddress),
                    aspect,
                );
                draw_field(
                    vertices,
                    "PORT",
                    &self.join_port,
                    -0.52,
                    0.52,
                    -0.04,
                    0.09,
                    self.active_field == Some(TextField::JoinPort),
                    aspect,
                );
                draw_field(
                    vertices,
                    "USERNAME",
                    &self.username,
                    -0.52,
                    0.52,
                    -0.25,
                    -0.12,
                    self.active_field == Some(TextField::Username),
                    aspect,
                );
            }
        }

        let confirm_label = match self.multiplayer_mode {
            MultiplayerMode::Host => "SELECT WORLD",
            MultiplayerMode::Join => "CONNECT",
        };
        for (x0, x1, label) in [(-0.52, -0.02, confirm_label), (0.02, 0.52, "BACK")] {
            let hover = hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, -0.58, -0.45);
            draw_button(vertices, x0, x1, -0.58, -0.45, hover);
            draw_centered_text_in(vertices, label, x0, x1, -0.542, 0.007, aspect, [1.0; 4]);
        }
    }

    fn draw_worlds(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.82, 0.82, -0.9, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.select_world"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
        );
        if self.worlds.is_empty() {
            draw_centered_text(
                vertices,
                "NO WORLDS YET",
                0.14,
                0.010,
                aspect,
                [0.8, 0.8, 0.8, 1.0],
            );
        }
        for (visible_index, world) in self
            .worlds
            .iter()
            .skip(self.world_scroll)
            .take(5)
            .enumerate()
        {
            let top = 0.58 - visible_index as f32 * 0.19;
            let selected = self.selected_world.as_deref() == Some(world.directory.as_path());
            let hover = hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.72,
                0.72,
                top - 0.15,
                top,
            );
            draw_button_state(vertices, -0.72, 0.72, top - 0.15, top, hover, selected);
            draw_text(
                vertices,
                &world.metadata.name,
                -0.68,
                top - 0.055,
                0.008,
                aspect,
                [1.0; 4],
            );
            let detail = format!(
                "{} / {} / {} / {}{} / v{}{}",
                relative_time(world.metadata.last_played),
                game_mode_name(world.metadata.game_mode),
                world.metadata.difficulty.as_str(),
                world.metadata.world_type.as_str(),
                if world.metadata.hardcore {
                    " / HARDCORE"
                } else {
                    ""
                },
                world.metadata.version,
                if world.metadata.needs_upgrade {
                    " / UPGRADE"
                } else {
                    ""
                },
            );
            draw_text(
                vertices,
                &detail,
                -0.68,
                top - 0.125,
                0.0055,
                aspect,
                [0.72, 0.76, 0.78, 1.0],
            );
        }
        if self.worlds.len() > 5 {
            draw_text(
                vertices,
                "SCROLL FOR MORE WORLDS",
                0.38,
                -0.44,
                0.0048,
                aspect,
                [0.72, 0.76, 0.78, 1.0],
            );
        }
        for (x0, x1, label) in [
            (-0.72, -0.27, "PLAY SELECTED"),
            (-0.23, 0.23, "CREATE NEW WORLD"),
            (0.27, 0.72, "DELETE"),
        ] {
            draw_button(
                vertices,
                x0,
                x1,
                -0.64,
                -0.51,
                hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, -0.64, -0.51),
            );
            draw_centered_text_in(vertices, label, x0, x1, -0.602, 0.006, aspect, [1.0; 4]);
        }
        for (x0, x1, label) in [
            (-0.72, -0.27, "COPY"),
            (-0.23, 0.23, "BACKUP"),
            (0.27, 0.72, "BACK"),
        ] {
            draw_button(
                vertices,
                x0,
                x1,
                -0.84,
                -0.72,
                hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, -0.84, -0.72),
            );
            draw_centered_text_in(vertices, label, x0, x1, -0.805, 0.006, aspect, [1.0; 4]);
        }
    }

    fn draw_create(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.64, 0.64, -0.92, 0.78);
        draw_centered_text(vertices, "CREATE NEW WORLD", 0.67, 0.012, aspect, [1.0; 4]);
        draw_field(
            vertices,
            "WORLD NAME",
            &self.create_name,
            -0.52,
            0.52,
            0.34,
            0.47,
            self.active_field == Some(TextField::WorldName),
            aspect,
        );
        let seed = if self.create_seed.is_empty() {
            "RANDOM"
        } else {
            &self.create_seed
        };
        draw_field(
            vertices,
            "SEED",
            seed,
            -0.52,
            0.52,
            0.13,
            0.26,
            self.active_field == Some(TextField::Seed),
            aspect,
        );
        draw_button(
            vertices,
            -0.52,
            0.52,
            -0.08,
            0.05,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                0.52,
                -0.08,
                0.05,
            ),
        );
        draw_centered_text(
            vertices,
            &format!("GAME MODE: < {} >", game_mode_name(self.create_mode)),
            -0.042,
            0.007,
            aspect,
            [1.0; 4],
        );
        draw_button(
            vertices,
            -0.52,
            0.52,
            -0.29,
            -0.16,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                0.52,
                -0.29,
                -0.16,
            ),
        );
        draw_centered_text(
            vertices,
            &format!("DIFFICULTY: < {} >", self.create_difficulty.as_str()),
            -0.252,
            0.007,
            aspect,
            [1.0; 4],
        );
        draw_button(
            vertices,
            -0.52,
            0.52,
            -0.42,
            -0.30,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                0.52,
                -0.42,
                -0.30,
            ),
        );
        draw_centered_text(
            vertices,
            &format!("WORLD TYPE: < {} >", self.create_world_type.as_str()),
            -0.372,
            0.007,
            aspect,
            [1.0; 4],
        );
        draw_button(
            vertices,
            -0.52,
            -0.02,
            -0.54,
            -0.43,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                -0.02,
                -0.54,
                -0.43,
            ),
        );
        draw_button(
            vertices,
            0.02,
            0.52,
            -0.54,
            -0.43,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                0.02,
                0.52,
                -0.54,
                -0.43,
            ),
        );
        draw_centered_text_in(
            vertices,
            &format!(
                "STRUCTURES: {}",
                if self.create_generate_structures {
                    "ON"
                } else {
                    "OFF"
                }
            ),
            -0.52,
            -0.02,
            -0.505,
            0.005,
            aspect,
            [1.0; 4],
        );
        draw_centered_text_in(
            vertices,
            &format!(
                "HARDCORE: {}",
                if self.create_hardcore { "ON" } else { "OFF" }
            ),
            0.02,
            0.52,
            -0.505,
            0.005,
            aspect,
            [1.0; 4],
        );
        draw_button(
            vertices,
            -0.52,
            -0.02,
            -0.69,
            -0.58,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                -0.02,
                -0.69,
                -0.58,
            ),
        );
        draw_button(
            vertices,
            0.02,
            0.52,
            -0.69,
            -0.58,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                0.02,
                0.52,
                -0.69,
                -0.58,
            ),
        );
        draw_centered_text_in(
            vertices,
            &format!(
                "BONUS CHEST: {}",
                if self.create_bonus_chest { "ON" } else { "OFF" }
            ),
            -0.52,
            -0.02,
            -0.657,
            0.0048,
            aspect,
            [1.0; 4],
        );
        draw_centered_text_in(
            vertices,
            &format!("CHEATS: {}", if self.create_cheats { "ON" } else { "OFF" }),
            0.02,
            0.52,
            -0.657,
            0.0048,
            aspect,
            [1.0; 4],
        );
        draw_button(
            vertices,
            -0.52,
            -0.02,
            -0.84,
            -0.71,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.52,
                -0.02,
                -0.84,
                -0.71,
            ),
        );
        draw_button(
            vertices,
            0.02,
            0.52,
            -0.84,
            -0.71,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                0.02,
                0.52,
                -0.84,
                -0.71,
            ),
        );
        draw_centered_text_in(
            vertices,
            "CREATE WORLD",
            -0.52,
            -0.02,
            -0.798,
            0.007,
            aspect,
            [1.0; 4],
        );
        draw_centered_text_in(
            vertices, "CANCEL", 0.02, 0.52, -0.798, 0.007, aspect, [1.0; 4],
        );
    }

    fn draw_options(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.9, 0.9, -0.88, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.options"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
        );
        let left = [
            format!("FOV: < {:.0} >", self.settings.fov),
            format!("RENDER DISTANCE: < {} >", self.settings.render_distance),
            format!("FULLSCREEN: < {} >", on_off(self.settings.fullscreen)),
            format!("VSYNC: < {} >", on_off(self.settings.vsync)),
            format!("DIFFICULTY: < {} >", self.settings.difficulty.as_str()),
            format!("FPS CAP: < {} >", fps_cap_label(self.settings.fps_cap)),
        ];
        let right = [
            format!(
                "MASTER VOLUME: < {}% >",
                percent(self.settings.master_volume)
            ),
            format!("MUSIC VOLUME: < {}% >", percent(self.settings.music_volume)),
            format!("SOUND VOLUME: < {}% >", percent(self.settings.sound_volume)),
            format!(
                "WEATHER VOLUME: < {}% >",
                percent(self.settings.weather_volume)
            ),
            format!("LANGUAGE: < {} >", self.settings.language.as_str()),
            self.tr("menu.accessibility"),
        ];
        for (row, label) in left.iter().enumerate() {
            let top = OPTIONS_ROW_TOPS[row];
            draw_button(
                vertices,
                -0.82,
                -0.05,
                top - 0.13,
                top,
                hit(
                    self.mouse_ndc[0],
                    self.mouse_ndc[1],
                    -0.82,
                    -0.05,
                    top - 0.13,
                    top,
                ),
            );
            draw_centered_text_in(
                vertices,
                label,
                -0.82,
                -0.05,
                top - 0.092,
                0.0058,
                aspect,
                [1.0; 4],
            );
        }
        for (row, label) in right.iter().enumerate() {
            let top = OPTIONS_ROW_TOPS[row];
            draw_button(
                vertices,
                0.05,
                0.82,
                top - 0.13,
                top,
                hit(
                    self.mouse_ndc[0],
                    self.mouse_ndc[1],
                    0.05,
                    0.82,
                    top - 0.13,
                    top,
                ),
            );
            draw_centered_text_in(
                vertices,
                label,
                0.05,
                0.82,
                top - 0.092,
                0.0058,
                aspect,
                [1.0; 4],
            );
        }
        for (x0, x1, label) in [
            (-0.82, -0.30, "RESOURCE PACKS"),
            (-0.25, 0.25, "CONTROLS"),
            (0.30, 0.82, "DONE"),
        ] {
            draw_button(
                vertices,
                x0,
                x1,
                -0.78,
                -0.64,
                hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, -0.78, -0.64),
            );
            draw_centered_text_in(vertices, label, x0, x1, -0.738, 0.006, aspect, [1.0; 4]);
        }
    }

    fn handle_accessibility_click(&mut self, x: f32, y: f32) {
        let column = if x < 0.0 { 0 } else { 1 };
        let row = ((0.56 - y) / 0.18).floor() as i32;
        if (0..5).contains(&row) && x.abs() <= 0.84 {
            let index = column * 5 + row as usize;
            let setting = crate::accessibility::AccessibilityRow::ALL[index];
            let delta = if x < -0.42 || (x > 0.05 && x < 0.42) {
                -1
            } else {
                1
            };
            match setting {
                crate::accessibility::AccessibilityRow::UiScale => {
                    self.settings.accessibility.cycle_ui_scale(delta)
                }
                crate::accessibility::AccessibilityRow::ChatScale => {
                    self.settings.accessibility.cycle_chat_scale(delta)
                }
                crate::accessibility::AccessibilityRow::ChatOpacity => {
                    self.settings.accessibility.cycle_chat_opacity(delta)
                }
                _ => self.settings.accessibility.toggle(setting),
            }
            self.settings.save();
        } else if hit(x, y, -0.25, 0.25, -0.78, -0.64) {
            self.screen = MenuScreen::Options;
            self.focus_index = 0;
        }
    }

    fn handle_resource_pack_click(&mut self, x: f32, y: f32) {
        let available = self.resource_packs.available();
        let row = ((0.56 - y) / 0.14).floor() as usize;
        if y <= 0.56 && y >= 0.56 - available.len() as f32 * 0.14 {
            if let Some(summary) = available.get(row) {
                let mut selected = self.resource_packs.enabled_order().to_vec();
                if let Some(position) = selected.iter().position(|id| id == &summary.manifest.id) {
                    selected.remove(position);
                } else {
                    selected.push(summary.manifest.id.clone());
                }
                if let Err(error) = self.resource_packs.apply_enabled_order(&selected) {
                    self.message = Some(format!("PACK REJECTED: {error}"));
                } else {
                    self.refresh_catalog();
                }
            }
        } else if hit(x, y, -0.78, -0.28, -0.78, -0.64) {
            self.settings.resource_packs = self.resource_packs.enabled_order().to_vec();
            self.settings.save();
            self.message = Some("PACKS APPLIED FOR NEXT WORLD".to_string());
        } else if hit(x, y, -0.22, 0.22, -0.78, -0.64) {
            if let Err(error) = self.resource_packs.reload() {
                self.message = Some(format!("PACK RELOAD FAILED: {error}"));
            } else if !self.settings.resource_packs.is_empty() {
                let _ = self
                    .resource_packs
                    .apply_enabled_order(&self.settings.resource_packs);
                self.refresh_catalog();
            } else {
                self.refresh_catalog();
            }
        } else if hit(x, y, 0.28, 0.78, -0.78, -0.64) {
            self.screen = MenuScreen::Options;
            self.focus_index = 0;
        }
    }

    fn draw_accessibility(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.90, 0.90, -0.88, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.accessibility"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
        );
        let rows = crate::accessibility::AccessibilityRow::ALL;
        for (index, setting) in rows.into_iter().enumerate() {
            let column = index / 5;
            let row = index % 5;
            let (x0, x1) = if column == 0 {
                (-0.82, -0.05)
            } else {
                (0.05, 0.82)
            };
            let top = 0.56 - row as f32 * 0.18;
            let value = match setting {
                crate::accessibility::AccessibilityRow::UiScale => {
                    format!("UI SCALE: < {:.2}x >", self.settings.accessibility.ui_scale)
                }
                crate::accessibility::AccessibilityRow::ChatScale => {
                    format!(
                        "CHAT SCALE: < {:.2}x >",
                        self.settings.accessibility.chat_scale
                    )
                }
                crate::accessibility::AccessibilityRow::ChatOpacity => format!(
                    "CHAT OPACITY: < {}% >",
                    percent(self.settings.accessibility.chat_opacity)
                ),
                _ => format!(
                    "{}: < {} >",
                    accessibility_label(&self.catalog, setting),
                    on_off(self.settings.accessibility.bool_value(setting))
                ),
            };
            draw_button(
                vertices,
                x0,
                x1,
                top - 0.13,
                top,
                hit(
                    self.mouse_ndc[0],
                    self.mouse_ndc[1],
                    x0,
                    x1,
                    top - 0.13,
                    top,
                ),
            );
            draw_centered_text_in(
                vertices,
                &value,
                x0,
                x1,
                top - 0.092,
                0.0055,
                aspect,
                [1.0; 4],
            );
        }
        draw_button(
            vertices,
            -0.25,
            0.25,
            -0.78,
            -0.64,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.25,
                0.25,
                -0.78,
                -0.64,
            ),
        );
        draw_centered_text(
            vertices,
            &self.tr("menu.done"),
            -0.738,
            0.008,
            aspect,
            [1.0; 4],
        );
    }

    fn draw_resource_packs(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.86, 0.86, -0.88, 0.82);
        draw_centered_text(
            vertices,
            &self.tr("menu.resource_packs"),
            0.72,
            0.012,
            aspect,
            [1.0; 4],
        );
        let available = self.resource_packs.available();
        if available.is_empty() {
            draw_centered_text(
                vertices,
                "NO USER PACKS FOUND",
                0.22,
                0.007,
                aspect,
                [0.8; 4],
            );
        }
        for (index, summary) in available.iter().enumerate() {
            let top = 0.56 - index as f32 * 0.14;
            let selected = summary.enabled;
            draw_button_state(
                vertices,
                -0.78,
                0.78,
                top - 0.11,
                top,
                hit(
                    self.mouse_ndc[0],
                    self.mouse_ndc[1],
                    -0.78,
                    0.78,
                    top - 0.11,
                    top,
                ),
                selected,
            );
            let marker = if selected { "[X]" } else { "[ ]" };
            let label = format!(
                "{marker} {}  {}",
                summary.manifest.name, summary.manifest.version
            );
            draw_text(
                vertices,
                &label,
                -0.72,
                top - 0.082,
                0.0058,
                aspect,
                [1.0; 4],
            );
        }
        for (x0, x1, label) in [
            (-0.78, -0.28, "APPLY"),
            (-0.22, 0.22, "RELOAD"),
            (0.28, 0.78, "BACK"),
        ] {
            draw_button(
                vertices,
                x0,
                x1,
                -0.78,
                -0.64,
                hit(self.mouse_ndc[0], self.mouse_ndc[1], x0, x1, -0.78, -0.64),
            );
            draw_centered_text_in(vertices, label, x0, x1, -0.738, 0.006, aspect, [1.0; 4]);
        }
        if !self.resource_packs.diagnostics().is_empty() {
            draw_text(
                vertices,
                "PACK DIAGNOSTICS AVAILABLE",
                -0.72,
                -0.58,
                0.0052,
                aspect,
                [1.0, 0.65, 0.25, 1.0],
            );
        }
    }

    fn draw_controls(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        panel(vertices, -0.86, 0.86, -0.88, 0.82);
        draw_centered_text(vertices, "CONTROLS", 0.72, 0.012, aspect, [1.0; 4]);
        draw_button(
            vertices,
            -0.48,
            0.48,
            0.49,
            0.62,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.48,
                0.48,
                0.49,
                0.62,
            ),
        );
        draw_centered_text(
            vertices,
            &format!(
                "MOUSE SENSITIVITY: < {:.1} >",
                self.settings.sensitivity * 1000.0
            ),
            0.528,
            0.0065,
            aspect,
            [1.0; 4],
        );
        let actions = [
            ControlAction::Forward,
            ControlAction::Backward,
            ControlAction::Left,
            ControlAction::Right,
            ControlAction::Jump,
            ControlAction::Sprint,
            ControlAction::Sneak,
            ControlAction::Inventory,
        ];
        for (index, action) in actions.into_iter().enumerate() {
            let column = index / 4;
            let row = index % 4;
            let (x0, x1) = if column == 0 {
                (-0.78, -0.04)
            } else {
                (0.04, 0.78)
            };
            let top = 0.38 - row as f32 * 0.19;
            let active = self.rebinding == Some(action);
            draw_button_state(
                vertices,
                x0,
                x1,
                top - 0.14,
                top,
                hit(
                    self.mouse_ndc[0],
                    self.mouse_ndc[1],
                    x0,
                    x1,
                    top - 0.14,
                    top,
                ),
                active,
            );
            let value = if active {
                "PRESS A KEY"
            } else {
                key_name(self.control(action))
            };
            draw_centered_text_in(
                vertices,
                &format!("{}: {}", action.label(), value),
                x0,
                x1,
                top - 0.098,
                0.0065,
                aspect,
                [1.0; 4],
            );
        }
        draw_button(
            vertices,
            -0.25,
            0.25,
            -0.78,
            -0.64,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.25,
                0.25,
                -0.78,
                -0.64,
            ),
        );
        draw_centered_text(vertices, "DONE", -0.738, 0.008, aspect, [1.0; 4]);
    }

    fn control(&self, action: ControlAction) -> KeyCode {
        match action {
            ControlAction::Forward => self.settings.controls.forward,
            ControlAction::Backward => self.settings.controls.backward,
            ControlAction::Left => self.settings.controls.left,
            ControlAction::Right => self.settings.controls.right,
            ControlAction::Jump => self.settings.controls.jump,
            ControlAction::Sprint => self.settings.controls.sprint,
            ControlAction::Sneak => self.settings.controls.sneak,
            ControlAction::Inventory => self.settings.controls.inventory,
        }
    }

    fn draw_delete_confirmation(&self, vertices: &mut Vec<UiVertex>, aspect: f32) {
        draw_rect(vertices, -1.0, 1.0, -1.0, 1.0, [0.0, 0.0, 0.0, 0.62]);
        panel(vertices, -0.58, 0.58, -0.32, 0.34);
        draw_centered_text(
            vertices,
            "DELETE THIS WORLD?",
            0.20,
            0.011,
            aspect,
            [1.0, 0.45, 0.35, 1.0],
        );
        draw_centered_text(
            vertices,
            "THIS CANNOT BE UNDONE",
            0.08,
            0.0065,
            aspect,
            [0.85, 0.85, 0.85, 1.0],
        );
        draw_button(
            vertices,
            -0.48,
            -0.02,
            -0.16,
            -0.02,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                -0.48,
                -0.02,
                -0.16,
                -0.02,
            ),
        );
        draw_button(
            vertices,
            0.02,
            0.48,
            -0.16,
            -0.02,
            hit(
                self.mouse_ndc[0],
                self.mouse_ndc[1],
                0.02,
                0.48,
                -0.16,
                -0.02,
            ),
        );
        draw_centered_text_in(
            vertices, "DELETE", -0.48, -0.02, -0.118, 0.007, aspect, [1.0; 4],
        );
        draw_centered_text_in(
            vertices, "CANCEL", 0.02, 0.48, -0.118, 0.007, aspect, [1.0; 4],
        );
    }
}

fn apply_fullscreen(window: &Window, enabled: bool) {
    window.set_fullscreen(if enabled {
        Some(Fullscreen::Borderless(window.current_monitor()))
    } else {
        None
    });
}

fn present_mode(vsync: bool, modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if vsync {
        wgpu::PresentMode::Fifo
    } else if modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else if modes.contains(&wgpu::PresentMode::Immediate) {
        wgpu::PresentMode::Immediate
    } else {
        wgpu::PresentMode::Fifo
    }
}

fn hash_seed(value: &str) -> u32 {
    value.bytes().fold(2_166_136_261, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(16_777_619)
    })
}

fn relative_time(timestamp: u64) -> String {
    let days = unix_now().saturating_sub(timestamp) / 86_400;
    match days {
        0 => "PLAYED TODAY".to_string(),
        1 => "PLAYED YESTERDAY".to_string(),
        days => format!("PLAYED {days} DAYS AGO"),
    }
}

fn percent(value: f32) -> u32 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u32
}

fn options_row_at(y: f32) -> Option<usize> {
    OPTIONS_ROW_TOPS
        .iter()
        .position(|top| y <= *top && y >= *top - 0.13)
}

fn on_off(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

fn accessibility_label(
    catalog: &TranslationCatalog,
    row: crate::accessibility::AccessibilityRow,
) -> String {
    let key = match row {
        crate::accessibility::AccessibilityRow::UiScale => "menu.ui_scale",
        crate::accessibility::AccessibilityRow::ChatScale => "menu.chat_scale",
        crate::accessibility::AccessibilityRow::ChatOpacity => "menu.chat_opacity",
        crate::accessibility::AccessibilityRow::Subtitles => "menu.subtitles",
        crate::accessibility::AccessibilityRow::HighContrast => "menu.high_contrast",
        crate::accessibility::AccessibilityRow::ReduceFlashing => "menu.reduce_flashing",
        crate::accessibility::AccessibilityRow::ToggleSprint => "menu.toggle_sprint",
        crate::accessibility::AccessibilityRow::ToggleSneak => "menu.toggle_sneak",
        crate::accessibility::AccessibilityRow::CameraBobbing => "menu.camera_bobbing",
        crate::accessibility::AccessibilityRow::DamageTilt => "menu.damage_tilt",
    };
    catalog.lookup(key)
}

fn hit(x: f32, y: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> bool {
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

fn draw_rect(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32, color: [f32; 4]) {
    for position in [[x0, y1], [x0, y0], [x1, y0], [x0, y1], [x1, y0], [x1, y1]] {
        vertices.push(UiVertex { position, color });
    }
}

fn draw_focus_ring(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32) {
    let color = [1.0, 0.78, 0.18, 1.0];
    let thickness = 0.008;
    draw_rect(
        vertices,
        x0 - thickness,
        x1 + thickness,
        y1,
        y1 + thickness,
        color,
    );
    draw_rect(
        vertices,
        x0 - thickness,
        x1 + thickness,
        y0 - thickness,
        y0,
        color,
    );
    draw_rect(vertices, x0 - thickness, x0, y0, y1, color);
    draw_rect(vertices, x1, x1 + thickness, y0, y1, color);
}

fn panel(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32) {
    draw_rect(vertices, x0, x1, y0, y1, [0.055, 0.06, 0.065, 0.93]);
    draw_rect(vertices, x0, x1, y1 - 0.012, y1, [0.42, 0.45, 0.46, 1.0]);
    draw_rect(vertices, x0, x1, y0, y0 + 0.012, [0.015, 0.018, 0.02, 1.0]);
}

fn draw_button(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32, hover: bool) {
    draw_button_state(vertices, x0, x1, y0, y1, hover, false);
}

fn draw_button_state(
    vertices: &mut Vec<UiVertex>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    hover: bool,
    selected: bool,
) {
    let fill = if selected {
        [0.23, 0.38, 0.18, 0.98]
    } else if hover {
        [0.30, 0.31, 0.32, 0.98]
    } else {
        [0.16, 0.17, 0.18, 0.98]
    };
    let light = if hover || selected {
        [0.92, 0.94, 0.90, 1.0]
    } else {
        [0.48, 0.50, 0.51, 1.0]
    };
    draw_rect(vertices, x0, x1, y0, y1, fill);
    draw_rect(vertices, x0, x1, y1 - 0.008, y1, light);
    draw_rect(vertices, x0, x0 + 0.006, y0, y1, light);
    draw_rect(vertices, x0, x1, y0, y0 + 0.008, [0.035, 0.04, 0.04, 1.0]);
    draw_rect(vertices, x1 - 0.006, x1, y0, y1, [0.035, 0.04, 0.04, 1.0]);
}

fn draw_field(
    vertices: &mut Vec<UiVertex>,
    label: &str,
    value: &str,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    active: bool,
    aspect: f32,
) {
    draw_text(
        vertices,
        label,
        x0,
        y1 + 0.035,
        0.006,
        aspect,
        [0.8, 0.82, 0.84, 1.0],
    );
    draw_button_state(vertices, x0, x1, y0, y1, false, active);
    draw_centered_text_in(vertices, value, x0, x1, y0 + 0.038, 0.008, aspect, [1.0; 4]);
}

fn draw_logo(vertices: &mut Vec<UiVertex>, aspect: f32) {
    draw_centered_text(
        vertices,
        "ICRAFT",
        0.505,
        0.026,
        aspect,
        [0.04, 0.045, 0.04, 1.0],
    );
    draw_centered_text(
        vertices,
        "ICRAFT",
        0.53,
        0.026,
        aspect,
        [0.72, 0.75, 0.70, 1.0],
    );
    draw_centered_text(
        vertices,
        "RUST EDITION",
        0.43,
        0.007,
        aspect,
        [1.0, 0.83, 0.18, 1.0],
    );
}

fn text_width(text: &str, pixel: f32, aspect: f32) -> f32 {
    let char_width = pixel * aspect * 6.0;
    text.chars().count() as f32 * char_width - pixel * aspect
}

fn draw_centered_text(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
) {
    let x = -text_width(text, pixel, aspect) * 0.5;
    draw_text(vertices, text, x, y, pixel, aspect, color);
}

fn draw_centered_text_in(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    x0: f32,
    x1: f32,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
) {
    let x = (x0 + x1 - text_width(text, pixel, aspect)) * 0.5;
    draw_text(vertices, text, x, y, pixel, aspect, color);
}

fn draw_text(
    vertices: &mut Vec<UiVertex>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    aspect: f32,
    color: [f32; 4],
) {
    let pixel_x = pixel * aspect;
    let mut cursor = x;
    for ch in text.to_ascii_uppercase().chars() {
        let rows = glyph(ch);
        for (row, bits) in rows.into_iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let px = cursor + column as f32 * pixel_x;
                    let py = y + (6 - row) as f32 * pixel;
                    draw_rect(
                        vertices,
                        px,
                        px + pixel_x * 0.88,
                        py,
                        py + pixel * 0.88,
                        color,
                    );
                }
            }
        }
        cursor += pixel_x * 6.0;
    }
}

fn glyph(ch: char) -> [u8; 7] {
    match ch {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 14],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '.' => [0, 0, 0, 0, 0, 4, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '%' => [17, 2, 4, 8, 17, 0, 0],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        _ => [0; 7],
    }
}

const UI_SHADER: &str = r#"
struct In { @location(0) position: vec2<f32>, @location(1) color: vec4<f32> };
struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs_main(input: In) -> Out {
    var out: Out;
    out.position = vec4<f32>(input.position, 0.0, 1.0);
    out.color = input.color;
    return out;
}
@fragment fn fs_main(input: Out) -> @location(0) vec4<f32> { return input.color; }
"#;

const PANORAMA_SHADER: &str = r#"
struct Panorama { time: f32, width: f32, height: f32, padding: f32 };
@group(0) @binding(0) var<uniform> panorama: Panorama;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}
fn hash(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453); }
@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let resolution = vec2<f32>(max(panorama.width, 1.0), max(panorama.height, 1.0));
    var uv = position.xy / resolution;
    let horizon = 0.58;
    let travel = panorama.time * 0.018;
    let sky = mix(vec3<f32>(0.07, 0.19, 0.34), vec3<f32>(0.45, 0.67, 0.78), clamp(uv.y / horizon, 0.0, 1.0));
    var color = sky;
    let sun = smoothstep(0.055, 0.045, distance(uv, vec2<f32>(0.78, 0.20)));
    color = mix(color, vec3<f32>(1.0, 0.82, 0.38), sun * 0.75);
    let far_h = 0.48 + floor((sin((uv.x + travel) * 18.0) * 0.035 + sin((uv.x + travel) * 7.0) * 0.05) * 40.0) / 40.0;
    if uv.y > far_h { color = vec3<f32>(0.18, 0.28, 0.25); }
    let near_h = 0.62 + floor((sin((uv.x + travel * 1.7) * 24.0) * 0.045 + sin((uv.x + travel) * 9.0) * 0.07) * 32.0) / 32.0;
    if uv.y > near_h { color = vec3<f32>(0.12, 0.26, 0.12); }
    if uv.y > near_h + 0.035 { color = vec3<f32>(0.25, 0.20, 0.12); }
    let cell = floor(vec2<f32>((uv.x + travel * 1.7) * 80.0, uv.y * 80.0));
    color *= 0.88 + hash(cell) * 0.18;
    let vignette = 1.0 - 0.50 * dot(uv - vec2<f32>(0.5), uv - vec2<f32>(0.5));
    return vec4<f32>(color * vignette, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_address_book_keeps_recent_ping_results() {
        let mut book = ServerAddressBook::new(2);
        book.remember("127.0.0.1:25565");
        book.record_ping(ServerPingResult {
            address: "example.test:25565".into(),
            version: "0.1.0".into(),
            motd: "Welcome".into(),
            online_players: 2,
            max_players: 20,
            error: None,
        });
        book.record_ping(ServerPingResult {
            address: "127.0.0.1:25565".into(),
            version: "0.1.0".into(),
            motd: "Local".into(),
            online_players: 1,
            max_players: 20,
            error: None,
        });
        assert_eq!(book.addresses().len(), 2);
        assert_eq!(book.addresses()[0], "127.0.0.1:25565");
        assert_eq!(book.recent_results()[0].motd, "Local");
    }

    #[test]
    fn sanitizes_world_names_and_generates_stable_slugs() {
        assert_eq!(sanitize_name("  My <World>!  "), "My World");
        assert_eq!(slugify("My World"), "my_world");
    }

    #[test]
    fn selected_world_directory_survives_list_reordering() {
        let metadata = |name: &str, last_played| WorldMetadata {
            name: name.to_string(),
            seed: 1,
            game_mode: GameMode::Creative,
            difficulty: Difficulty::Normal,
            last_played,
            world_type: WorldType::Default,
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: false,
            hardcore: false,
            version: CURRENT_WORLD_FORMAT_VERSION,
            needs_upgrade: false,
        };
        let first_dir = PathBuf::from("C:/saves/first");
        let second_dir = PathBuf::from("C:/saves/second");
        let mut worlds = vec![
            WorldEntry {
                directory: first_dir,
                metadata: metadata("SAME NAME", 2),
            },
            WorldEntry {
                directory: second_dir.clone(),
                metadata: metadata("SAME NAME", 1),
            },
        ];

        worlds.reverse();

        let index = world_index_by_directory(&worlds, &second_dir).unwrap();
        assert_eq!(worlds[index].directory, second_dir);
    }

    #[test]
    fn settings_key_names_round_trip() {
        for code in [
            KeyCode::KeyW,
            KeyCode::Space,
            KeyCode::ControlLeft,
            KeyCode::ArrowUp,
        ] {
            assert_eq!(parse_key(key_name(code)), Some(code));
        }
    }

    #[test]
    fn difficulty_steps_both_directions() {
        assert_eq!(Difficulty::Peaceful.step(-1), Difficulty::Hard);
        assert_eq!(Difficulty::Normal.step(1), Difficulty::Hard);
    }

    #[test]
    fn legacy_settings_without_weather_volume_use_reduced_default() {
        let settings = GameSettings::from_file_contents(
            "master_volume:0.8\nsound_volume:0.6\nmusic_volume:0.2\n",
        );

        assert!((settings.weather_volume - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn weather_volume_load_clamps_out_of_range_values() {
        let too_high = GameSettings::from_file_contents("weather_volume:4.5\n");
        let too_low = GameSettings::from_file_contents("weather_volume:-2\n");
        let not_finite = GameSettings::from_file_contents("weather_volume:NaN\n");

        assert_eq!(too_high.weather_volume, 1.0);
        assert_eq!(too_low.weather_volume, 0.0);
        assert_eq!(not_finite.weather_volume, 0.4);
    }

    #[test]
    fn settings_file_round_trip_includes_weather_volume() {
        let mut original = GameSettings::default();
        original.weather_volume = 0.3;

        let contents = original.to_file_contents();
        let loaded = GameSettings::from_file_contents(&contents);

        assert!(contents.contains("weather_volume:0.3\n"));
        assert!((loaded.weather_volume - original.weather_volume).abs() < f32::EPSILON);
    }

    #[test]
    fn non_finite_view_settings_fall_back_during_load() {
        for value in ["NaN", "inf", "-inf"] {
            let settings =
                GameSettings::from_file_contents(&format!("fov:{value}\nsensitivity:{value}\n"));

            assert_eq!(settings.fov, 70.0, "fov should reject {value}");
            assert_eq!(
                settings.sensitivity, 0.002,
                "sensitivity should reject {value}"
            );
        }
    }

    #[test]
    fn non_finite_view_settings_are_sanitized_before_save() {
        let mut settings = GameSettings::default();
        settings.fov = f32::NAN;
        settings.sensitivity = f32::INFINITY;

        let contents = settings.to_file_contents();
        let loaded = GameSettings::from_file_contents(&contents);

        assert!(contents.contains("fov:70\n"));
        assert!(contents.contains("sensitivity:0.002\n"));
        assert!(!contents.contains("NaN"));
        assert!(!contents.contains("inf"));
        assert_eq!(loaded.fov, 70.0);
        assert_eq!(loaded.sensitivity, 0.002);
    }

    #[test]
    fn view_setting_boundaries_round_trip_without_drift() {
        for (fov, sensitivity) in [(30.0, 0.0002), (120.0, 0.006)] {
            let mut settings = GameSettings::default();
            settings.fov = fov;
            settings.sensitivity = sensitivity;

            let loaded = GameSettings::from_file_contents(&settings.to_file_contents());

            assert_eq!(loaded.fov, fov);
            assert_eq!(loaded.sensitivity, sensitivity);
        }
    }

    #[test]
    fn weather_options_row_is_distinct_from_language_controls_and_back() {
        assert_eq!(options_row_at(-0.08), Some(3));
        assert_eq!(options_row_at(-0.28), Some(4));
        assert_eq!(options_row_at(-0.48), Some(5));
        assert_eq!(options_row_at(-0.70), None);
        assert!(hit(0.4, -0.08, 0.05, 0.82, -0.15, -0.02));
    }

    #[test]
    fn multiplayer_settings_defaults_and_mutation() {
        let mut settings = GameSettings::default();
        assert_eq!(settings.mp_host_port, "25565");
        assert_eq!(settings.mp_server_address, "127.0.0.1");
        assert_eq!(settings.mp_join_port, "25565");
        assert_eq!(settings.mp_username, "PLAYER");

        settings.mp_host_port = "25570".to_string();
        settings.mp_server_address = "192.168.1.100".to_string();
        settings.mp_join_port = "25571".to_string();
        settings.mp_username = "TEST_USER".to_string();

        assert_eq!(settings.mp_host_port, "25570");
        assert_eq!(settings.mp_server_address, "192.168.1.100");
        assert_eq!(settings.mp_join_port, "25571");
        assert_eq!(settings.mp_username, "TEST_USER");
    }

    #[test]
    fn leaving_controls_clears_pending_rebind() {
        let (screen, active_field, rebinding) = back_transition(
            MenuScreen::Controls,
            Some(TextField::WorldName),
            Some(ControlAction::Forward),
        );

        assert_eq!(screen, MenuScreen::Options);
        assert_eq!(active_field, None);
        assert_eq!(rebinding, None);
    }

    #[test]
    fn world_path_guard_rejects_saves_root() {
        assert!(validated_world_path(Path::new(SAVES_DIR)).is_err());
    }

    #[test]
    fn client_world_launch_uses_temp_dir_and_placeholder_seed() {
        let launch = WorldLaunch {
            world_dir: std::env::temp_dir().join("icraft_multiplayer_client"),
            seed: 0,
            game_mode: GameMode::Survival,
            difficulty: Difficulty::Normal,
            role: MultiplayerRole::Client {
                server_addr: "127.0.0.1".to_string(),
                port: 25565,
                username: "PLAYER".to_string(),
            },
        };
        assert!(launch.world_dir.starts_with(std::env::temp_dir()));
        assert!(!launch.world_dir.starts_with("saves"));
        assert_eq!(launch.seed, 0);
        assert!(matches!(launch.game_mode, GameMode::Survival));
        assert!(matches!(launch.role, MultiplayerRole::Client { .. }));
    }

    #[test]
    fn controls_config_parse_and_round_trip() {
        let mut settings = GameSettings::default();
        let config_text = r#"
# Custom Controls Config Test
key_forward = UP
key_backward = DOWN
key_left = LEFT
key_right = RIGHT
key_jump = SPACE
key_sprint = LCTRL
key_sneak = LSHIFT
key_inventory = E
key_chat = Y
key_advancements = K
key_debug = F1
key_perspective = F2
key_gamemode = M
key_pause = ESC
"#;
        settings.apply_file_contents(config_text);
        assert_eq!(settings.controls.forward, KeyCode::ArrowUp);
        assert_eq!(settings.controls.backward, KeyCode::ArrowDown);
        assert_eq!(settings.controls.left, KeyCode::ArrowLeft);
        assert_eq!(settings.controls.right, KeyCode::ArrowRight);
        assert_eq!(settings.controls.chat, KeyCode::KeyY);
        assert_eq!(settings.controls.advancements, KeyCode::KeyK);
        assert_eq!(settings.controls.debug, KeyCode::F1);
        assert_eq!(settings.controls.perspective, KeyCode::F2);
        assert_eq!(settings.controls.gamemode, KeyCode::KeyM);
        assert_eq!(settings.controls.pause, KeyCode::Escape);

        let exported = settings.to_controls_file_contents();
        assert!(exported.contains("key_forward = UP"));
        assert!(exported.contains("key_backward = DOWN"));
        assert!(exported.contains("key_chat = Y"));
        assert!(exported.contains("key_advancements = K"));
        assert!(exported.contains("key_debug = F1"));
    }

    #[test]
    fn fps_cap_round_trip_and_sanitization() {
        let settings = GameSettings::from_file_contents("fps_cap:144");
        assert_eq!(settings.fps_cap, 144);
        assert!(GameSettings::from_file_contents("fps_cap:0").fps_cap == 0);
        assert_eq!(GameSettings::from_file_contents("fps_cap:999").fps_cap, 240);
        assert!(GameSettings::from_file_contents("fps_cap:-1").fps_cap == 0);
        let mut settings = GameSettings::default();
        settings.fps_cap = 60;
        assert!(settings.to_file_contents().contains("fps_cap:60"));
    }

    #[test]
    fn fps_cap_cycles_through_uncapped_and_standard_rates() {
        assert_eq!(cycle_fps_cap(0, 1), 30);
        assert_eq!(cycle_fps_cap(30, 1), 60);
        assert_eq!(cycle_fps_cap(60, 1), 144);
        assert_eq!(cycle_fps_cap(144, 1), 0);
        assert_eq!(fps_cap_label(0), "UNCAPPED");
    }
}

//! Accessibility preferences and presentation-only helpers.
//!
//! These values are intentionally independent from gameplay state.  They can
//! change while a world is running without changing tick timing, authority,
//! damage, or network snapshots.

use std::collections::VecDeque;

pub const SUBTITLE_QUEUE_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilitySettings {
    pub ui_scale: f32,
    pub chat_scale: f32,
    pub chat_opacity: f32,
    pub subtitles: bool,
    pub high_contrast: bool,
    pub reduce_flashing: bool,
    pub toggle_sprint: bool,
    pub toggle_sneak: bool,
    pub camera_bobbing: bool,
    pub damage_tilt: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            chat_scale: 1.0,
            chat_opacity: 0.72,
            subtitles: false,
            high_contrast: false,
            reduce_flashing: false,
            toggle_sprint: false,
            toggle_sneak: false,
            camera_bobbing: true,
            damage_tilt: true,
        }
    }
}

impl AccessibilitySettings {
    pub fn sanitize(&mut self) {
        self.ui_scale = finite_clamp(self.ui_scale, 1.0, 0.75, 2.0);
        self.chat_scale = finite_clamp(self.chat_scale, 1.0, 0.5, 2.0);
        self.chat_opacity = finite_clamp(self.chat_opacity, 0.72, 0.0, 1.0);
    }

    pub fn cycle_ui_scale(&mut self, delta: i32) {
        self.ui_scale = cycle(self.ui_scale, delta, &[0.75, 1.0, 1.25, 1.5, 1.75, 2.0]);
    }

    pub fn cycle_chat_scale(&mut self, delta: i32) {
        self.chat_scale = cycle(
            self.chat_scale,
            delta,
            &[0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0],
        );
    }

    pub fn cycle_chat_opacity(&mut self, delta: i32) {
        self.chat_opacity = cycle(self.chat_opacity, delta, &[0.25, 0.5, 0.72, 0.85, 1.0]);
    }

    pub fn bool_value(&self, row: AccessibilityRow) -> bool {
        match row {
            AccessibilityRow::Subtitles => self.subtitles,
            AccessibilityRow::HighContrast => self.high_contrast,
            AccessibilityRow::ReduceFlashing => self.reduce_flashing,
            AccessibilityRow::ToggleSprint => self.toggle_sprint,
            AccessibilityRow::ToggleSneak => self.toggle_sneak,
            AccessibilityRow::CameraBobbing => self.camera_bobbing,
            AccessibilityRow::DamageTilt => self.damage_tilt,
            AccessibilityRow::UiScale
            | AccessibilityRow::ChatScale
            | AccessibilityRow::ChatOpacity => false,
        }
    }

    pub fn set_bool(&mut self, row: AccessibilityRow, value: bool) {
        match row {
            AccessibilityRow::Subtitles => self.subtitles = value,
            AccessibilityRow::HighContrast => self.high_contrast = value,
            AccessibilityRow::ReduceFlashing => self.reduce_flashing = value,
            AccessibilityRow::ToggleSprint => self.toggle_sprint = value,
            AccessibilityRow::ToggleSneak => self.toggle_sneak = value,
            AccessibilityRow::CameraBobbing => self.camera_bobbing = value,
            AccessibilityRow::DamageTilt => self.damage_tilt = value,
            AccessibilityRow::UiScale
            | AccessibilityRow::ChatScale
            | AccessibilityRow::ChatOpacity => {}
        }
    }

    pub fn toggle(&mut self, row: AccessibilityRow) {
        let value = !self.bool_value(row);
        self.set_bool(row, value);
    }
}

fn finite_clamp(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn cycle(value: f32, delta: i32, values: &[f32]) -> f32 {
    let index = values
        .iter()
        .position(|candidate| (*candidate - value).abs() < f32::EPSILON)
        .unwrap_or(0) as i32;
    values[(index + delta).rem_euclid(values.len() as i32) as usize]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityRow {
    UiScale,
    ChatScale,
    ChatOpacity,
    Subtitles,
    HighContrast,
    ReduceFlashing,
    ToggleSprint,
    ToggleSneak,
    CameraBobbing,
    DamageTilt,
}

impl AccessibilityRow {
    pub const ALL: [Self; 10] = [
        Self::UiScale,
        Self::ChatScale,
        Self::ChatOpacity,
        Self::Subtitles,
        Self::HighContrast,
        Self::ReduceFlashing,
        Self::ToggleSprint,
        Self::ToggleSneak,
        Self::CameraBobbing,
        Self::DamageTilt,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusNavigator {
    count: usize,
    index: usize,
}

impl FocusNavigator {
    pub fn new(count: usize) -> Self {
        Self { count, index: 0 }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn move_by(&mut self, direction: FocusDirection) {
        if self.count == 0 {
            self.index = 0;
            return;
        }
        let delta = match direction {
            FocusDirection::Forward => 1,
            FocusDirection::Backward => -1,
        };
        self.index = (self.index as i32 + delta).rem_euclid(self.count as i32) as usize;
    }

    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        self.index = self.index.min(count.saturating_sub(1));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleDirection {
    Center,
    Left,
    Right,
    Front,
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleEvent {
    pub key: &'static str,
    pub direction: SubtitleDirection,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SubtitleQueue {
    events: VecDeque<SubtitleEvent>,
    capacity: usize,
}

impl Default for SubtitleQueue {
    fn default() -> Self {
        Self::new(SUBTITLE_QUEUE_CAPACITY)
    }
}

impl SubtitleQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, event: SubtitleEvent) {
        if self
            .events
            .back()
            .is_some_and(|last| last.key == event.key && last.direction == event.direction)
        {
            return;
        }
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn drain_expired(&mut self, now_ms: u64) -> Vec<SubtitleEvent> {
        while self
            .events
            .front()
            .is_some_and(|event| event.expires_at_ms <= now_ms)
        {
            self.events.pop_front();
        }
        self.events.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

pub fn reduced_flash_alpha(base: f32, reduce_flashing: bool) -> f32 {
    let base = if base.is_finite() { base.max(0.0) } else { 0.0 };
    if reduce_flashing {
        (base * 0.18).min(0.18)
    } else {
        base.min(1.0)
    }
}

pub fn damage_overlay_alpha(base: f32, damage_tilt: bool, reduce_flashing: bool) -> f32 {
    if !damage_tilt {
        return 0.0;
    }
    reduced_flash_alpha(base, reduce_flashing)
}

pub fn direction_from_vector(x: f32, z: f32, right_x: f32, right_z: f32) -> SubtitleDirection {
    let horizontal = (x * x + z * z).sqrt();
    if !horizontal.is_finite() || horizontal < 0.001 {
        return SubtitleDirection::Center;
    }
    let side = x * right_x + z * right_z;
    if side.abs() > horizontal * 0.35 {
        if side < 0.0 {
            SubtitleDirection::Left
        } else {
            SubtitleDirection::Right
        }
    } else if z < 0.0 {
        SubtitleDirection::Front
    } else {
        SubtitleDirection::Back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_sanitize_and_cycle_stay_bounded() {
        let mut settings = AccessibilitySettings {
            ui_scale: f32::NAN,
            chat_scale: 9.0,
            chat_opacity: -2.0,
            ..Default::default()
        };
        settings.sanitize();
        assert_eq!(settings.ui_scale, 1.0);
        assert_eq!(settings.chat_scale, 2.0);
        assert_eq!(settings.chat_opacity, 0.0);
        settings.cycle_ui_scale(1);
        assert_eq!(settings.ui_scale, 1.25);
    }

    #[test]
    fn focus_navigation_wraps_in_stable_order() {
        let mut focus = FocusNavigator::new(3);
        focus.move_by(FocusDirection::Backward);
        assert_eq!(focus.index(), 2);
        focus.move_by(FocusDirection::Forward);
        assert_eq!(focus.index(), 0);
        focus.set_count(1);
        assert_eq!(focus.index(), 0);
    }

    #[test]
    fn subtitle_queue_is_bounded_and_deduplicated() {
        let mut queue = SubtitleQueue::new(2);
        queue.push(SubtitleEvent {
            key: "sound.jump",
            direction: SubtitleDirection::Center,
            expires_at_ms: 10,
        });
        queue.push(SubtitleEvent {
            key: "sound.jump",
            direction: SubtitleDirection::Center,
            expires_at_ms: 11,
        });
        queue.push(SubtitleEvent {
            key: "sound.hurt",
            direction: SubtitleDirection::Left,
            expires_at_ms: 20,
        });
        queue.push(SubtitleEvent {
            key: "sound.death",
            direction: SubtitleDirection::Right,
            expires_at_ms: 30,
        });
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.drain_expired(0).len(), 2);
        assert_eq!(queue.drain_expired(21).len(), 1);
    }

    #[test]
    fn reduced_motion_does_not_change_authority_values() {
        assert_eq!(reduced_flash_alpha(0.8, false), 0.8);
        assert!(reduced_flash_alpha(0.8, true) < 0.2);
        assert_eq!(damage_overlay_alpha(0.8, false, false), 0.0);
        assert_eq!(
            direction_from_vector(-1.0, 0.0, 1.0, 0.0),
            SubtitleDirection::Left
        );
    }
}

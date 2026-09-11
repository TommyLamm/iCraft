//! Presentation weather driven by `TimeSync.weather` (wire u8).
//!
//! There is no second climate / RNG authority on the GPU thread. Live hosts
//! currently always send Clear (`0`); particles and HUD react to that enum only.
//! `GameRules.do_weather_cycle` remains a saved/synced field for a future
//! authority weather owner.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weather {
    Clear,
    Rain,
    Thunder,
}

impl Weather {
    pub fn wire_value(self) -> u8 {
        match self {
            Weather::Clear => 0,
            Weather::Rain => 1,
            Weather::Thunder => 2,
        }
    }

    pub fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Weather::Clear),
            1 => Some(Weather::Rain),
            2 => Some(Weather::Thunder),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

/// GPU-thread weather presentation. Phase comes only from TimeSync / connect reset.
pub struct WeatherPresentation {
    pub current: Weather,
    flash_timer: f32,
    precipitation_accumulator: f32,
    presentation_rng: u32,
}

impl WeatherPresentation {
    pub fn new(seed: u32) -> Self {
        Self {
            current: Weather::Clear,
            flash_timer: 0.0,
            precipitation_accumulator: 0.0,
            presentation_rng: seed ^ 0x5A5A_E1C2,
        }
    }

    /// Apply host TimeSync weather. Ignores `remaining_ticks` (no local cycle).
    pub fn apply_wire(&mut self, weather: u8) -> bool {
        let Some(current) = Weather::from_wire(weather) else {
            return false;
        };
        if self.current != current {
            self.precipitation_accumulator = 0.0;
        }
        self.current = current;
        true
    }

    pub fn tick_presentation(&mut self, dt: f32) {
        self.flash_timer = (self.flash_timer - dt.max(0.0)).max(0.0);
    }

    pub fn trigger_lightning_flash(&mut self) {
        self.flash_timer = 0.32;
    }

    pub fn sky_brightness(&self) -> f32 {
        match self.current {
            Weather::Clear => 1.0,
            Weather::Rain => 0.62,
            Weather::Thunder => 0.38,
        }
    }

    pub fn flash_intensity(&self) -> f32 {
        (self.flash_timer / 0.32).clamp(0.0, 1.0)
    }

    /// Without a presentation climate authority, non-clear weather rains everywhere.
    pub fn precipitation_at(&self, _world_x: i32, _world_z: i32) -> Precipitation {
        match self.current {
            Weather::Clear => Precipitation::None,
            Weather::Rain | Weather::Thunder => Precipitation::Rain,
        }
    }

    pub fn take_precipitation_spawn_count(&mut self, dt: f32) -> usize {
        let rate = match self.current {
            Weather::Clear => 0.0,
            Weather::Rain => 150.0,
            Weather::Thunder => 220.0,
        };
        self.precipitation_accumulator += dt.max(0.0) * rate;
        let count = self.precipitation_accumulator.floor() as usize;
        self.precipitation_accumulator -= count as f32;
        count.min(64)
    }

    pub fn presentation_random_unit(&mut self) -> f32 {
        random_unit(&mut self.presentation_rng)
    }

    pub fn presentation_random_offset(&mut self, radius: i32) -> i32 {
        let width = (radius * 2 + 1).max(1) as u32;
        let roll = next_random(&mut self.presentation_rng) % width;
        roll as i32 - radius
    }
}

fn next_random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    *state
}

fn random_unit(state: &mut u32) -> f32 {
    next_random(state) as f32 / u32::MAX as f32
}

pub fn seeded_visual_unit(seed: &mut u32) -> f32 {
    random_unit(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_roundtrip_covers_known_values() {
        for weather in [Weather::Clear, Weather::Rain, Weather::Thunder] {
            assert_eq!(Weather::from_wire(weather.wire_value()), Some(weather));
        }
        assert_eq!(Weather::from_wire(9), None);
    }

    #[test]
    fn time_sync_drives_phase_without_local_cycle() {
        let mut weather = WeatherPresentation::new(23);
        assert!(weather.apply_wire(2));
        assert_eq!(weather.current, Weather::Thunder);
        weather.tick_presentation(10_000.0);
        assert_eq!(weather.current, Weather::Thunder);
        assert_eq!(weather.flash_intensity(), 0.0);
    }

    #[test]
    fn invalid_wire_is_rejected() {
        let mut weather = WeatherPresentation::new(29);
        assert!(!weather.apply_wire(99));
        assert_eq!(weather.current, Weather::Clear);
    }

    #[test]
    fn clear_weather_spawns_no_precipitation() {
        let mut weather = WeatherPresentation::new(31);
        assert_eq!(weather.take_precipitation_spawn_count(1.0), 0);
        assert_eq!(weather.precipitation_at(0, 0), Precipitation::None);
    }

    #[test]
    fn rain_weather_spawns_particles() {
        let mut weather = WeatherPresentation::new(31);
        assert!(weather.apply_wire(1));
        assert!(weather.take_precipitation_spawn_count(1.0) > 0);
        assert_eq!(weather.precipitation_at(0, 0), Precipitation::Rain);
    }

    #[test]
    fn lightning_flash_decays() {
        let mut weather = WeatherPresentation::new(17);
        weather.trigger_lightning_flash();
        assert!(weather.flash_intensity() > 0.9);
        weather.tick_presentation(0.4);
        assert_eq!(weather.flash_intensity(), 0.0);
    }

    #[test]
    fn seeded_visuals_are_deterministic() {
        let mut left = 7u32;
        let mut right = 7u32;
        for _ in 0..8 {
            assert_eq!(seeded_visual_unit(&mut left), seeded_visual_unit(&mut right));
        }
    }
}

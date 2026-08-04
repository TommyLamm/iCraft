use noise::Perlin;

use crate::world::Biome;

pub const TICKS_PER_DAY: f32 = 24_000.0;
const MIN_WEATHER_TICKS: f32 = TICKS_PER_DAY * 0.5;
const MAX_WEATHER_TICKS: f32 = TICKS_PER_DAY;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherSnapshot {
    pub current: Weather,
    pub remaining_ticks: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeatherUpdate {
    pub changed: bool,
    pub lightning_due: bool,
}

/// Pure weather timing and deterministic random state. Rendering, audio, and
/// world mutations remain owned by `State`.
pub struct WeatherSystem {
    pub current: Weather,
    remaining_ticks: f32,
    lightning_timer: f32,
    flash_timer: f32,
    precipitation_accumulator: f32,
    snow_accumulation_timer: f32,
    authority_rng: u32,
    presentation_rng: u32,
    temp_perlin: Perlin,
    moist_perlin: Perlin,
    ocean_perlin: Perlin,
}

impl WeatherSystem {
    pub fn new(seed: u32) -> Self {
        let mut system = Self {
            current: Weather::Clear,
            remaining_ticks: 0.0,
            lightning_timer: f32::INFINITY,
            flash_timer: 0.0,
            precipitation_accumulator: 0.0,
            snow_accumulation_timer: 0.0,
            authority_rng: seed ^ 0xA5A5_1F3D,
            presentation_rng: seed ^ 0x5A5A_E1C2,
            temp_perlin: Perlin::new(99_999),
            moist_perlin: Perlin::new(88_888),
            ocean_perlin: Perlin::new(77_777),
        };
        system.remaining_ticks = system.random_duration_ticks();
        system
    }

    pub fn clear_weather(&mut self) {
        self.current = Weather::Clear;
        self.remaining_ticks = self.random_duration_ticks();
        self.lightning_timer = f32::INFINITY;
    }

    pub fn is_thundering(&self) -> bool {
        self.current == Weather::Thunder
    }

    pub fn snapshot(&self) -> WeatherSnapshot {
        WeatherSnapshot {
            current: self.current,
            remaining_ticks: self.remaining_ticks.max(0.0),
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: WeatherSnapshot) -> bool {
        if !snapshot.remaining_ticks.is_finite() || snapshot.remaining_ticks < 0.0 {
            return false;
        }
        if self.current != snapshot.current {
            self.precipitation_accumulator = 0.0;
            self.snow_accumulation_timer = 0.0;
        }
        self.current = snapshot.current;
        self.remaining_ticks = snapshot.remaining_ticks;
        // Lightning is an explicit authoritative network event. A client that
        // applies a snapshot must never schedule its own strike.
        self.lightning_timer = f32::INFINITY;
        true
    }

    pub fn update_authoritative(&mut self, elapsed_world_ticks: f32, dt: f32) -> WeatherUpdate {
        let mut update = WeatherUpdate::default();
        self.remaining_ticks -= elapsed_world_ticks.max(0.0);
        while self.remaining_ticks <= 0.0 {
            self.current = match self.current {
                Weather::Clear => Weather::Rain,
                Weather::Rain => Weather::Thunder,
                Weather::Thunder => Weather::Clear,
            };
            self.remaining_ticks += self.random_duration_ticks();
            update.changed = true;
            self.lightning_timer = if self.current == Weather::Thunder {
                self.random_lightning_interval()
            } else {
                f32::INFINITY
            };
        }

        self.update_presentation(dt);
        if self.current == Weather::Thunder {
            self.lightning_timer -= dt.max(0.0);
            if self.lightning_timer <= 0.0 {
                update.lightning_due = true;
                self.lightning_timer = self.random_lightning_interval();
            }
        }
        update
    }

    pub fn update_client(&mut self, elapsed_world_ticks: f32, dt: f32) {
        // Locally project the countdown for smooth diagnostics, but wait for
        // the next host snapshot before changing phase.
        self.remaining_ticks = (self.remaining_ticks - elapsed_world_ticks.max(0.0)).max(0.0);
        self.update_presentation(dt);
    }

    pub fn trigger_lightning_flash(&mut self) {
        self.flash_timer = 0.32;
    }

    fn update_presentation(&mut self, dt: f32) {
        self.flash_timer = (self.flash_timer - dt.max(0.0)).max(0.0);
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

    pub fn biome_at(&self, world_x: i32, world_z: i32) -> Biome {
        Biome::get_biome(
            world_x,
            world_z,
            &self.temp_perlin,
            &self.moist_perlin,
            &self.ocean_perlin,
        )
    }

    pub fn precipitation_at(&self, world_x: i32, world_z: i32) -> Precipitation {
        if self.current == Weather::Clear {
            return Precipitation::None;
        }
        precipitation_for_biome(self.biome_at(world_x, world_z))
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

    pub fn take_snow_accumulation_steps(&mut self, dt: f32) -> usize {
        if self.current == Weather::Clear {
            self.snow_accumulation_timer = 0.0;
            return 0;
        }
        self.snow_accumulation_timer += dt.max(0.0);
        let steps = (self.snow_accumulation_timer / 0.75).floor() as usize;
        self.snow_accumulation_timer -= steps as f32 * 0.75;
        steps.min(2)
    }

    pub fn presentation_random_unit(&mut self) -> f32 {
        random_unit(&mut self.presentation_rng)
    }

    pub fn presentation_random_offset(&mut self, radius: i32) -> i32 {
        random_offset(&mut self.presentation_rng, radius)
    }

    pub fn authority_random_offset(&mut self, radius: i32) -> i32 {
        random_offset(&mut self.authority_rng, radius)
    }

    pub fn authority_random_seed(&mut self) -> u32 {
        next_random(&mut self.authority_rng)
    }

    fn random_duration_ticks(&mut self) -> f32 {
        MIN_WEATHER_TICKS
            + random_unit(&mut self.authority_rng) * (MAX_WEATHER_TICKS - MIN_WEATHER_TICKS)
    }

    fn random_lightning_interval(&mut self) -> f32 {
        4.0 + random_unit(&mut self.authority_rng) * 5.0
    }
}

fn next_random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    *state
}

pub fn seeded_visual_unit(seed: &mut u32) -> f32 {
    random_unit(seed)
}

fn random_unit(state: &mut u32) -> f32 {
    next_random(state) as f32 / u32::MAX as f32
}

fn random_offset(state: &mut u32, radius: i32) -> i32 {
    let width = (radius * 2 + 1).max(1) as u32;
    (random_unit(state) * width as f32).floor() as i32 - radius
}

fn precipitation_for_biome(biome: Biome) -> Precipitation {
    match biome {
        Biome::Desert => Precipitation::None,
        Biome::Taiga | Biome::Mountains => Precipitation::Snow,
        Biome::Plains | Biome::Forest | Biome::Swamp | Biome::Ocean => Precipitation::Rain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_cycles_in_the_required_order() {
        let mut weather = WeatherSystem::new(7);
        weather.remaining_ticks = 1.0;
        assert!(weather.update_authoritative(2.0, 0.0).changed);
        assert_eq!(weather.current, Weather::Rain);
        weather.remaining_ticks = 1.0;
        weather.update_authoritative(2.0, 0.0);
        assert_eq!(weather.current, Weather::Thunder);
        weather.remaining_ticks = 1.0;
        weather.update_authoritative(2.0, 0.0);
        assert_eq!(weather.current, Weather::Clear);
    }

    #[test]
    fn random_durations_stay_between_half_and_one_day() {
        let mut weather = WeatherSystem::new(11);
        for _ in 0..128 {
            let duration = weather.random_duration_ticks();
            assert!((MIN_WEATHER_TICKS..=MAX_WEATHER_TICKS).contains(&duration));
        }
    }

    #[test]
    fn desert_is_dry_and_cold_biomes_snow() {
        assert_eq!(precipitation_for_biome(Biome::Desert), Precipitation::None);
        assert_eq!(precipitation_for_biome(Biome::Taiga), Precipitation::Snow);
        assert_eq!(
            precipitation_for_biome(Biome::Mountains),
            Precipitation::Snow
        );
        assert_eq!(precipitation_for_biome(Biome::Forest), Precipitation::Rain);
    }

    #[test]
    fn thunder_schedules_a_flash_and_strike() {
        let mut weather = WeatherSystem::new(17);
        weather.current = Weather::Thunder;
        weather.remaining_ticks = TICKS_PER_DAY;
        weather.lightning_timer = 0.01;
        let update = weather.update_authoritative(0.0, 0.02);
        assert!(update.lightning_due);
        assert_eq!(weather.flash_intensity(), 0.0);
        weather.trigger_lightning_flash();
        assert!(weather.flash_intensity() > 0.9);
    }

    #[test]
    fn client_snapshot_never_advances_phase_or_schedules_lightning() {
        let mut weather = WeatherSystem::new(23);
        assert!(weather.apply_snapshot(WeatherSnapshot {
            current: Weather::Thunder,
            remaining_ticks: 1.0,
        }));
        let authority_rng = weather.authority_rng;

        weather.update_client(10_000.0, 10_000.0);

        assert_eq!(weather.current, Weather::Thunder);
        assert_eq!(weather.remaining_ticks, 0.0);
        assert_eq!(weather.lightning_timer, f32::INFINITY);
        assert_eq!(weather.authority_rng, authority_rng);
        assert_eq!(weather.flash_intensity(), 0.0);
    }

    #[test]
    fn invalid_snapshot_is_rejected_without_mutating_client_weather() {
        let mut weather = WeatherSystem::new(29);
        let before = weather.snapshot();

        assert!(!weather.apply_snapshot(WeatherSnapshot {
            current: Weather::Rain,
            remaining_ticks: f32::NAN,
        }));
        assert_eq!(weather.snapshot(), before);
    }

    #[test]
    fn presentation_randomness_does_not_change_authority_sequence() {
        let mut with_particles = WeatherSystem::new(31);
        let mut without_particles = WeatherSystem::new(31);
        for _ in 0..128 {
            with_particles.presentation_random_unit();
            with_particles.presentation_random_offset(14);
        }

        assert_eq!(
            with_particles.authority_random_seed(),
            without_particles.authority_random_seed()
        );
    }

    #[test]
    fn wire_values_and_seeded_visuals_are_deterministic() {
        for weather in [Weather::Clear, Weather::Rain, Weather::Thunder] {
            assert_eq!(Weather::from_wire(weather.wire_value()), Some(weather));
        }
        assert_eq!(Weather::from_wire(3), None);

        let mut left = 0xCAFE_BABE;
        let mut right = 0xCAFE_BABE;
        for _ in 0..24 {
            assert_eq!(
                seeded_visual_unit(&mut left),
                seeded_visual_unit(&mut right)
            );
        }
    }
}

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
    rng: u32,
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
            rng: seed ^ 0xA5A5_1F3D,
            temp_perlin: Perlin::new(99_999),
            moist_perlin: Perlin::new(88_888),
            ocean_perlin: Perlin::new(77_777),
        };
        system.remaining_ticks = system.random_duration_ticks();
        system
    }

    pub fn update(&mut self, elapsed_world_ticks: f32, dt: f32) -> WeatherUpdate {
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

        self.flash_timer = (self.flash_timer - dt.max(0.0)).max(0.0);
        if self.current == Weather::Thunder {
            self.lightning_timer -= dt.max(0.0);
            if self.lightning_timer <= 0.0 {
                update.lightning_due = true;
                self.flash_timer = 0.32;
                self.lightning_timer = self.random_lightning_interval();
            }
        }
        update
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

    pub fn random_unit(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.rng as f32 / u32::MAX as f32
    }

    pub fn random_offset(&mut self, radius: i32) -> i32 {
        let width = (radius * 2 + 1).max(1) as u32;
        (self.random_unit() * width as f32).floor() as i32 - radius
    }

    fn random_duration_ticks(&mut self) -> f32 {
        MIN_WEATHER_TICKS + self.random_unit() * (MAX_WEATHER_TICKS - MIN_WEATHER_TICKS)
    }

    fn random_lightning_interval(&mut self) -> f32 {
        4.0 + self.random_unit() * 5.0
    }
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
        assert!(weather.update(2.0, 0.0).changed);
        assert_eq!(weather.current, Weather::Rain);
        weather.remaining_ticks = 1.0;
        weather.update(2.0, 0.0);
        assert_eq!(weather.current, Weather::Thunder);
        weather.remaining_ticks = 1.0;
        weather.update(2.0, 0.0);
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
        let update = weather.update(0.0, 0.02);
        assert!(update.lightning_due);
        assert!(weather.flash_intensity() > 0.9);
    }
}

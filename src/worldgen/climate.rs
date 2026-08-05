use crate::world::Biome;
use noise::{NoiseFn, Perlin};

/// A single climate sample at a world column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimateSample {
    pub temperature: f64,
    pub humidity: f64,
    pub continentalness: f64,
    pub erosion: f64,
    pub weirdness: f64,
}

impl ClimateSample {
    pub fn is_ocean(&self) -> bool {
        self.continentalness < -0.35
    }

    pub fn is_deep_ocean(&self) -> bool {
        self.continentalness < -0.55
    }

    pub fn is_near_coast(&self) -> bool {
        self.continentalness < 0.05
    }
}

/// Continuous climate noise sampled at 2D world coordinates.
/// All noise fields are seeded from the world seed with explicit salts so
/// generation is deterministic and thread-independent.
#[derive(Debug, Clone)]
pub struct ClimateSystem {
    pub temperature: Perlin,
    pub humidity: Perlin,
    pub continentalness: Perlin,
    pub erosion: Perlin,
    pub weirdness: Perlin,
}

impl ClimateSystem {
    pub fn new(world_seed: u32) -> Self {
        Self {
            temperature: Perlin::new(world_seed ^ 0xAD90_777D),
            humidity: Perlin::new(world_seed ^ 0x7E95_761E),
            continentalness: Perlin::new(world_seed ^ 0x4CF5_AD43),
            erosion: Perlin::new(world_seed ^ 0x1A2B_3C4D),
            weirdness: Perlin::new(world_seed ^ 0x5E6F_7081),
        }
    }

    pub fn sample(&self, wx: i32, wz: i32) -> ClimateSample {
        ClimateSample {
            temperature: self.temperature.get([wx as f64 * 0.002, wz as f64 * 0.002]),
            humidity: self.humidity.get([wx as f64 * 0.002, wz as f64 * 0.002]),
            continentalness: self
                .continentalness
                .get([wx as f64 * 0.001, wz as f64 * 0.001]),
            erosion: self.erosion.get([wx as f64 * 0.002, wz as f64 * 0.002]),
            weirdness: self.weirdness.get([wx as f64 * 0.002, wz as f64 * 0.002]),
        }
    }

    /// Selects a biome from the climate sample. Rivers, beaches, and oceans
    /// are handled as continuous overlays rather than isolated cells.
    pub fn biome_at(&self, wx: i32, wz: i32) -> Biome {
        biome_from_climate(self.sample(wx, wz))
    }
}

/// Pure biome selection from a climate sample. Exposed for tests.
pub fn biome_from_climate(c: ClimateSample) -> Biome {
    // Deep ocean below the continental shelf.
    if c.is_deep_ocean() {
        return Biome::DeepOcean;
    }
    if c.is_ocean() {
        return Biome::Ocean;
    }

    // Near-coast zones become beach or river.
    if c.is_near_coast() {
        // River noise is derived from the sum of erosion and weirdness,
        // which forms winding bands without a separate noise field.
        let river_like = c.erosion * 0.6 + c.weirdness * 0.4;
        if river_like > 0.3 {
            return Biome::River;
        }
        // Beach is a thin band between land and ocean.
        if c.continentalness > -0.25 && c.continentalness < 0.02 {
            return Biome::Beach;
        }
    }

    // Mountain zones: high weirdness amplitude with erosion extremes.
    if c.weirdness.abs() > 0.72 && c.erosion.abs() > 0.55 {
        return Biome::WindsweptHills;
    }

    // Meadow: moderate altitude, moderate temperature, low erosion.
    if c.temperature > 0.05 && c.temperature < 0.45 && c.erosion < -0.2 {
        return Biome::Meadow;
    }

    // Badlands: hot, dry, high erosion.
    if c.temperature > 0.5 && c.humidity < -0.45 && c.erosion > 0.4 {
        return Biome::Badlands;
    }

    // Desert: hot, very dry.
    if c.temperature > 0.4 && c.humidity < -0.3 {
        return Biome::Desert;
    }

    // Savanna: hot, moderately dry.
    if c.temperature > 0.45 && c.humidity < 0.15 {
        return Biome::Savanna;
    }

    // Snowy Plains: cold, moderate humidity.
    if c.temperature < -0.35 && c.humidity > -0.2 {
        return Biome::SnowyPlains;
    }

    // Taiga: cold, moderate humidity.
    if c.temperature < -0.2 && c.humidity > 0.0 {
        return Biome::Taiga;
    }

    // Jungle: hot, very humid.
    if c.temperature > 0.35 && c.humidity > 0.55 {
        return Biome::Jungle;
    }

    // Swamp: warm, humid.
    if c.temperature > 0.2 && c.humidity > 0.4 {
        return Biome::Swamp;
    }

    // Birch Forest: moderate temperature, moderate humidity.
    if c.temperature > 0.1 && c.temperature < 0.4 && c.humidity > 0.2 {
        return Biome::BirchForest;
    }

    // Forest: moderate temperature, moderate humidity.
    if c.temperature > 0.1 && c.humidity > 0.0 {
        return Biome::Forest;
    }

    // Plains: everything else.
    Biome::Plains
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climate_samples_are_deterministic() {
        let a = ClimateSystem::new(12345);
        let b = ClimateSystem::new(12345);
        for &(x, z) in &[(0, 0), (17, -42), (1000, 77)] {
            assert_eq!(a.sample(x, z), b.sample(x, z));
        }
    }

    #[test]
    fn different_seeds_produce_different_climate() {
        let a = ClimateSystem::new(12345);
        let b = ClimateSystem::new(54321);
        let pa = a.sample(17, -42);
        let pb = b.sample(17, -42);
        assert!(
            (pa.temperature - pb.temperature).abs() > 1e-6
                || (pa.humidity - pb.humidity).abs() > 1e-6
                || (pa.continentalness - pb.continentalness).abs() > 1e-6
        );
    }

    #[test]
    fn deep_ocean_is_below_ocean() {
        let deep = ClimateSample {
            temperature: 0.0,
            humidity: 0.0,
            continentalness: -0.7,
            erosion: 0.0,
            weirdness: 0.0,
        };
        let ocean = ClimateSample {
            temperature: 0.0,
            humidity: 0.0,
            continentalness: -0.4,
            erosion: 0.0,
            weirdness: 0.0,
        };
        assert_eq!(biome_from_climate(deep), Biome::DeepOcean);
        assert_eq!(biome_from_climate(ocean), Biome::Ocean);
    }

    #[test]
    fn desert_and_badlands_are_dry_hot() {
        let desert = ClimateSample {
            temperature: 0.6,
            humidity: -0.5,
            continentalness: 0.3,
            erosion: 0.0,
            weirdness: 0.0,
        };
        assert_eq!(biome_from_climate(desert), Biome::Desert);

        let badlands = ClimateSample {
            temperature: 0.7,
            humidity: -0.6,
            continentalness: 0.3,
            erosion: 0.8,
            weirdness: 0.0,
        };
        assert_eq!(biome_from_climate(badlands), Biome::Badlands);
    }

    #[test]
    fn snowy_plains_and_taiga_are_cold() {
        let snowy = ClimateSample {
            temperature: -0.5,
            humidity: 0.0,
            continentalness: 0.3,
            erosion: 0.0,
            weirdness: 0.0,
        };
        assert_eq!(biome_from_climate(snowy), Biome::SnowyPlains);

        let taiga = ClimateSample {
            temperature: -0.3,
            humidity: 0.3,
            continentalness: 0.3,
            erosion: 0.0,
            weirdness: 0.0,
        };
        assert_eq!(biome_from_climate(taiga), Biome::Taiga);
    }

    #[test]
    fn jungle_and_swamp_are_hot_humid() {
        let jungle = ClimateSample {
            temperature: 0.5,
            humidity: 0.7,
            continentalness: 0.3,
            erosion: 0.0,
            weirdness: 0.0,
        };
        assert_eq!(biome_from_climate(jungle), Biome::Jungle);

        let swamp = ClimateSample {
            temperature: 0.3,
            humidity: 0.6,
            continentalness: 0.3,
            erosion: 0.0,
            weirdness: 0.0,
        };
        assert_eq!(biome_from_climate(swamp), Biome::Swamp);
    }

    #[test]
    fn all_16_biomes_are_reachable() {
        let values = [
            (
                "deep_ocean",
                ClimateSample {
                    temperature: 0.0,
                    humidity: 0.0,
                    continentalness: -0.7,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::DeepOcean,
            ),
            (
                "ocean",
                ClimateSample {
                    temperature: 0.0,
                    humidity: 0.0,
                    continentalness: -0.4,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Ocean,
            ),
            (
                "river",
                ClimateSample {
                    temperature: 0.0,
                    humidity: 0.0,
                    continentalness: -0.1,
                    erosion: 0.4,
                    weirdness: 0.5,
                },
                Biome::River,
            ),
            (
                "beach",
                ClimateSample {
                    temperature: 0.0,
                    humidity: 0.0,
                    continentalness: -0.1,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Beach,
            ),
            (
                "windswept",
                ClimateSample {
                    temperature: 0.0,
                    humidity: 0.0,
                    continentalness: 0.5,
                    erosion: 0.7,
                    weirdness: 0.8,
                },
                Biome::WindsweptHills,
            ),
            (
                "meadow",
                ClimateSample {
                    temperature: 0.2,
                    humidity: 0.0,
                    continentalness: 0.5,
                    erosion: -0.4,
                    weirdness: 0.0,
                },
                Biome::Meadow,
            ),
            (
                "badlands",
                ClimateSample {
                    temperature: 0.7,
                    humidity: -0.6,
                    continentalness: 0.5,
                    erosion: 0.8,
                    weirdness: 0.0,
                },
                Biome::Badlands,
            ),
            (
                "savanna",
                ClimateSample {
                    temperature: 0.6,
                    humidity: 0.0,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Savanna,
            ),
            (
                "desert",
                ClimateSample {
                    temperature: 0.6,
                    humidity: -0.5,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Desert,
            ),
            (
                "snowy_plains",
                ClimateSample {
                    temperature: -0.5,
                    humidity: 0.0,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::SnowyPlains,
            ),
            (
                "taiga",
                ClimateSample {
                    temperature: -0.3,
                    humidity: 0.3,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Taiga,
            ),
            (
                "jungle",
                ClimateSample {
                    temperature: 0.5,
                    humidity: 0.7,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Jungle,
            ),
            (
                "swamp",
                ClimateSample {
                    temperature: 0.3,
                    humidity: 0.6,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Swamp,
            ),
            (
                "birch_forest",
                ClimateSample {
                    temperature: 0.25,
                    humidity: 0.3,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::BirchForest,
            ),
            (
                "forest",
                ClimateSample {
                    temperature: 0.2,
                    humidity: 0.1,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Forest,
            ),
            (
                "plains",
                ClimateSample {
                    temperature: 0.0,
                    humidity: 0.0,
                    continentalness: 0.5,
                    erosion: 0.0,
                    weirdness: 0.0,
                },
                Biome::Plains,
            ),
        ];
        let mut seen = std::collections::HashSet::new();
        for (name, sample, expected) in values {
            let actual = biome_from_climate(sample);
            assert_eq!(actual, expected, "biome mismatch for {name}");
            seen.insert(actual);
        }
        assert_eq!(seen.len(), 16, "all 16 biomes must be reachable");
    }
}

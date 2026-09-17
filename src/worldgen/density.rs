use crate::worldgen::{climate::ClimateSystem, SEA_LEVEL};
use noise::{NoiseFn, Perlin};

/// Terrain surface shaping field combining detail, ridge, and river noise.
#[derive(Debug, Clone)]
pub struct DensityField {
    /// Medium-frequency detail noise for hills and valleys.
    detail_noise: Perlin,
    /// High-frequency ridge noise for mountain shaping.
    ridge_noise: Perlin,
    /// Medium-frequency noise for river carving.
    river_noise: Perlin,
}

impl DensityField {
    pub fn new(world_seed: u32) -> Self {
        Self {
            detail_noise: Perlin::new(world_seed ^ 0x1357_9BDE),
            ridge_noise: Perlin::new(world_seed ^ 0x0F0F_0F0F),
            river_noise: Perlin::new(world_seed ^ 0xABCD_EF01),
        }
    }

    /// Returns the base surface height for a column, before carving.
    pub fn surface_height(&self, climate: &ClimateSystem, wx: i32, wz: i32) -> i32 {
        let c = climate.sample(wx, wz);
        let x = wx as f64;
        let z = wz as f64;

        // Continentalness drives the coarse landmass height.
        let continent = c.continentalness;

        // Base height: ocean floor below sea level, land above.
        let base = if continent < -0.35 {
            // Ocean floor around 30-55.
            (SEA_LEVEL as f64 - 20.0 + (continent + 0.35) * 40.0).round() as i32
        } else {
            // Land: sea level + continentalness scaling.
            SEA_LEVEL + (continent * 60.0).round() as i32
        };

        // Detail noise adds hills and dips.
        let detail = self.detail_noise.get([x * 0.03, z * 0.03]);
        let detail_scale = 8.0 * (1.0 - c.erosion.abs()).max(0.2);
        let detail_h = (detail * detail_scale).round() as i32;

        // Ridge noise creates mountain bands.
        let ridge = self.ridge_noise.get([x * 0.008, z * 0.008]);
        let ridge_scale = if c.weirdness.abs() > 0.6 {
            28.0 * c.weirdness.abs()
        } else {
            8.0 * c.weirdness.abs()
        };
        let ridge_h = (ridge * ridge_scale).round() as i32;

        // River carving: river noise lowers terrain near river paths.
        let river = self.river_noise.get([x * 0.012, z * 0.012]);
        let river_carve = if river.abs() < 0.12 {
            let depth = (0.12 - river.abs()) / 0.12;
            -(depth * 16.0).round() as i32
        } else {
            0
        };

        let mut h = base + detail_h + ridge_h + river_carve;

        // Clamp to world bounds.
        h = h.clamp(-60, 300);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_height_within_world_bounds() {
        let climate = ClimateSystem::new(12345);
        let density = DensityField::new(12345);
        for &(x, z) in &[(-1000, -1000), (0, 0), (1000, 1000), (-37, 42)] {
            let h = density.surface_height(&climate, x, z);
            assert!(
                h >= -60 && h <= 300,
                "height {h} out of bounds at ({x},{z})"
            );
        }
    }
}

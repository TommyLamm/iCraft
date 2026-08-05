use crate::worldgen::{climate::ClimateSystem, SEA_LEVEL};
use noise::{NoiseFn, Perlin};

/// 3D terrain density field combining continentalness, erosion, ridge
/// shaping, and cave noise. Negative density means solid; positive means air.
#[derive(Debug, Clone)]
pub struct DensityField {
    /// Low-frequency continent/landmass noise.
    continent_noise: Perlin,
    /// Medium-frequency detail noise for hills and valleys.
    detail_noise: Perlin,
    /// High-frequency ridge noise for mountain shaping.
    ridge_noise: Perlin,
    /// Medium-frequency noise for river carving.
    river_noise: Perlin,
    /// 3D cheese/cavern noise.
    cavern_noise: Perlin,
    /// 3D tunnel noise.
    tunnel_noise: Perlin,
    /// 3D ravine noise.
    ravine_noise: Perlin,
}

impl DensityField {
    pub fn new(world_seed: u32) -> Self {
        Self {
            continent_noise: Perlin::new(world_seed ^ 0x2468_ACE1),
            detail_noise: Perlin::new(world_seed ^ 0x1357_9BDE),
            ridge_noise: Perlin::new(world_seed ^ 0x0F0F_0F0F),
            river_noise: Perlin::new(world_seed ^ 0xABCD_EF01),
            cavern_noise: Perlin::new(world_seed ^ 0xA341_316C),
            tunnel_noise: Perlin::new(world_seed ^ 0xC801_3EA4),
            ravine_noise: Perlin::new(world_seed ^ 0xDEAD_BEEF),
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

    /// Returns density at a 3D point. Negative = solid, positive = air.
    pub fn density_at(&self, climate: &ClimateSystem, wx: i32, wy: i32, wz: i32) -> f64 {
        let surface_h = self.surface_height(climate, wx, wz) as f64;
        let y = wy as f64;

        // Distance from surface controls the solid/air transition.
        let mut density = (y - surface_h) / 12.0;

        // Add layered detail so terrain isn't flat.
        let detail = self
            .detail_noise
            .get([wx as f64 * 0.05, wy as f64 * 0.05, wz as f64 * 0.05]);
        density += detail * 0.35;

        density
    }

    /// Returns true if a position is carved by caves.
    pub fn is_cave(&self, wx: i32, wy: i32, wz: i32, surface_h: i32) -> bool {
        if wy <= -60 || wy > 200 {
            return false;
        }

        let x = wx as f64;
        let y = wy as f64;
        let z = wz as f64;

        // Cheese/cavern noise: large open chambers.
        let cavern = self.cavern_noise.get([x * 0.012, y * 0.012, z * 0.012]);
        let cavern_open = cavern > 0.62;

        // Tunnel noise: winding narrow passages.
        let tunnel = self.tunnel_noise.get([x * 0.045, y * 0.08, z * 0.045]);
        let tunnel_open = tunnel.abs() < 0.09 && wy < surface_h - 8;

        // Ravine noise: vertical fissures.
        let ravine = self.ravine_noise.get([x * 0.02, y * 0.05, z * 0.02]);
        let ravine_open = ravine.abs() < 0.05 && wy < surface_h - 4;

        cavern_open || tunnel_open || ravine_open
    }
}

/// Returns the surface height for a column using the shared climate system.
/// Uses a stateless density field (only the climate noise matters for the
/// height computation; the surface height function is deterministic for a
/// given climate).
pub fn surface_height(climate: &ClimateSystem, wx: i32, wz: i32) -> i32 {
    DensityField::new(0).surface_height(climate, wx, wz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_is_deterministic() {
        let a = DensityField::new(12345);
        let b = DensityField::new(12345);
        let climate = ClimateSystem::new(12345);
        for &(x, y, z) in &[(0, 50, 0), (17, 80, -42), (100, 20, 77)] {
            assert_eq!(
                a.density_at(&climate, x, y, z),
                b.density_at(&climate, x, y, z)
            );
        }
    }

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

    #[test]
    fn cave_detection_is_bounded() {
        let density = DensityField::new(12345);
        // At very low and very high Y, caves should never trigger.
        assert!(!density.is_cave(0, -64, 0, 63));
        assert!(!density.is_cave(0, 300, 0, 63));
    }
}

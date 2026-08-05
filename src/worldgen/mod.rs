pub mod carver;
pub mod climate;
pub mod density;
pub mod feature;
pub mod ore;
pub mod surface;

use crate::dimension::Dimension;

/// Overworld sea level (matching vanilla 1.21.5).
pub const SEA_LEVEL: i32 = 63;
/// River bed depth relative to sea level.
pub const RIVER_BED_DEPTH: i32 = 8;
/// Beach transition band used by the surface overlay.
pub const BEACH_BAND: f64 = 0.06;

/// Shared world-generation context for a single world seed.
/// All perlin fields derive from seed ^ salt so generation is deterministic
/// and order-independent.
#[derive(Clone, Debug)]
pub struct WorldGenContext {
    pub seed: u32,
    pub climate: climate::ClimateSystem,
    pub density: density::DensityField,
    pub carver: carver::CaveCarver,
    pub ore: ore::OreGenerator,
}

impl WorldGenContext {
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            climate: climate::ClimateSystem::new(seed),
            density: density::DensityField::new(seed),
            carver: carver::CaveCarver::new(seed),
            ore: ore::OreGenerator::new(seed),
        }
    }

    /// Returns the biome for a world column, including river/beach overlays.
    pub fn biome_at(&self, wx: i32, wz: i32) -> crate::world::Biome {
        self.climate.biome_at(wx, wz)
    }

    /// Returns the terrain surface height for a world column.
    pub fn surface_height_at(&self, wx: i32, wz: i32) -> i32 {
        self.density.surface_height(&self.climate, wx, wz)
    }

    /// Returns the block type for a world position.
    pub fn block_at(&self, wx: i32, wy: i32, wz: i32) -> Option<crate::world::BlockType> {
        let surface_y = self.surface_height_at(wx, wz);
        let biome = self.biome_at(wx, wz);
        surface::block_for_column(self, wx, wy, wz, surface_y, biome)
    }

    /// Whether this context is valid for the given dimension.
    pub fn supports_dimension(&self, dimension: Dimension) -> bool {
        matches!(dimension, Dimension::Overworld)
    }
}

/// Computes a deterministic PRNG seed from a world seed and a salt.
pub fn salted_seed(world_seed: u32, salt: u32) -> u32 {
    world_seed ^ salt
}

/// Deterministic hash for feature placement that does not depend on
/// iteration order.
pub fn hash_coord(seed: u32, x: i32, y: i32, z: i32, salt: u32) -> u32 {
    let mut value = seed
        ^ salt
        ^ (x as u32).wrapping_mul(0x9E37_79B9)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^= value >> 16;
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_chunk_surface_height_continuity() {
        let ctx = WorldGenContext::new(12345);
        // Check adjacent voxels across chunk boundary x=15 (cx=0) and x=16 (cx=1).
        for z in 0..16 {
            let h1 = ctx.surface_height_at(15, z);
            let h2 = ctx.surface_height_at(16, z);
            let diff = (h1 - h2).abs();
            assert!(
                diff <= 4,
                "surface height jump of {diff} across chunk boundary at z={z}: {h1} vs {h2}"
            );
        }
    }

    #[test]
    fn chunk_generation_is_byte_identical_across_threads() {
        use std::sync::Arc;
        let ctx = Arc::new(WorldGenContext::new(9999));
        let handle1 = {
            let ctx = Arc::clone(&ctx);
            std::thread::spawn(move || {
                (0..16)
                    .map(|x| (0..16).map(|z| ctx.block_at(x, 64, z)).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            })
        };
        let handle2 = {
            let ctx = Arc::clone(&ctx);
            std::thread::spawn(move || {
                (0..16)
                    .map(|x| (0..16).map(|z| ctx.block_at(x, 64, z)).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            })
        };

        let res1 = handle1.join().unwrap();
        let res2 = handle2.join().unwrap();
        assert_eq!(res1, res2);
    }
}

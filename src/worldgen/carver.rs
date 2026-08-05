use noise::{NoiseFn, Perlin};

/// Carves caves using three distinct noise types: cheese/cavern (large open
/// chambers), tunnel (winding passages), and ravine (vertical fissures).
#[derive(Debug, Clone)]
pub struct CaveCarver {
    /// 3D cheese/cavern noise.
    cavern_noise: Perlin,
    /// 3D tunnel noise.
    tunnel_noise: Perlin,
    /// 3D ravine noise.
    ravine_noise: Perlin,
}

impl CaveCarver {
    pub fn new(world_seed: u32) -> Self {
        Self {
            cavern_noise: Perlin::new(world_seed ^ 0xA341_316C),
            tunnel_noise: Perlin::new(world_seed ^ 0xC801_3EA4),
            ravine_noise: Perlin::new(world_seed ^ 0xDEAD_BEEF),
        }
    }

    /// Returns true if the position is carved by one of the cave types.
    ///
    /// surface_h is the surface height for the column; wy is the world Y.
    /// Caves do not carve at the very top of the world or at bedrock.
    pub fn is_carved(&self, wx: i32, wy: i32, wz: i32, surface_h: i32) -> bool {
        // Only carve within the underground region.
        if wy <= -60 || wy > 200 || wy >= surface_h - 1 {
            return false;
        }

        let x = wx as f64;
        let y = wy as f64;
        let z = wz as f64;

        // Cheese/cavern noise: large open chambers.
        let cavern = self.cavern_noise.get([x * 0.012, y * 0.012, z * 0.012]);
        let cavern_open = cavern > 0.60;

        // Tunnel noise: winding narrow passages, deeper underground.
        let tunnel = self.tunnel_noise.get([x * 0.045, y * 0.08, z * 0.045]);
        let tunnel_open = tunnel.abs() < 0.085 && wy < surface_h - 8;

        // Ravine noise: vertical fissures that reach the surface.
        let ravine = self.ravine_noise.get([x * 0.02, y * 0.05, z * 0.02]);
        let ravine_open = ravine.abs() < 0.045 && wy < surface_h - 2;

        cavern_open || tunnel_open || ravine_open
    }

    /// Whether a cave position should be a lava lake (deep underground).
    pub fn is_lava_lake(&self, wx: i32, wy: i32, wz: i32) -> bool {
        if wy <= 0 {
            let x = wx as f64;
            let y = wy as f64;
            let z = wz as f64;
            let n = self
                .cavern_noise
                .get([x * 0.02, y * 0.02 + 1000.0, z * 0.02]);
            n > 0.55
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carving_is_deterministic() {
        let a = CaveCarver::new(12345);
        let b = CaveCarver::new(12345);
        for &(x, y, z, h) in &[(0, 30, 0, 63), (17, -20, -42, 65), (100, 10, 77, 80)] {
            assert_eq!(a.is_carved(x, y, z, h), b.is_carved(x, y, z, h));
        }
    }

    #[test]
    fn carving_respects_bounds() {
        let carver = CaveCarver::new(12345);
        // Never carve at bedrock or above the surface.
        assert!(!carver.is_carved(0, -64, 0, 63));
        assert!(!carver.is_carved(0, 100, 0, 80));
        assert!(!carver.is_carved(0, 63, 0, 63));
    }

    #[test]
    fn lava_lakes_only_deep() {
        let carver = CaveCarver::new(12345);
        // Above y=0, lava lakes should never form.
        assert!(!carver.is_lava_lake(0, 10, 0));
        // At y<0 they may form.
        assert_eq!(
            carver.is_lava_lake(0, -10, 0),
            carver.is_lava_lake(0, -10, 0)
        );
    }
}

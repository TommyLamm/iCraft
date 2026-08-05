use crate::world::{Biome, BlockType};
use crate::worldgen::{WorldGenContext, SEA_LEVEL};

/// Surface composition data for a biome.
#[derive(Debug, Clone, Copy)]
pub struct BiomeSurfaceData {
    pub top: BlockType,
    pub filler: BlockType,
    pub underwater: BlockType,
    pub underwater_filler: BlockType,
    pub is_snowy: bool,
    pub is_dry: bool,
}

impl BiomeSurfaceData {
    pub fn for_biome(biome: Biome) -> Self {
        use BlockType::*;
        match biome {
            Biome::Plains | Biome::Forest | Biome::BirchForest | Biome::Meadow => Self {
                top: Grass,
                filler: Dirt,
                underwater: Sand,
                underwater_filler: Dirt,
                is_snowy: false,
                is_dry: false,
            },
            Biome::Taiga => Self {
                top: Grass,
                filler: Dirt,
                underwater: Gravel,
                underwater_filler: Dirt,
                is_snowy: false,
                is_dry: false,
            },
            Biome::SnowyPlains => Self {
                top: Grass,
                filler: Dirt,
                underwater: Gravel,
                underwater_filler: Dirt,
                is_snowy: true,
                is_dry: false,
            },
            Biome::Desert => Self {
                top: Sand,
                filler: Sandstone,
                underwater: Sand,
                underwater_filler: Sandstone,
                is_snowy: false,
                is_dry: true,
            },
            Biome::Savanna => Self {
                top: Grass,
                filler: Dirt,
                underwater: Sand,
                underwater_filler: Dirt,
                is_snowy: false,
                is_dry: true,
            },
            Biome::Swamp => Self {
                top: Grass,
                filler: Dirt,
                underwater: Clay,
                underwater_filler: Dirt,
                is_snowy: false,
                is_dry: false,
            },
            Biome::Jungle => Self {
                top: Grass,
                filler: Dirt,
                underwater: Sand,
                underwater_filler: Dirt,
                is_snowy: false,
                is_dry: false,
            },
            Biome::Badlands => Self {
                top: Sand,
                filler: Sandstone,
                underwater: Sand,
                underwater_filler: Sandstone,
                is_snowy: false,
                is_dry: true,
            },
            Biome::WindsweptHills => Self {
                top: Stone,
                filler: Stone,
                underwater: Gravel,
                underwater_filler: Stone,
                is_snowy: true,
                is_dry: false,
            },
            Biome::River => Self {
                top: Sand,
                filler: Dirt,
                underwater: Gravel,
                underwater_filler: Dirt,
                is_snowy: false,
                is_dry: false,
            },
            Biome::Beach => Self {
                top: Sand,
                filler: Sand,
                underwater: Sand,
                underwater_filler: Sand,
                is_snowy: false,
                is_dry: false,
            },
            Biome::Ocean | Biome::DeepOcean => Self {
                top: Sand,
                filler: Dirt,
                underwater: Sand,
                underwater_filler: Dirt,
                is_snowy: false,
                is_dry: false,
            },
        }
    }
}

/// Computes the block type for a column position.
///
/// Returns None for air (or water above the sea floor).
pub fn block_for_column(
    _ctx: &WorldGenContext,
    _wx: i32,
    wy: i32,
    _wz: i32,
    surface_y: i32,
    biome: Biome,
) -> Option<BlockType> {
    use BlockType::*;

    // Bedrock and void protection at world bottom.
    if wy <= -60 {
        return Some(if wy == -60 { Bedrock } else { Stone });
    }

    let surface = BiomeSurfaceData::for_biome(biome);

    if wy == surface_y {
        // Surface / sea-floor block.
        if surface_y <= SEA_LEVEL {
            return Some(surface.underwater);
        }
        if surface.is_snowy && wy > 80 {
            return Some(Snow);
        }
        return Some(surface.top);
    }

    if wy < surface_y {
        // Below the surface: filler then stone.
        let depth = surface_y - wy;
        if depth <= 3 {
            return Some(if surface_y <= SEA_LEVEL {
                surface.underwater_filler
            } else {
                surface.filler
            });
        }
        return Some(Stone);
    }

    // Above the surface: water up to sea level, otherwise air.
    if wy <= SEA_LEVEL {
        return Some(Water);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_biome_has_surface_data() {
        let all = [
            Biome::Plains,
            Biome::Forest,
            Biome::BirchForest,
            Biome::Taiga,
            Biome::SnowyPlains,
            Biome::Desert,
            Biome::Savanna,
            Biome::Swamp,
            Biome::Jungle,
            Biome::Badlands,
            Biome::Meadow,
            Biome::WindsweptHills,
            Biome::River,
            Biome::Beach,
            Biome::Ocean,
            Biome::DeepOcean,
        ];
        for biome in all {
            let data = BiomeSurfaceData::for_biome(biome);
            assert_ne!(data.top, BlockType::Air);
            assert_ne!(data.filler, BlockType::Air);
        }
    }

    #[test]
    fn block_for_column_handles_water() {
        let ctx = WorldGenContext::new(12345);
        // Surface at y=50 (below sea level), water fills above.
        let water = block_for_column(&ctx, 0, 60, 0, 50, Biome::Ocean);
        assert_eq!(water, Some(BlockType::Water));

        let air = block_for_column(&ctx, 0, 70, 0, 65, Biome::Plains);
        assert_eq!(air, None);
    }
}

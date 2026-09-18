use crate::world::{Biome, BlockType};
use crate::worldgen::SEA_LEVEL;

/// Surface composition data for a biome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiomeSurfaceData {
    pub top: BlockType,
    pub filler: BlockType,
    pub underwater: BlockType,
    pub underwater_filler: BlockType,
    pub is_snowy: bool,
}

impl BiomeSurfaceData {
    pub fn for_biome(biome: Biome) -> Self {
        use BlockType::*;
        match biome {
            Biome::Plains
            | Biome::Forest
            | Biome::BirchForest
            | Biome::Meadow
            | Biome::Savanna
            | Biome::Jungle => Self {
                top: Grass,
                filler: Dirt,
                underwater: Sand,
                underwater_filler: Dirt,
                is_snowy: false,
            },
            Biome::Taiga => Self {
                top: Grass,
                filler: Dirt,
                underwater: Gravel,
                underwater_filler: Dirt,
                is_snowy: false,
            },
            Biome::SnowyPlains => Self {
                top: Grass,
                filler: Dirt,
                underwater: Gravel,
                underwater_filler: Dirt,
                is_snowy: true,
            },
            Biome::Desert | Biome::Badlands => Self {
                top: Sand,
                filler: Sandstone,
                underwater: Sand,
                underwater_filler: Sandstone,
                is_snowy: false,
            },
            Biome::Swamp => Self {
                top: Grass,
                filler: Dirt,
                underwater: Clay,
                underwater_filler: Dirt,
                is_snowy: false,
            },
            Biome::WindsweptHills => Self {
                top: Stone,
                filler: Stone,
                underwater: Gravel,
                underwater_filler: Stone,
                is_snowy: true,
            },
            Biome::River => Self {
                top: Sand,
                filler: Dirt,
                underwater: Gravel,
                underwater_filler: Dirt,
                is_snowy: false,
            },
            Biome::Beach => Self {
                top: Sand,
                filler: Sand,
                underwater: Sand,
                underwater_filler: Sand,
                is_snowy: false,
            },
            Biome::Ocean | Biome::DeepOcean => Self {
                top: Sand,
                filler: Dirt,
                underwater: Sand,
                underwater_filler: Dirt,
                is_snowy: false,
            },
        }
    }
}

/// Computes the block type for a column position given precomputed surface height and biome surface data.
///
/// Returns None for air (or water above the sea floor).
pub fn block_for_column(
    wy: i32,
    surface_y: i32,
    surface: &BiomeSurfaceData,
) -> Option<BlockType> {
    use BlockType::*;

    // Bedrock floor at the dimension minimum. Nothing below is diggable stone.
    let min_y = crate::dimension::Dimension::Overworld.height().min_y();
    if wy <= min_y {
        return Some(Bedrock);
    }

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
    fn merged_biome_surface_data_matches() {
        assert_eq!(
            BiomeSurfaceData::for_biome(Biome::Desert),
            BiomeSurfaceData::for_biome(Biome::Badlands)
        );
        assert_eq!(
            BiomeSurfaceData::for_biome(Biome::Plains),
            BiomeSurfaceData::for_biome(Biome::Savanna)
        );
        assert_eq!(
            BiomeSurfaceData::for_biome(Biome::Plains),
            BiomeSurfaceData::for_biome(Biome::Jungle)
        );
    }

    #[test]
    fn overworld_floor_is_bedrock_at_min_y() {
        let min_y = crate::dimension::Dimension::Overworld.height().min_y();
        let surface = BiomeSurfaceData::for_biome(Biome::Plains);
        let floor = block_for_column(min_y, 70, &surface);
        assert_eq!(floor, Some(BlockType::Bedrock));
        let below = block_for_column(min_y - 1, 70, &surface);
        assert_eq!(below, Some(BlockType::Bedrock));
        let above = block_for_column(min_y + 1, 70, &surface);
        assert_ne!(above, Some(BlockType::Bedrock));
    }

    #[test]
    fn block_for_column_handles_water() {
        let surface_ocean = BiomeSurfaceData::for_biome(Biome::Ocean);
        // Surface at y=50 (below sea level), water fills above.
        let water = block_for_column(60, 50, &surface_ocean);
        assert_eq!(water, Some(BlockType::Water));

        let surface_plains = BiomeSurfaceData::for_biome(Biome::Plains);
        let air = block_for_column(70, 65, &surface_plains);
        assert_eq!(air, None);
    }

    #[test]
    fn block_for_column_handles_snow_filler_and_stone() {
        let surface_snow = BiomeSurfaceData::for_biome(Biome::SnowyPlains);
        // Snow block on high snowy surface.
        assert_eq!(block_for_column(90, 90, &surface_snow), Some(BlockType::Snow));
        // Normal grass on snowy surface at or below y=80.
        assert_eq!(block_for_column(80, 80, &surface_snow), Some(BlockType::Grass));
        // Filler within 3 blocks below surface.
        assert_eq!(block_for_column(78, 80, &surface_snow), Some(BlockType::Dirt));
        // Stone more than 3 blocks below surface.
        assert_eq!(block_for_column(70, 80, &surface_snow), Some(BlockType::Stone));
    }
}

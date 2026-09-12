use super::*;

pub(super) fn ambient_occlusion_value(occluders: u8) -> f32 {
    match occluders.min(3) {
        0 => 1.0,
        1 => 0.75,
        2 => 0.5,
        _ => 0.25,
    }
}

pub(super) fn ao_sample_positions(
    block_position: [i32; 3],
    normal: [i32; 3],
    corner: [f32; 3],
) -> [[i32; 3]; 3] {
    let tangent_axes = if normal[0] != 0 {
        [1, 2]
    } else if normal[1] != 0 {
        [0, 2]
    } else {
        [0, 1]
    };

    let mut side_u = [0; 3];
    let mut side_v = [0; 3];
    side_u[tangent_axes[0]] = if corner[tangent_axes[0]] == 0.0 {
        -1
    } else {
        1
    };
    side_v[tangent_axes[1]] = if corner[tangent_axes[1]] == 0.0 {
        -1
    } else {
        1
    };

    let outside = [
        block_position[0] + normal[0],
        block_position[1] + normal[1],
        block_position[2] + normal[2],
    ];
    [
        [
            outside[0] + side_u[0],
            outside[1] + side_u[1],
            outside[2] + side_u[2],
        ],
        [
            outside[0] + side_v[0],
            outside[1] + side_v[1],
            outside[2] + side_v[2],
        ],
        [
            outside[0] + side_u[0] + side_v[0],
            outside[1] + side_u[1] + side_v[1],
            outside[2] + side_u[2] + side_v[2],
        ],
    ]
}

pub(super) fn ambient_occlusion_for_vertex<F>(
    block_position: [i32; 3],
    normal: [i32; 3],
    corner: [f32; 3],
    get_block_at: &F,
) -> f32
where
    F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
{
    let occluders = ao_sample_positions(block_position, normal, corner)
        .iter()
        .filter(|position| {
            get_block_at(position[0], position[1], position[2])
                .0
                .is_ao_occluder()
        })
        .count() as u8;
    ambient_occlusion_value(occluders)
}

pub(super) fn quad_indices_for_ao(ao: [f32; 4]) -> [u32; 6] {
    if ao[0] + ao[2] > ao[1] + ao[3] {
        [0, 1, 3, 1, 2, 3]
    } else {
        [0, 1, 2, 0, 2, 3]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GreedyFace {
    pub(super) block: BlockType,
    pub(super) atlas_tile: (u32, u32),
    pub(super) light_level: u16,
    pub(super) ao_levels: [u8; 4],
}

impl GreedyFace {
    pub(super) fn can_merge_with(self, other: Self) -> bool {
        self.ao_levels
            .iter()
            .all(|level| *level == self.ao_levels[0])
            && other
                .ao_levels
                .iter()
                .all(|level| *level == other.ao_levels[0])
            && self == other
    }

    pub(super) fn ao(self) -> [f32; 4] {
        self.ao_levels.map(ambient_occlusion_value)
    }
}

pub(super) fn ao_level(value: f32) -> u8 {
    if value >= 0.875 {
        0
    } else if value >= 0.625 {
        1
    } else if value >= 0.375 {
        2
    } else {
        3
    }
}

pub(super) fn push_terrain_quad(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    positions: [[f32; 3]; 4],
    local_uvs: [[f32; 2]; 4],
    atlas_tile: (u32, u32),
    light_level: f32,
    ao: [f32; 4],
    region_coord: (i32, i32),
) {
    let start = vertices.len() as u32;
    for corner in 0..4 {
        vertices.push(TerrainVertex::new(
            positions[corner],
            local_uvs[corner],
            [atlas_tile.0 as f32, atlas_tile.1 as f32],
            light_level,
            ao[corner],
            region_coord,
        ));
    }
    indices.extend(quad_indices_for_ao(ao).iter().map(|index| start + index));
}

pub(super) const TORCH_MIN: f32 = 7.0 / 16.0;
pub(super) const TORCH_MAX: f32 = 9.0 / 16.0;
pub(super) const TORCH_HEIGHT: f32 = 10.0 / 16.0;
pub(super) const TORCH_ATLAS_TILE: (u32, u32) = (4, 2);
pub(super) const REDSTONE_TORCH_ATLAS_TILE: (u32, u32) = (6, 2);

pub(super) const CACTUS_MIN: f32 = 1.0 / 16.0;
pub(super) const CACTUS_MAX: f32 = 15.0 / 16.0;
pub(super) const END_PORTAL_FRAME_HEIGHT: f32 = 13.0 / 16.0;
pub(super) const END_PORTAL_SURFACE_HEIGHT: f32 = 12.0 / 16.0;

// Tile-local UV rectangles with a half-texel inset. Side faces use the full
// flame/stem artwork, the cap uses the flame, and the base stretches the final
// stem texel across the otherwise unseen bottom face.
pub(super) const TORCH_SIDE_UV: [f32; 4] = [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 13.5 / 16.0];
pub(super) const TORCH_TOP_UV: [f32; 4] = [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 4.5 / 16.0];
pub(super) const TORCH_BOTTOM_UV: [f32; 4] = [7.5 / 16.0, 13.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0];

pub(super) fn append_torch_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    let light_level = sky_light as f32 + block_light as f32 * 16.0;

    for (face_idx, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let uv_rect = match face_idx {
            0..=3 => TORCH_SIDE_UV,
            4 => TORCH_TOP_UV,
            5 => TORCH_BOTTOM_UV,
            _ => unreachable!(),
        };
        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];

        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            positions[corner_idx] = [
                origin[0]
                    + if offset[0] == 0.0 {
                        TORCH_MIN
                    } else {
                        TORCH_MAX
                    },
                origin[1] + if offset[1] == 0.0 { 0.0 } else { TORCH_HEIGHT },
                origin[2]
                    + if offset[2] == 0.0 {
                        TORCH_MIN
                    } else {
                        TORCH_MAX
                    },
            ];
            local_uvs[corner_idx] = [
                if uv[0] == 0.0 { uv_rect[0] } else { uv_rect[2] },
                if uv[1] == 0.0 { uv_rect[1] } else { uv_rect[3] },
            ];
        }

        push_terrain_quad(
            vertices,
            indices,
            positions,
            local_uvs,
            atlas_tile,
            light_level,
            [1.0; 4],
            region_coord,
        );
    }
}

pub(super) fn append_cactus_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    let light_level = sky_light as f32 + block_light as f32 * 16.0;

    for (face_idx, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let multiplier_code = match face_idx {
            4 => 0.0, // Top
            5 => 2.0, // Bottom
            _ => 1.0, // Sides
        };
        let face_light_level = light_level + multiplier_code * 256.0;

        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];

        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            let vx = origin[0]
                + if offset[0] == 0.0 {
                    CACTUS_MIN
                } else {
                    CACTUS_MAX
                };
            let vy = origin[1] + offset[1];
            let vz = origin[2]
                + if offset[2] == 0.0 {
                    CACTUS_MIN
                } else {
                    CACTUS_MAX
                };
            positions[corner_idx] = [vx, vy, vz];

            let u = if uv[0] == 0.0 { CACTUS_MIN } else { CACTUS_MAX };
            let v = if face_idx < 4 {
                uv[1]
            } else if uv[1] == 0.0 {
                CACTUS_MIN
            } else {
                CACTUS_MAX
            };
            local_uvs[corner_idx] = [u, v];
        }

        push_terrain_quad(
            vertices,
            indices,
            positions,
            local_uvs,
            atlas_tile,
            face_light_level,
            [1.0; 4],
            region_coord,
        );
    }
}

pub(super) fn append_end_portal_frame_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    block: BlockType,
    state: BlockState,
    sky_light: u8,
    block_light: u8,
    region_coord: (i32, i32),
    registry: Option<&crate::block_model::ModelRegistry>,
) {
    for (face_idx, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let multiplier_code = match face_idx {
            4 => 0.0,
            5 => 2.0,
            _ => 1.0,
        };
        let light_level = sky_light as f32 + block_light as f32 * 16.0 + multiplier_code * 256.0;
        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];
        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            positions[corner_idx] = [
                origin[0] + offset[0],
                origin[1]
                    + if offset[1] == 0.0 {
                        0.0
                    } else {
                        END_PORTAL_FRAME_HEIGHT
                    },
                origin[2] + offset[2],
            ];
            local_uvs[corner_idx] = *uv;
        }
        let fallback_tile = block.face_tex_for(state, face_idx);
        let atlas_tile = registry.map_or(fallback_tile, |registry| {
            registry.atlas_tile_for_block(block, fallback_tile)
        });
        push_terrain_quad(
            vertices,
            indices,
            positions,
            local_uvs,
            atlas_tile,
            light_level,
            [1.0; 4],
            region_coord,
        );
    }
}

pub(super) fn append_end_portal_surface(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    region_coord: (i32, i32),
    registry: Option<&crate::block_model::ModelRegistry>,
) {
    let y = origin[1] + END_PORTAL_SURFACE_HEIGHT;
    let positions = [
        [origin[0], y, origin[2] + 1.0],
        [origin[0] + 1.0, y, origin[2] + 1.0],
        [origin[0] + 1.0, y, origin[2]],
        [origin[0], y, origin[2]],
    ];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let fallback_tile = BlockType::EndPortal.get_face_tex_index(4);
    let tile = registry.map_or(fallback_tile, |registry| {
        registry.atlas_tile_for_block(BlockType::EndPortal, fallback_tile)
    });
    let light_level = 15.0 * 16.0 + 15.0;
    push_terrain_quad(
        vertices,
        indices,
        positions,
        uvs,
        tile,
        light_level,
        [1.0; 4],
        region_coord,
    );
    push_terrain_quad(
        vertices,
        indices,
        [positions[3], positions[2], positions[1], positions[0]],
        uvs,
        tile,
        light_level,
        [1.0; 4],
        region_coord,
    );
}

pub(super) fn face_should_render(
    block: BlockType,
    face_idx: usize,
    level: u8,
    falling: bool,
    neighbor: BlockType,
    neighbor_level: u8,
    neighbor_falling: bool,
) -> bool {
    if neighbor == BlockType::Air {
        return true;
    }

    if neighbor.def().properties.render_type == RenderType::Opaque {
        return false;
    }

    let is_fluid = matches!(block, BlockType::Water | BlockType::Lava);
    if !is_fluid || neighbor != block {
        return true;
    }

    match face_idx {
        4 | 5 => false,
        _ if neighbor_falling => false,
        _ if falling => true,
        _ => neighbor_level > level,
    }
}

pub(super) fn append_box_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    bounds: ([f32; 3], [f32; 3]),
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    crate::block_model::emit_box(
        vertices,
        indices,
        origin,
        bounds,
        sky_light,
        block_light,
        atlas_tile,
        region_coord,
        None,
    );
}

pub(super) fn append_door_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    state: BlockState,
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    const THICKNESS: f32 = 3.0 / 16.0;

    let (min_x, max_x, min_z, max_z) = if !state.is_open {
        match state.facing {
            Direction::North => (0.0, 1.0, 0.0, THICKNESS),
            Direction::South => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
            Direction::West => (0.0, THICKNESS, 0.0, 1.0),
            _ => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
        }
    } else if !state.is_right_hinge {
        match state.facing {
            Direction::North => (0.0, THICKNESS, 0.0, 1.0),
            Direction::South => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
            Direction::West => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
            _ => (0.0, 1.0, 0.0, THICKNESS),
        }
    } else {
        match state.facing {
            Direction::North => (1.0 - THICKNESS, 1.0, 0.0, 1.0),
            Direction::South => (0.0, THICKNESS, 0.0, 1.0),
            Direction::West => (0.0, 1.0, 0.0, THICKNESS),
            _ => (0.0, 1.0, 1.0 - THICKNESS, 1.0),
        }
    };

    append_box_mesh(
        vertices,
        indices,
        origin,
        ([min_x, 0.0, min_z], [max_x, 1.0, max_z]),
        sky_light,
        block_light,
        atlas_tile,
        region_coord,
    );
}

pub(super) fn append_trapdoor_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    state: BlockState,
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    const THICKNESS: f32 = 3.0 / 16.0;

    let bounds = if !state.is_open {
        ([0.0, 0.0, 0.0], [1.0, THICKNESS, 1.0])
    } else {
        match state.facing {
            Direction::North => ([0.0, 0.0, 0.0], [1.0, 1.0, THICKNESS]),
            Direction::South => ([0.0, 0.0, 1.0 - THICKNESS], [1.0, 1.0, 1.0]),
            Direction::West => ([0.0, 0.0, 0.0], [THICKNESS, 1.0, 1.0]),
            _ => ([1.0 - THICKNESS, 0.0, 0.0], [1.0, 1.0, 1.0]),
        }
    };

    append_box_mesh(
        vertices,
        indices,
        origin,
        bounds,
        sky_light,
        block_light,
        atlas_tile,
        region_coord,
    );
}

pub(super) fn is_greedy_cube(block: BlockType) -> bool {
    let def = block.def();
    def.properties.is_solid
        && !def.is_cross_model
        && !matches!(
            block,
            BlockType::Water
                | BlockType::Lava
                | BlockType::SnowLayer
                | BlockType::OakDoor
                | BlockType::OakTrapdoor
                | BlockType::Cactus
                | BlockType::EndPortalFrame
                | BlockType::OakSlab
                | BlockType::CobblestoneSlab
                | BlockType::OakStair
                | BlockType::CobblestoneStair
                | BlockType::OakFence
                | BlockType::OakFenceGate
                | BlockType::CobblestoneWall
                | BlockType::GlassPane
                | BlockType::OakLadder
                | BlockType::OakSign
                | BlockType::Hopper
                | BlockType::Chest
                | BlockType::EndCityChest
        )
}



use crate::chunk_render::{ChunkLodMeshData, ChunkMeshBundle, TerrainVertex};
use crate::redstone::Direction;
use crate::world::block::{
    BlockState, BlockType, RenderType, CHUNK_DEPTH, CHUNK_WIDTH, FLUID_WATERLOGGED_BIT,
};
use crate::world::chunk::Chunk;
use crate::world::section::{
    world_y_to_section_y, SectionIdentity, SectionKey, NO_HEIGHT, SECTION_SIZE, SECTION_VOLUME,
};

/// Complete voxel value consumed by section meshing. Keeping all render
/// inputs together prevents workers from falling back to live world lookups.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MeshVoxel {
    pub block: BlockType,
    pub state: u8,
    pub sky: u8,
    pub block_light: u8,
    pub raw_fluid: u8,
}

impl Default for MeshVoxel {
    fn default() -> Self {
        Self {
            block: BlockType::Air,
            state: 0,
            sky: 0,
            block_light: 0,
            raw_fluid: 0,
        }
    }
}

/// Immutable 18^3 voxel snapshot (one-cell halo on all six sides).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionHaloSnapshot {
    pub key: SectionKey,
    pub voxels: Box<[MeshVoxel]>,
}

impl SectionHaloSnapshot {
    pub const SIDE: usize = SECTION_SIZE + 2;
    pub const VOLUME: usize = Self::SIDE * Self::SIDE * Self::SIDE;
    pub fn from_chunk<F>(key: SectionKey, mut get: F) -> Self
    where
        F: FnMut(i32, i32, i32) -> MeshVoxel,
    {
        let mut voxels = vec![MeshVoxel::default(); Self::VOLUME].into_boxed_slice();
        for ly in 0..Self::SIDE {
            for z in 0..Self::SIDE {
                for x in 0..Self::SIDE {
                    let wx = key.cx * CHUNK_WIDTH as i32 + x as i32 - 1;
                    let wy = key.min_world_y() + ly as i32 - 1;
                    let wz = key.cz * CHUNK_DEPTH as i32 + z as i32 - 1;
                    voxels[(ly * Self::SIDE + z) * Self::SIDE + x] = get(wx, wy, wz);
                }
            }
        }
        Self { key, voxels }
    }
    pub fn get(&self, x: usize, y: usize, z: usize) -> MeshVoxel {
        self.voxels[(y * Self::SIDE + z) * Self::SIDE + x]
    }
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockType {
        self.get(x, y, z).block
    }
}

type FaceCorner = ([f32; 3], [f32; 2]);

// Face order: south, north, west, east, up, down.
const BLOCK_FACES: [([i32; 3], [FaceCorner; 4]); 6] = [
    (
        [0, 0, 1],
        [
            ([0.0, 0.0, 1.0], [0.0, 1.0]),
            ([1.0, 0.0, 1.0], [1.0, 1.0]),
            ([1.0, 1.0, 1.0], [1.0, 0.0]),
            ([0.0, 1.0, 1.0], [0.0, 0.0]),
        ],
    ),
    (
        [0, 0, -1],
        [
            ([1.0, 0.0, 0.0], [0.0, 1.0]),
            ([0.0, 0.0, 0.0], [1.0, 1.0]),
            ([0.0, 1.0, 0.0], [1.0, 0.0]),
            ([1.0, 1.0, 0.0], [0.0, 0.0]),
        ],
    ),
    (
        [-1, 0, 0],
        [
            ([0.0, 0.0, 0.0], [0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 1.0]),
            ([0.0, 1.0, 1.0], [1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 0.0]),
        ],
    ),
    (
        [1, 0, 0],
        [
            ([1.0, 0.0, 1.0], [0.0, 1.0]),
            ([1.0, 0.0, 0.0], [1.0, 1.0]),
            ([1.0, 1.0, 0.0], [1.0, 0.0]),
            ([1.0, 1.0, 1.0], [0.0, 0.0]),
        ],
    ),
    (
        [0, 1, 0],
        [
            ([0.0, 1.0, 1.0], [0.0, 1.0]),
            ([1.0, 1.0, 1.0], [1.0, 1.0]),
            ([1.0, 1.0, 0.0], [1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 0.0]),
        ],
    ),
    (
        [0, -1, 0],
        [
            ([0.0, 0.0, 0.0], [0.0, 1.0]),
            ([1.0, 0.0, 0.0], [1.0, 1.0]),
            ([1.0, 0.0, 1.0], [1.0, 0.0]),
            ([0.0, 0.0, 1.0], [0.0, 0.0]),
        ],
    ),
];

fn ambient_occlusion_value(occluders: u8) -> f32 {
    match occluders.min(3) {
        0 => 1.0,
        1 => 0.75,
        2 => 0.5,
        _ => 0.25,
    }
}

fn ao_sample_positions(
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

fn ambient_occlusion_for_vertex<F>(
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

fn quad_indices_for_ao(ao: [f32; 4]) -> [u32; 6] {
    if ao[0] + ao[2] > ao[1] + ao[3] {
        [0, 1, 3, 1, 2, 3]
    } else {
        [0, 1, 2, 0, 2, 3]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GreedyFace {
    block: BlockType,
    atlas_tile: (u32, u32),
    light_level: u16,
    ao_levels: [u8; 4],
}

impl GreedyFace {
    fn can_merge_with(self, other: Self) -> bool {
        self.ao_levels
            .iter()
            .all(|level| *level == self.ao_levels[0])
            && other
                .ao_levels
                .iter()
                .all(|level| *level == other.ao_levels[0])
            && self == other
    }

    fn ao(self) -> [f32; 4] {
        self.ao_levels.map(ambient_occlusion_value)
    }
}

fn ao_level(value: f32) -> u8 {
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

fn push_terrain_quad(
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

const TORCH_MIN: f32 = 7.0 / 16.0;
const TORCH_MAX: f32 = 9.0 / 16.0;
const TORCH_HEIGHT: f32 = 10.0 / 16.0;
const TORCH_ATLAS_TILE: (u32, u32) = (4, 2);
const REDSTONE_TORCH_ATLAS_TILE: (u32, u32) = (6, 2);

const CACTUS_MIN: f32 = 1.0 / 16.0;
const CACTUS_MAX: f32 = 15.0 / 16.0;
const END_PORTAL_FRAME_HEIGHT: f32 = 13.0 / 16.0;
const END_PORTAL_SURFACE_HEIGHT: f32 = 12.0 / 16.0;

// Tile-local UV rectangles with a half-texel inset. Side faces use the full
// flame/stem artwork, the cap uses the flame, and the base stretches the final
// stem texel across the otherwise unseen bottom face.
const TORCH_SIDE_UV: [f32; 4] = [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 13.5 / 16.0];
const TORCH_TOP_UV: [f32; 4] = [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 4.5 / 16.0];
const TORCH_BOTTOM_UV: [f32; 4] = [7.5 / 16.0, 13.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0];

fn append_torch_mesh(
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

fn append_cactus_mesh(
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

fn append_end_portal_frame_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    block: BlockType,
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
        let fallback_tile = block.get_face_tex_index(face_idx);
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

fn append_end_portal_surface(
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

fn face_should_render(
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

    if neighbor.properties().render_type == RenderType::Opaque {
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

fn append_box_mesh(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    origin: [f32; 3],
    bounds: ([f32; 3], [f32; 3]),
    sky_light: u8,
    block_light: u8,
    atlas_tile: (u32, u32),
    region_coord: (i32, i32),
) {
    let light_level = sky_light as f32 + block_light as f32 * 16.0;
    let (min, max) = bounds;

    for (_, (_, corner_data)) in BLOCK_FACES.iter().enumerate() {
        let mut positions = [[0.0; 3]; 4];
        let mut local_uvs = [[0.0; 2]; 4];

        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
            positions[corner_idx] = [
                origin[0] + if offset[0] == 0.0 { min[0] } else { max[0] },
                origin[1] + if offset[1] == 0.0 { min[1] } else { max[1] },
                origin[2] + if offset[2] == 0.0 { min[2] } else { max[2] },
            ];
            local_uvs[corner_idx] = [uv[0], uv[1]];
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

fn append_door_mesh(
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

fn append_trapdoor_mesh(
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

fn is_greedy_cube(block: BlockType) -> bool {
    block.properties().is_solid
        && !block.is_cross_model()
        && !matches!(
            block,
            BlockType::Water
                | BlockType::Lava
                | BlockType::SnowLayer
                | BlockType::OakDoor
                | BlockType::OakDoorOpen
                | BlockType::OakTrapdoor
                | BlockType::OakTrapdoorOpen
                | BlockType::Cactus
                | BlockType::EndPortalFrame
                | BlockType::EndPortalFrameFilled
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceCell {
    height: i32,
    block: BlockType,
    top_tile: (u32, u32),
    light_level: u16,
}

fn is_lod_surface(block: BlockType) -> bool {
    block != BlockType::Air
        && !block.is_cross_model()
        && (block.properties().is_solid
            || matches!(
                block,
                BlockType::Water | BlockType::Lava | BlockType::SnowLayer
            ))
}

impl Chunk {
    // Generate opaque/cutout and translucent terrain meshes. Full cube faces
    // use conservative greedy merging: material/light must match and AO must
    // be uniform so removing internal vertices cannot change shading.
    pub fn mesh_l0_volume<F>(
        origin: [i32; 3],
        extent: [usize; 3],
        get_voxel: F,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> MeshVoxel,
    {
        Self::mesh_l0_volume_with_registry(origin, extent, get_voxel, None)
    }

    pub fn mesh_l0_volume_with_registry<F>(
        origin: [i32; 3],
        extent: [usize; 3],
        get_voxel: F,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> MeshVoxel,
    {
        let get_block_at = |x: i32, y: i32, z: i32| {
            let v = get_voxel(x, y, z);
            (
                v.block,
                v.sky,
                v.block_light,
                v.raw_fluid & 7,
                v.raw_fluid & 8 != 0,
            )
        };
        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();

        let region_coord = crate::chunk_render::chunk_to_region_coord(
            origin[0] / CHUNK_WIDTH as i32,
            origin[2] / CHUNK_DEPTH as i32,
        );

        // Non-cubic geometry and non-solid decorative blocks retain the exact
        // per-block path. They cannot be combined into rectangular cube faces.
        for x in 0..extent[0] {
            for z in 0..extent[2] {
                for y in 0..extent[1] {
                    let voxel = get_voxel(
                        origin[0] + x as i32,
                        origin[1] + y as i32,
                        origin[2] + z as i32,
                    );
                    let block = voxel.block;
                    if block == BlockType::Air || is_greedy_cube(block) {
                        continue;
                    }

                    let world_x = origin[0] + x as i32;
                    let world_y = origin[1] + y as i32;
                    let world_z = origin[2] + z as i32;

                    let custom_mesh = if let Some(registry) = registry {
                        crate::block_model::append_custom_block_mesh_with_registry(
                            block,
                            voxel.state,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            region_coord,
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            &mut trans_vertices,
                            &mut trans_indices,
                            crate::block_model::model_path_for_block(block),
                            registry,
                            |nx, ny, nz| get_block_at(nx, ny, nz).0,
                        )
                    } else {
                        crate::block_model::append_custom_block_mesh(
                            block,
                            voxel.state,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            region_coord,
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            &mut trans_vertices,
                            &mut trans_indices,
                            |nx, ny, nz| get_block_at(nx, ny, nz).0,
                        )
                    };
                    if custom_mesh {
                        if block.is_waterloggable() && voxel.raw_fluid & FLUID_WATERLOGGED_BIT != 0
                        {
                            crate::block_model::append_waterlogged_slab_mesh(
                                block,
                                voxel.state,
                                [world_x as f32, world_y as f32, world_z as f32],
                                voxel.sky,
                                voxel.block_light,
                                region_coord,
                                &mut trans_vertices,
                                &mut trans_indices,
                                registry.map(|registry| {
                                    registry.atlas_tile_for_block(
                                        BlockType::Water,
                                        BlockType::Water.get_face_tex_index(0),
                                    )
                                }),
                            );
                        }
                        continue;
                    }

                    let torch_atlas_tile = match block {
                        BlockType::Torch => Some(registry.map_or(TORCH_ATLAS_TILE, |registry| {
                            registry.atlas_tile_for_block(block, TORCH_ATLAS_TILE)
                        })),
                        BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => {
                            Some(registry.map_or(REDSTONE_TORCH_ATLAS_TILE, |registry| {
                                registry.atlas_tile_for_block(block, REDSTONE_TORCH_ATLAS_TILE)
                            }))
                        }
                        _ => None,
                    };
                    if let Some(atlas_tile) = torch_atlas_tile {
                        append_torch_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            atlas_tile,
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(block, BlockType::OakDoor | BlockType::OakDoorOpen) {
                        let state = BlockState::decode(voxel.state);
                        append_door_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            state,
                            voxel.sky,
                            voxel.block_light,
                            registry.map_or((9, 14), |registry| {
                                registry.atlas_tile_for_block(block, (9, 14))
                            }),
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(block, BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen) {
                        let state = BlockState::decode(voxel.state);
                        append_trapdoor_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            state,
                            voxel.sky,
                            voxel.block_light,
                            registry.map_or((10, 14), |registry| {
                                registry.atlas_tile_for_block(block, (10, 14))
                            }),
                            region_coord,
                        );
                        continue;
                    }

                    if block == BlockType::Cactus {
                        append_cactus_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            registry.map_or((11, 12), |registry| {
                                registry.atlas_tile_for_block(block, (11, 12))
                            }),
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(
                        block,
                        BlockType::EndPortalFrame | BlockType::EndPortalFrameFilled
                    ) {
                        append_end_portal_frame_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            block,
                            voxel.sky,
                            voxel.block_light,
                            region_coord,
                            registry,
                        );
                        continue;
                    }

                    if block == BlockType::EndPortal {
                        append_end_portal_surface(
                            &mut trans_vertices,
                            &mut trans_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            region_coord,
                            registry,
                        );
                        continue;
                    }

                    if block.is_cross_model() {
                        let sky_val = voxel.sky;
                        let block_val = voxel.block_light;
                        let light_val = sky_val as f32 + block_val as f32 * 16.0 + 1.0 * 256.0;

                        let fallback_tile = block.get_face_tex_index(0);
                        let atlas_tile = registry.map_or(fallback_tile, |registry| {
                            registry.atlas_tile_for_block(block, fallback_tile)
                        });

                        let wx = world_x as f32;
                        let wy = world_y as f32;
                        let wz = world_z as f32;

                        let min_off = 0.1464466;
                        let max_off = 0.8535534;

                        let plane1_p0 = [wx + min_off, wy, wz + min_off];
                        let plane1_p1 = [wx + max_off, wy, wz + max_off];
                        let plane1_p2 = [wx + max_off, wy + 1.0, wz + max_off];
                        let plane1_p3 = [wx + min_off, wy + 1.0, wz + min_off];

                        let plane2_p0 = [wx + max_off, wy, wz + min_off];
                        let plane2_p1 = [wx + min_off, wy, wz + max_off];
                        let plane2_p2 = [wx + min_off, wy + 1.0, wz + max_off];
                        let plane2_p3 = [wx + max_off, wy + 1.0, wz + min_off];

                        let planes = [
                            (plane1_p0, plane1_p1, plane1_p2, plane1_p3),
                            (plane2_p0, plane2_p1, plane2_p2, plane2_p3),
                        ];

                        for (p0, p1, p2, p3) in planes {
                            push_terrain_quad(
                                &mut opaque_vertices,
                                &mut opaque_indices,
                                [p0, p1, p2, p3],
                                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                                atlas_tile,
                                light_val,
                                [1.0; 4],
                                region_coord,
                            );
                            push_terrain_quad(
                                &mut opaque_vertices,
                                &mut opaque_indices,
                                [p1, p0, p3, p2],
                                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                                atlas_tile,
                                light_val,
                                [1.0; 4],
                                region_coord,
                            );
                        }

                        continue;
                    }

                    for (face_idx, (normal, corner_data)) in BLOCK_FACES.iter().enumerate() {
                        let nx = world_x + normal[0];
                        let ny = world_y + normal[1];
                        let nz = world_z + normal[2];

                        let (
                            neighbor,
                            neighbor_sky,
                            neighbor_block,
                            neighbor_level,
                            neighbor_falling,
                        ) = get_block_at(nx, ny, nz);
                        let is_fluid = block == BlockType::Water || block == BlockType::Lava;
                        let fl_raw = voxel.raw_fluid;
                        let level = fl_raw & 0x07;
                        let falling = (fl_raw & 0x08) != 0;

                        if face_should_render(
                            block,
                            face_idx,
                            level,
                            falling,
                            neighbor,
                            neighbor_level,
                            neighbor_falling,
                        ) {
                            let block_render_type = block.properties().render_type;
                            let is_translucent = block_render_type == RenderType::Translucent;

                            let (v_list, i_list) = if is_translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };

                            let fallback_tile = block.get_face_tex_index(face_idx);
                            let atlas_tile = registry.map_or(fallback_tile, |registry| {
                                registry.atlas_tile_for_block(block, fallback_tile)
                            });

                            let multiplier_code = match face_idx {
                                4 => 0.0, // Top
                                5 => 2.0, // Bottom
                                _ => 1.0, // Sides
                            };
                            let light_val = if block == BlockType::Lava {
                                15.0 * 16.0 + 15.0 + multiplier_code * 256.0
                            } else {
                                (neighbor_sky as f32)
                                    + (neighbor_block as f32) * 16.0
                                    + multiplier_code * 256.0
                            };

                            let h = if is_fluid {
                                if falling {
                                    1.0
                                } else {
                                    (8 - level) as f32 / 8.0 * 0.9
                                }
                            } else if block == BlockType::SnowLayer {
                                0.125
                            } else {
                                1.0
                            };

                            let mut ao = [1.0; 4];
                            for (corner_idx, (offset, _)) in corner_data.iter().enumerate() {
                                ao[corner_idx] = ambient_occlusion_for_vertex(
                                    [world_x, world_y, world_z],
                                    *normal,
                                    *offset,
                                    &get_block_at,
                                );
                            }

                            let mut positions = [[0.0; 3]; 4];
                            let mut local_uvs = [[0.0; 2]; 4];
                            for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
                                let mut vy = world_y as f32 + offset[1];
                                if (is_fluid || block == BlockType::SnowLayer) && offset[1] > 0.0 {
                                    vy = world_y as f32 + h;
                                }

                                positions[corner_idx] =
                                    [world_x as f32 + offset[0], vy, world_z as f32 + offset[2]];
                                local_uvs[corner_idx] = *uv;
                            }
                            push_terrain_quad(
                                v_list,
                                i_list,
                                positions,
                                local_uvs,
                                atlas_tile,
                                light_val,
                                ao,
                                region_coord,
                            );
                        }
                    }
                }
            }
        }

        // Full cube faces are processed one direction/slice at a time. Each
        // mask cell describes one visible face. Rectangles only grow across
        // identical material/light and uniform AO.
        let dimensions = extent;
        for (face_idx, (normal, corner_data)) in BLOCK_FACES.iter().enumerate() {
            let normal_axis = (0..3).find(|axis| normal[*axis] != 0).unwrap();
            let u_axis = (0..3)
                .find(|axis| corner_data[0].0[*axis] != corner_data[1].0[*axis])
                .unwrap();
            let v_axis = (0..3)
                .find(|axis| corner_data[0].0[*axis] != corner_data[3].0[*axis])
                .unwrap();
            let u_len = dimensions[u_axis];
            let v_len = dimensions[v_axis];

            for slice in 0..dimensions[normal_axis] {
                let mut mask = vec![None::<GreedyFace>; u_len * v_len];
                for v in 0..v_len {
                    for u in 0..u_len {
                        let mut local = [0usize; 3];
                        local[normal_axis] = slice;
                        local[u_axis] = u;
                        local[v_axis] = v;
                        let [x, y, z] = local;

                        let voxel = get_voxel(
                            origin[0] + x as i32,
                            origin[1] + y as i32,
                            origin[2] + z as i32,
                        );
                        let block = voxel.block;
                        if !is_greedy_cube(block) {
                            continue;
                        }

                        let world = [
                            origin[0] + x as i32,
                            origin[1] + y as i32,
                            origin[2] + z as i32,
                        ];
                        let nx = world[0] + normal[0];
                        let ny = world[1] + normal[1];
                        let nz = world[2] + normal[2];
                        let (
                            neighbor,
                            neighbor_sky,
                            neighbor_block,
                            neighbor_level,
                            neighbor_falling,
                        ) = get_block_at(nx, ny, nz);
                        if !face_should_render(
                            block,
                            face_idx,
                            0,
                            false,
                            neighbor,
                            neighbor_level,
                            neighbor_falling,
                        ) {
                            continue;
                        }

                        let multiplier_code = match face_idx {
                            4 => 0u16,
                            5 => 2u16,
                            _ => 1u16,
                        };
                        let light_level = neighbor_sky as u16
                            + neighbor_block as u16 * 16
                            + multiplier_code * 256;
                        let mut ao_levels = [0u8; 4];
                        for (corner_idx, (offset, _)) in corner_data.iter().enumerate() {
                            ao_levels[corner_idx] = ao_level(ambient_occlusion_for_vertex(
                                world,
                                *normal,
                                *offset,
                                &get_block_at,
                            ));
                        }

                        let fallback_tile = block.get_face_tex_index(face_idx);
                        let (tile_x, tile_y) = registry.map_or(fallback_tile, |registry| {
                            registry.atlas_tile_for_block(block, fallback_tile)
                        });
                        mask[v * u_len + u] = Some(GreedyFace {
                            block,
                            atlas_tile: (tile_x, tile_y),
                            light_level,
                            ao_levels,
                        });
                    }
                }

                for v in 0..v_len {
                    let mut u = 0;
                    while u < u_len {
                        let index = v * u_len + u;
                        let Some(face) = mask[index] else {
                            u += 1;
                            continue;
                        };

                        let mut width = 1;
                        if face
                            .ao_levels
                            .iter()
                            .all(|level| *level == face.ao_levels[0])
                        {
                            while u + width < u_len
                                && mask[v * u_len + u + width]
                                    .is_some_and(|other| face.can_merge_with(other))
                            {
                                width += 1;
                            }
                        }

                        let mut height = 1;
                        'grow_height: while v + height < v_len {
                            for offset in 0..width {
                                if !mask[(v + height) * u_len + u + offset]
                                    .is_some_and(|other| face.can_merge_with(other))
                                {
                                    break 'grow_height;
                                }
                            }
                            height += 1;
                        }

                        for row in 0..height {
                            for column in 0..width {
                                mask[(v + row) * u_len + u + column] = None;
                            }
                        }

                        let mut min_local = [0.0f32; 3];
                        min_local[normal_axis] = slice as f32;
                        min_local[u_axis] = u as f32;
                        min_local[v_axis] = v as f32;
                        let mut max_local =
                            [min_local[0] + 1.0, min_local[1] + 1.0, min_local[2] + 1.0];
                        max_local[u_axis] = min_local[u_axis] + width as f32;
                        max_local[v_axis] = min_local[v_axis] + height as f32;

                        let world_origin = [origin[0] as f32, origin[1] as f32, origin[2] as f32];
                        let mut positions = [[0.0f32; 3]; 4];
                        let mut local_uvs = [[0.0f32; 2]; 4];
                        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
                            for axis in 0..3 {
                                positions[corner_idx][axis] = world_origin[axis]
                                    + if offset[axis] == 0.0 {
                                        min_local[axis]
                                    } else {
                                        max_local[axis]
                                    };
                            }
                            local_uvs[corner_idx] = [uv[0] * width as f32, uv[1] * height as f32];
                        }

                        let (vertices, indices) =
                            if face.block.properties().render_type == RenderType::Translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };
                        push_terrain_quad(
                            vertices,
                            indices,
                            positions,
                            local_uvs,
                            face.atlas_tile,
                            face.light_level as f32,
                            face.ao(),
                            region_coord,
                        );
                        u += width;
                    }
                }
            }
        }

        (
            opaque_vertices,
            opaque_indices,
            trans_vertices,
            trans_indices,
        )
    }

    pub fn generate_mesh<F>(
        &self,
        get_block_at: F,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
    {
        self.generate_mesh_inner(get_block_at, None)
    }

    /// Generates a complete chunk mesh using immutable resource-pack model
    /// descriptors. The legacy entry point above intentionally retains the
    /// procedural atlas mapping for callers without a selected pack.
    pub fn generate_mesh_with_registry<F>(
        &self,
        get_block_at: F,
        registry: &crate::block_model::ModelRegistry,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
    {
        self.generate_mesh_inner(get_block_at, Some(registry))
    }

    fn generate_mesh_inner<F>(
        &self,
        get_block_at: F,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
    {
        let min_y = self.min_section_y as i32 * 16;
        let total_height = self.sections.len() * 16;
        let origin = [
            self.chunk_x * CHUNK_WIDTH as i32,
            min_y,
            self.chunk_z * CHUNK_DEPTH as i32,
        ];
        let max_y = min_y + total_height as i32;
        Self::mesh_l0_volume_with_registry(
            origin,
            [CHUNK_WIDTH, total_height, CHUNK_DEPTH],
            |x, y, z| {
                let (lookup_block, sky, block_light, level, falling) = get_block_at(x, y, z);
                let in_chunk = x.div_euclid(CHUNK_WIDTH as i32) == self.chunk_x
                    && z.div_euclid(CHUNK_DEPTH as i32) == self.chunk_z
                    && y >= min_y
                    && y < max_y;
                let block = if in_chunk {
                    self.get_block_local(
                        x.rem_euclid(CHUNK_WIDTH as i32) as usize,
                        y,
                        z.rem_euclid(CHUNK_DEPTH as i32) as usize,
                    )
                } else {
                    lookup_block
                };
                MeshVoxel {
                    block,
                    state: self.get_block_state(x - origin[0], y, z - origin[2]),
                    sky,
                    block_light,
                    raw_fluid: level | if falling { 8 } else { 0 },
                }
            },
            registry,
        )
    }

    pub fn generate_mesh_bundle<F>(&self, get_block_at: F) -> ChunkMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        self.generate_mesh_bundle_inner(get_block_at, None)
    }

    /// Builds all chunk LODs while applying the selected model registry.
    pub fn generate_mesh_bundle_with_registry<F>(
        &self,
        get_block_at: F,
        registry: &crate::block_model::ModelRegistry,
    ) -> ChunkMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        self.generate_mesh_bundle_inner(get_block_at, Some(registry))
    }

    fn generate_mesh_bundle_inner<F>(
        &self,
        get_block_at: F,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> ChunkMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        let region_coord = crate::chunk_render::chunk_to_region_coord(self.chunk_x, self.chunk_z);
        let (o0, oi0, t0, ti0) = self.generate_mesh_inner(get_block_at, registry);
        let l1 = self.generate_surface_mesh_with_registry(get_block_at, 1, registry);
        let l2 = self.generate_surface_mesh_with_registry(get_block_at, 4, registry);
        let mut section_connectivity =
            vec![crate::culling::SectionConnectivity::FULL; self.sections.len()];
        for sec_idx in 0..self.sections.len() {
            let sec_y = self.section_y_at_index(sec_idx);
            section_connectivity[sec_idx] =
                crate::culling::compute_section_connectivity(self, sec_y);
        }
        ChunkMeshBundle {
            levels: [
                ChunkLodMeshData::from_parts(o0, oi0, t0, ti0, region_coord),
                l1,
                l2,
            ],
            section_connectivity,
        }
    }

    /// Generates meshes for one section only. Blocks outside the requested
    /// 16-block Y interval are blanked in the meshing copy while the supplied
    /// lookup remains world-backed, preserving the one-cell halo semantics.
    pub fn generate_section_mesh_bundle<F>(
        &self,
        key: SectionKey,
        revision: u64,
        lifetime: u64,
        get_block_at: F,
    ) -> crate::chunk_render::SectionMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        self.generate_section_mesh_bundle_inner(key, revision, lifetime, get_block_at, None)
    }

    /// Generates one section and its coarse LODs using immutable model-pack
    /// descriptors. The halo remains captured exactly once before dispatch.
    pub fn generate_section_mesh_bundle_with_registry<F>(
        &self,
        key: SectionKey,
        revision: u64,
        lifetime: u64,
        get_block_at: F,
        registry: &crate::block_model::ModelRegistry,
    ) -> crate::chunk_render::SectionMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        self.generate_section_mesh_bundle_inner(
            key,
            revision,
            lifetime,
            get_block_at,
            Some(registry),
        )
    }

    fn generate_section_mesh_bundle_inner<F>(
        &self,
        key: SectionKey,
        revision: u64,
        lifetime: u64,
        get_block_at: F,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> crate::chunk_render::SectionMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        assert_eq!((self.chunk_x, self.chunk_z), (key.cx, key.cz));
        assert!(self.section_index(key.section_y).is_some());
        // Materialize the immutable 18^3 halo up front. The worker lookup
        // below consults this snapshot for all block-occlusion decisions,
        // ensuring boundary/AO results are independent of later mutations.
        let halo = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
            if wx.div_euclid(CHUNK_WIDTH as i32) == key.cx
                && wz.div_euclid(CHUNK_DEPTH as i32) == key.cz
                && self.section_index(world_y_to_section_y(wy)).is_some()
            {
                let x = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
                let z = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
                MeshVoxel {
                    block: self.get_block_local(x, wy, z),
                    state: self.get_block_state(
                        wx - key.cx * CHUNK_WIDTH as i32,
                        wy,
                        wz - key.cz * CHUNK_DEPTH as i32,
                    ),
                    sky: self.get_sky_light(x, wy, z),
                    block_light: self.get_block_light(x, wy, z),
                    raw_fluid: self.get_fluid_level(x, wy, z),
                }
            } else {
                let (block, sky, block_light, level, falling) = get_block_at(wx, wy, wz);
                MeshVoxel {
                    block,
                    sky,
                    block_light,
                    raw_fluid: level | if falling { 8 } else { 0 },
                    ..MeshVoxel::default()
                }
            }
        });
        Self::generate_section_mesh_bundle_from_halo_inner(
            SectionIdentity::new(key, revision, lifetime),
            &halo,
            registry,
        )
    }

    /// Builds a section mesh exclusively from the immutable 18^3 worker
    /// snapshot. This is the runtime entry point; no live Chunk/ChunkManager
    /// state is consulted after dispatch.
    pub fn generate_section_mesh_bundle_from_halo(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
    ) -> crate::chunk_render::SectionMeshBundle {
        Self::generate_section_mesh_bundle_from_halo_inner(identity, halo, None)
    }

    /// Worker-safe section mesh entry point with an immutable model registry.
    pub fn generate_section_mesh_bundle_from_halo_with_registry(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
        registry: &crate::block_model::ModelRegistry,
    ) -> crate::chunk_render::SectionMeshBundle {
        Self::generate_section_mesh_bundle_from_halo_inner(identity, halo, Some(registry))
    }

    fn generate_section_mesh_bundle_from_halo_inner(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> crate::chunk_render::SectionMeshBundle {
        let key = identity.key;
        debug_assert_eq!(halo.key, key);
        let section_voxel = |wx: i32, wy: i32, wz: i32| {
            let hx = wx - key.cx * CHUNK_WIDTH as i32 + 1;
            let hy = wy - key.min_world_y() + 1;
            let hz = wz - key.cz * CHUNK_DEPTH as i32 + 1;
            if (0..SectionHaloSnapshot::SIDE as i32).contains(&hx)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hy)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hz)
            {
                let voxel = halo.get(hx as usize, hy as usize, hz as usize);
                return voxel;
            }
            // Coordinates outside the captured halo are never queried by the
            // section worker. Keep a deterministic sentinel for defensive use.
            MeshVoxel::default()
        };
        let origin = [
            key.cx * CHUNK_WIDTH as i32,
            key.min_world_y(),
            key.cz * CHUNK_DEPTH as i32,
        ];
        let (o, oi, t, ti) = Self::mesh_l0_volume_with_registry(
            origin,
            [CHUNK_WIDTH, SECTION_SIZE, CHUNK_DEPTH],
            section_voxel,
            registry,
        );
        let region_coord = crate::chunk_render::chunk_to_region_coord(key.cx, key.cz);
        let l0 = ChunkLodMeshData::from_parts(o, oi, t, ti, region_coord);
        let l1 = Self::mesh_section_lod_from_halo_with_registry(key, halo, 2, registry);
        let l2 = Self::mesh_section_lod_from_halo_with_registry(key, halo, 4, registry);
        let levels = [l0, l1, l2];
        let bounds = levels
            .iter()
            .filter_map(ChunkLodMeshData::bounds)
            .reduce(|a, b| a.union(b));
        crate::chunk_render::SectionMeshBundle {
            identity,
            levels,
            bounds,
            connectivity: crate::culling::compute_section_connectivity_snapshot(halo),
        }
    }

    fn mesh_section_lod_from_halo(
        key: SectionKey,
        halo: &SectionHaloSnapshot,
        step: usize,
    ) -> ChunkLodMeshData {
        Self::mesh_section_lod_from_halo_with_registry(key, halo, step, None)
    }

    fn mesh_section_lod_from_halo_with_registry(
        key: SectionKey,
        halo: &SectionHaloSnapshot,
        step: usize,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> ChunkLodMeshData {
        debug_assert!(step > 1 && SECTION_SIZE % step == 0);
        let mut coarse = [MeshVoxel::default(); SECTION_VOLUME];

        for cell_y in (0..SECTION_SIZE).step_by(step) {
            for cell_z in (0..CHUNK_DEPTH).step_by(step) {
                for cell_x in (0..CHUNK_WIDTH).step_by(step) {
                    let mut representative = MeshVoxel::default();
                    'sample: for dy in 0..step {
                        for dz in 0..step {
                            for dx in 0..step {
                                let voxel =
                                    halo.get(cell_x + dx + 1, cell_y + dy + 1, cell_z + dz + 1);
                                if voxel.block != BlockType::Air {
                                    representative = voxel;
                                    break 'sample;
                                }
                            }
                        }
                    }
                    if representative.block == BlockType::Air {
                        continue;
                    }
                    for dy in 0..step {
                        for dz in 0..step {
                            for dx in 0..step {
                                let x = cell_x + dx;
                                let y = cell_y + dy;
                                let z = cell_z + dz;
                                coarse[(y * CHUNK_DEPTH + z) * CHUNK_WIDTH + x] = representative;
                            }
                        }
                    }
                }
            }
        }

        let origin = [
            key.cx * CHUNK_WIDTH as i32,
            key.min_world_y(),
            key.cz * CHUNK_DEPTH as i32,
        ];
        let voxel = |wx: i32, wy: i32, wz: i32| {
            let x = wx - origin[0];
            let y = wy - origin[1];
            let z = wz - origin[2];
            if (0..CHUNK_WIDTH as i32).contains(&x)
                && (0..SECTION_SIZE as i32).contains(&y)
                && (0..CHUNK_DEPTH as i32).contains(&z)
            {
                return coarse[(y as usize * CHUNK_DEPTH + z as usize) * CHUNK_WIDTH + x as usize];
            }
            let hx = x + 1;
            let hy = y + 1;
            let hz = z + 1;
            if (0..SectionHaloSnapshot::SIDE as i32).contains(&hx)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hy)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hz)
            {
                halo.get(hx as usize, hy as usize, hz as usize)
            } else {
                MeshVoxel::default()
            }
        };
        let (opaque, opaque_indices, transparent, transparent_indices) =
            Self::mesh_l0_volume_with_registry(
                origin,
                [CHUNK_WIDTH, SECTION_SIZE, CHUNK_DEPTH],
                voxel,
                registry,
            );
        ChunkLodMeshData::from_parts(
            opaque,
            opaque_indices,
            transparent,
            transparent_indices,
            crate::chunk_render::chunk_to_region_coord(key.cx, key.cz),
        )
    }

    fn generate_surface_mesh<F>(&self, get_block_at: F, step: usize) -> ChunkLodMeshData
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        self.generate_surface_mesh_with_registry(get_block_at, step, None)
    }

    fn generate_surface_mesh_with_registry<F>(
        &self,
        get_block_at: F,
        step: usize,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> ChunkLodMeshData
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        let region_coord = crate::chunk_render::chunk_to_region_coord(self.chunk_x, self.chunk_z);
        debug_assert!(step > 0 && CHUNK_WIDTH % step == 0 && CHUNK_DEPTH % step == 0);
        let grid_width = CHUNK_WIDTH / step;
        let grid_depth = CHUNK_DEPTH / step;
        let mut cells = vec![None::<SurfaceCell>; grid_width * grid_depth];

        for gz in 0..grid_depth {
            for gx in 0..grid_width {
                let mut best: Option<(usize, i32, usize, BlockType)> = None;
                for dz in 0..step {
                    for dx in 0..step {
                        let x = gx * step + dx;
                        let z = gz * step + dz;
                        let h = self.heightmap[x][z];
                        if h == NO_HEIGHT {
                            continue;
                        }
                        let mut y = h as i32;
                        let min_y = self.min_section_y as i32 * 16;
                        loop {
                            let block = self.get_block_local(x, y, z);
                            if is_lod_surface(block) {
                                if best.map_or(true, |(_, best_y, _, _)| y > best_y) {
                                    best = Some((x, y, z, block));
                                }
                                break;
                            }
                            if y <= min_y {
                                break;
                            }
                            y -= 1;
                        }
                    }
                }

                let Some((x, y, z, block)) = best else {
                    continue;
                };
                let world_x = self.chunk_x * CHUNK_WIDTH as i32 + x as i32;
                let world_z = self.chunk_z * CHUNK_DEPTH as i32 + z as i32;
                let (_, sky, block_light, _, _) = get_block_at(world_x, y + 1, world_z);
                let fallback_tile = block.get_face_tex_index(4);
                let top_tile = registry.map_or(fallback_tile, |registry| {
                    registry.atlas_tile_for_block(block, fallback_tile)
                });
                cells[gz * grid_width + gx] = Some(SurfaceCell {
                    height: y,
                    block,
                    top_tile,
                    light_level: sky as u16 + block_light as u16 * 16,
                });
            }
        }

        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();
        let world_x0 = (self.chunk_x * CHUNK_WIDTH as i32) as f32;
        let world_z0 = (self.chunk_z * CHUNK_DEPTH as i32) as f32;

        // Greedily merge equal top surface cells.
        let mut top_mask = cells.clone();
        for gz in 0..grid_depth {
            let mut gx = 0;
            while gx < grid_width {
                let index = gz * grid_width + gx;
                let Some(cell) = top_mask[index] else {
                    gx += 1;
                    continue;
                };
                let mut width = 1;
                while gx + width < grid_width
                    && top_mask[gz * grid_width + gx + width] == Some(cell)
                {
                    width += 1;
                }
                let mut depth = 1;
                'grow_depth: while gz + depth < grid_depth {
                    for offset in 0..width {
                        if top_mask[(gz + depth) * grid_width + gx + offset] != Some(cell) {
                            break 'grow_depth;
                        }
                    }
                    depth += 1;
                }
                for row in 0..depth {
                    for column in 0..width {
                        top_mask[(gz + row) * grid_width + gx + column] = None;
                    }
                }

                let x0 = world_x0 + (gx * step) as f32;
                let x1 = world_x0 + ((gx + width) * step) as f32;
                let z0 = world_z0 + (gz * step) as f32;
                let z1 = world_z0 + ((gz + depth) * step) as f32;
                let y = cell.height as f32 + 1.0;
                let (vertices, indices) =
                    if cell.block.properties().render_type == RenderType::Translucent {
                        (&mut trans_vertices, &mut trans_indices)
                    } else {
                        (&mut opaque_vertices, &mut opaque_indices)
                    };
                push_terrain_quad(
                    vertices,
                    indices,
                    [[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]],
                    [
                        [0.0, (depth * step) as f32],
                        [(width * step) as f32, (depth * step) as f32],
                        [(width * step) as f32, 0.0],
                        [0.0, 0.0],
                    ],
                    cell.top_tile,
                    cell.light_level as f32,
                    [1.0; 4],
                    region_coord,
                );
                gx += width;
            }
        }

        // Add vertical skirts wherever a coarse cell is higher than its
        // neighbor. Adjacent equal skirts are merged along their tangent axis
        // so a flat 16x16 surface remains five quads instead of 65.
        let side_at = |face_idx: usize, gx: usize, gz: usize| {
            let cell = cells[gz * grid_width + gx]?;
            let neighbor = match face_idx {
                0 => (gx as i32, gz as i32 + 1),
                1 => (gx as i32, gz as i32 - 1),
                2 => (gx as i32 - 1, gz as i32),
                _ => (gx as i32 + 1, gz as i32),
            };
            let neighbor_height = if neighbor.0 >= 0
                && neighbor.0 < grid_width as i32
                && neighbor.1 >= 0
                && neighbor.1 < grid_depth as i32
            {
                cells[neighbor.1 as usize * grid_width + neighbor.0 as usize]
                    .map(|neighbor| neighbor.height)
                    .unwrap_or(-1)
            } else {
                -1
            };
            (neighbor_height < cell.height).then_some((cell, neighbor_height))
        };

        for face_idx in 0..4 {
            let (line_count, line_length) = if face_idx < 2 {
                (grid_depth, grid_width)
            } else {
                (grid_width, grid_depth)
            };
            for line in 0..line_count {
                let mut cursor = 0;
                while cursor < line_length {
                    let (gx, gz) = if face_idx < 2 {
                        (cursor, line)
                    } else {
                        (line, cursor)
                    };
                    let Some(side) = side_at(face_idx, gx, gz) else {
                        cursor += 1;
                        continue;
                    };
                    let mut run = 1;
                    while cursor + run < line_length {
                        let (next_gx, next_gz) = if face_idx < 2 {
                            (cursor + run, line)
                        } else {
                            (line, cursor + run)
                        };
                        if side_at(face_idx, next_gx, next_gz) != Some(side) {
                            break;
                        }
                        run += 1;
                    }

                    let (cell, neighbor_height) = side;
                    let top = cell.height as f32 + 1.0;
                    let bottom = neighbor_height as f32 + 1.0;
                    let run_blocks = (run * step) as f32;
                    let x0 = world_x0 + (gx * step) as f32;
                    let z0 = world_z0 + (gz * step) as f32;
                    let positions = match face_idx {
                        0 => {
                            let x1 = x0 + run_blocks;
                            let z1 = z0 + step as f32;
                            [
                                [x0, bottom, z1],
                                [x1, bottom, z1],
                                [x1, top, z1],
                                [x0, top, z1],
                            ]
                        }
                        1 => {
                            let x1 = x0 + run_blocks;
                            [
                                [x1, bottom, z0],
                                [x0, bottom, z0],
                                [x0, top, z0],
                                [x1, top, z0],
                            ]
                        }
                        2 => {
                            let z1 = z0 + run_blocks;
                            [
                                [x0, bottom, z0],
                                [x0, bottom, z1],
                                [x0, top, z1],
                                [x0, top, z0],
                            ]
                        }
                        _ => {
                            let x1 = x0 + step as f32;
                            let z1 = z0 + run_blocks;
                            [
                                [x1, bottom, z1],
                                [x1, bottom, z0],
                                [x1, top, z0],
                                [x1, top, z1],
                            ]
                        }
                    };
                    let fallback_tile = cell.block.get_face_tex_index(face_idx);
                    let side_tile = registry.map_or(fallback_tile, |registry| {
                        registry.atlas_tile_for_block(cell.block, fallback_tile)
                    });
                    let (vertices, indices) =
                        if cell.block.properties().render_type == RenderType::Translucent {
                            (&mut trans_vertices, &mut trans_indices)
                        } else {
                            (&mut opaque_vertices, &mut opaque_indices)
                        };
                    push_terrain_quad(
                        vertices,
                        indices,
                        positions,
                        [
                            [0.0, top - bottom],
                            [run_blocks, top - bottom],
                            [run_blocks, 0.0],
                            [0.0, 0.0],
                        ],
                        side_tile,
                        cell.light_level as f32 + 256.0,
                        [1.0; 4],
                        region_coord,
                    );
                    cursor += run;
                }
            }
        }

        ChunkLodMeshData::from_parts(
            opaque_vertices,
            opaque_indices,
            trans_vertices,
            trans_indices,
            region_coord,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_MODEL_TEMP: AtomicU64 = AtomicU64::new(0);

    fn model_temp_dir(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = NEXT_MODEL_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("icraft_world_model_{label}_{stamp}_{id}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn selected_world_model_registry() -> (std::path::PathBuf, crate::block_model::ModelRegistry) {
        let root = model_temp_dir("selected");
        fs::write(
            root.join("pack.json"),
            br#"{"id":"icraft.builtin","name":"builtin","version":"1","format":1,"description":""}"#,
        )
        .unwrap();
        let user = root.join("resourcepacks");
        let pack = user.join("mesh.pack");
        fs::create_dir_all(pack.join("models/block")).unwrap();
        fs::write(
            pack.join("pack.json"),
            br#"{"id":"mesh.pack","name":"mesh","version":"1","format":1,"description":""}"#,
        )
        .unwrap();
        fs::write(
            pack.join("models/block/stone.json"),
            br#"{"parent":"builtin","atlas_tile":[13,13]}"#,
        )
        .unwrap();
        fs::write(
            pack.join("models/block/oak_slab.json"),
            br#"{"parent":"builtin","atlas_tile":[12,12]}"#,
        )
        .unwrap();

        let mut manager = crate::resources::ResourcePackManager::discover(&root, &user);
        manager.apply_enabled_order(["mesh.pack"]).unwrap();
        let registry = crate::block_model::ModelRegistry::from_resource_packs(
            &mut manager,
            [
                crate::block_model::model_path_for_block(BlockType::Stone),
                crate::block_model::model_path_for_block(BlockType::OakSlab),
            ],
        );
        (root, registry)
    }

    #[test]
    fn end_portal_frames_use_distinct_top_side_and_filled_top_tiles() {
        for block in [BlockType::EndPortalFrame, BlockType::EndPortalFrameFilled] {
            assert_eq!(block.get_face_tex_index(0), (9, 4));
            assert_eq!(block.get_face_tex_index(5), (9, 4));
        }
        assert_eq!(BlockType::EndPortalFrame.get_face_tex_index(4), (15, 15));
        assert_eq!(
            BlockType::EndPortalFrameFilled.get_face_tex_index(4),
            (6, 4)
        );
    }

    fn empty_test_chunk() -> Chunk {
        let mut chunk = Chunk::new(0, 0);
        let min_y = chunk.min_section_y as i32 * 16;
        let max_y = min_y + (chunk.sections.len() as i32) * 16;
        for x in 0..CHUNK_WIDTH {
            for y in min_y..max_y {
                for z in 0..CHUNK_DEPTH {
                    chunk.set_block_local(x, y, z, BlockType::Air);
                    chunk.set_sky_light(x, y, z, 15);
                    chunk.set_block_light(x, y, z, 0);
                    chunk.set_fluid_level(x, y, z, 0);
                }
            }
            for z in 0..CHUNK_DEPTH {
                chunk.heightmap[x][z] = NO_HEIGHT;
            }
        }
        chunk
    }

    #[test]
    fn l0_volume_respects_section_extent_bounds() {
        let origin = [0, 32, 0];
        let (ov, _, tv, _) = Chunk::mesh_l0_volume(origin, [16, 16, 16], |x, y, z| MeshVoxel {
            block: if (x, y, z) == (0, 32, 0) {
                BlockType::Stone
            } else {
                BlockType::Air
            },
            state: 0,
            sky: 15,
            block_light: 0,
            raw_fluid: 0,
        });
        assert!(!ov.is_empty(), "block inside extent must generate quads");
        assert!(tv.is_empty());
    }

    #[test]
    fn production_mesh_entries_apply_selected_tiles_and_keep_default_fallback() {
        let (_temp_root, registry) = selected_world_model_registry();
        let mut chunk = empty_test_chunk();
        chunk.set_block_local(8, 64, 8, BlockType::Stone);
        chunk.set_block_local(9, 64, 8, BlockType::Dirt);
        let key = SectionKey::new(0, 4, 0);

        let bundle_default = chunk
            .generate_section_mesh_bundle(key, 1, 1, |x, y, z| test_chunk_lookup(&chunk, x, y, z));
        let default_stone_tile = BlockType::Stone.get_face_tex_index(0);
        let default_dirt_tile = BlockType::Dirt.get_face_tex_index(0);
        let default_vertices = &bundle_default.levels[0].opaque.vertices;
        assert!(default_vertices
            .iter()
            .any(|v| v.atlas_tile_u32() == default_stone_tile));
        assert!(default_vertices
            .iter()
            .any(|v| v.atlas_tile_u32() == default_dirt_tile));

        let bundle_selected = chunk.generate_section_mesh_bundle_with_registry(
            key,
            1,
            1,
            |x, y, z| test_chunk_lookup(&chunk, x, y, z),
            &registry,
        );
        let selected_vertices = &bundle_selected.levels[0].opaque.vertices;
        assert!(
            selected_vertices
                .iter()
                .any(|v| v.atlas_tile_u32() == (13, 13)),
            "stone must use the selected resource pack tile [13, 13]"
        );
        assert!(
            selected_vertices
                .iter()
                .any(|v| v.atlas_tile_u32() == default_dirt_tile),
            "unconfigured dirt must retain its default procedural tile"
        );
        assert!(
            !selected_vertices
                .iter()
                .any(|v| v.atlas_tile_u32() == default_stone_tile),
            "stone should no longer produce default tile vertices under the selected pack"
        );

        let halo = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
            let (block, sky, block_light, level, falling) = test_chunk_lookup(&chunk, wx, wy, wz);
            MeshVoxel {
                block,
                state: chunk.get_block_state(wx, wy, wz),
                sky,
                block_light,
                raw_fluid: level | if falling { 8 } else { 0 },
            }
        });
        let worker_bundle = Chunk::generate_section_mesh_bundle_from_halo_with_registry(
            SectionIdentity::new(key, 1, 1),
            &halo,
            &registry,
        );
        assert!(
            worker_bundle.levels[0]
                .opaque
                .vertices
                .iter()
                .any(|v| v.atlas_tile_u32() == (13, 13)),
            "worker halo entry point must apply selected pack tiles"
        );
    }

    #[test]
    fn section_bundle_builds_distinct_bounded_lods_and_preserves_identity() {
        let mut chunk = empty_test_chunk();
        let min_y = chunk.min_section_y as i32 * 16;
        let max_y = min_y + (chunk.sections.len() as i32) * 16;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in min_y..max_y {
                    let block = if y < 64 {
                        BlockType::Stone
                    } else if y == 64 {
                        BlockType::Grass
                    } else {
                        BlockType::Air
                    };
                    chunk.set_block_local(x, y, z, block);
                    chunk.set_sky_light(x, y, z, if y >= 64 { 15 } else { 0 });
                }
                chunk.heightmap[x][z] = 64;
            }
        }

        let key = SectionKey::new(0, 4, 0);
        let bundle = chunk
            .generate_section_mesh_bundle(key, 42, 7, |x, y, z| test_chunk_lookup(&chunk, x, y, z));

        assert_eq!(bundle.identity.key, key);
        assert_eq!(bundle.identity.revision, 42);
        assert_eq!(bundle.identity.lifetime, 7);
        assert_eq!(bundle.levels.len(), 3);
        assert!(
            bundle.bounds.is_some(),
            "non-empty section must produce overall bounds"
        );

        for (lod_idx, lod) in bundle.levels.iter().enumerate() {
            assert!(
                lod.bounds().is_some(),
                "LOD {lod_idx} must produce valid bounds"
            );
            if let Some(bounds) = lod.bounds() {
                assert!(bounds.min[1] >= key.min_world_y() as f32 - 0.01);
                assert!(bounds.max[1] <= key.max_world_y() as f32 + 0.01);
            }
        }

        let l0_quads = bundle.levels[0].opaque.indices.len() / 6;
        let l1_quads = bundle.levels[1].opaque.indices.len() / 6;
        let l2_quads = bundle.levels[2].opaque.indices.len() / 6;
        assert!(l0_quads > 0 && l1_quads > 0 && l2_quads > 0);
    }

    #[test]
    fn section_halo_occludes_boundary_neighbor() {
        let key = SectionKey::new(0, 0, 0);
        let snapshot = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
            let in_section =
                (0..16).contains(&wx) && (0..16).contains(&wy) && (0..16).contains(&wz);
            let neighbor_block = (wx, wy, wz) == (16, 0, 0);
            MeshVoxel {
                block: if in_section || neighbor_block {
                    BlockType::Stone
                } else {
                    BlockType::Air
                },
                state: 0,
                sky: 15,
                block_light: 0,
                raw_fluid: 0,
            }
        });

        assert_eq!(snapshot.get_block(1, 1, 1), BlockType::Stone);
        assert_eq!(snapshot.get_block(17, 1, 1), BlockType::Stone);
    }

    #[test]
    fn mesh_voxel_render_inputs_flow_through_core() {
        let voxel = MeshVoxel {
            block: BlockType::Torch,
            state: 3,
            sky: 14,
            block_light: 12,
            raw_fluid: 5 | 8,
        };
        assert_eq!(voxel.block, BlockType::Torch);
        assert_eq!(voxel.state, 3);
        assert_eq!(voxel.sky, 14);
        assert_eq!(voxel.block_light, 12);
        assert_eq!(voxel.raw_fluid & 7, 5);
        assert_ne!(voxel.raw_fluid & 8, 0);
    }

    #[test]
    fn legacy_generate_mesh_matches_l0_adapter_fixture() {
        let mut chunk = empty_test_chunk();
        chunk.set_block_local(2, 64, 2, BlockType::Stone);
        chunk.set_block_local(3, 64, 2, BlockType::Glass);

        let legacy = chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z));
        let min_y = chunk.min_section_y as i32 * 16;
        let total_height = chunk.sections.len() * 16;
        let core = Chunk::mesh_l0_volume(
            [0, min_y, 0],
            [CHUNK_WIDTH, total_height, CHUNK_DEPTH],
            |x, y, z| {
                let (block, sky, block_light, level, falling) = test_chunk_lookup(&chunk, x, y, z);
                MeshVoxel {
                    block,
                    state: chunk.get_block_state(x, y, z),
                    sky,
                    block_light,
                    raw_fluid: level | if falling { 8 } else { 0 },
                }
            },
        );
        assert_eq!(legacy, core);
    }

    fn test_chunk_lookup(
        chunk: &Chunk,
        world_x: i32,
        world_y: i32,
        world_z: i32,
    ) -> (BlockType, u8, u8, u8, bool) {
        if world_x < 0
            || world_x >= CHUNK_WIDTH as i32
            || world_z < 0
            || world_z >= CHUNK_DEPTH as i32
        {
            return (BlockType::Air, 15, 0, 0, false);
        }
        let x = world_x as usize;
        let z = world_z as usize;
        let fluid = chunk.get_fluid_level(x, world_y, z);
        (
            chunk.get_block_local(x, world_y, z),
            chunk.get_sky_light(x, world_y, z),
            chunk.get_block_light(x, world_y, z),
            fluid & 0x07,
            fluid & 0x08 != 0,
        )
    }

    fn single_torch_mesh(
        block: BlockType,
        sky_light: u8,
        block_light: u8,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>) {
        let mut chunk = empty_test_chunk();
        chunk.set_block_local(8, 1, 8, block);
        chunk.set_sky_light(8, 1, 8, sky_light);
        chunk.set_block_light(8, 1, 8, block_light);
        chunk.heightmap[8][8] = 1;
        chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z))
    }

    #[test]
    fn ambient_occlusion_levels_match_occluder_counts() {
        assert_eq!(ambient_occlusion_value(0), 1.0);
        assert_eq!(ambient_occlusion_value(1), 0.75);
        assert_eq!(ambient_occlusion_value(2), 0.5);
        assert_eq!(ambient_occlusion_value(3), 0.25);
        assert_eq!(ambient_occlusion_value(4), 0.25);
    }

    #[test]
    fn only_solid_opaque_blocks_cast_ambient_occlusion() {
        assert!(BlockType::Stone.is_ao_occluder());
        assert!(BlockType::Grass.is_ao_occluder());
        for block in [
            BlockType::Air,
            BlockType::Water,
            BlockType::Lava,
            BlockType::Glass,
            BlockType::OakLeaves,
            BlockType::Torch,
            BlockType::TallGrass,
            BlockType::Cactus,
        ] {
            assert!(!block.is_ao_occluder(), "{block:?} should not cast AO");
        }
    }

    #[test]
    fn ao_samples_follow_every_face_and_corner_direction() {
        let block_pos = [10, 20, 30];

        // Top face (+Y): normal is [0, 1, 0]
        let top_normal = [0, 1, 0];
        let corner_north_west = [0.0, 1.0, 0.0];
        let samples = ao_sample_positions(block_pos, top_normal, corner_north_west);
        assert_eq!(
            samples,
            [
                [9, 21, 30],  // side U: -X
                [10, 21, 29], // side V: -Z
                [9, 21, 29]   // corner: -X -Z
            ]
        );

        let corner_south_east = [1.0, 1.0, 1.0];
        let samples = ao_sample_positions(block_pos, top_normal, corner_south_east);
        assert_eq!(
            samples,
            [
                [11, 21, 30], // side U: +X
                [10, 21, 31], // side V: +Z
                [11, 21, 31]  // corner: +X +Z
            ]
        );

        // East face (+X): normal is [1, 0, 0]
        let east_normal = [1, 0, 0];
        let corner_top_south = [1.0, 1.0, 1.0];
        let samples = ao_sample_positions(block_pos, east_normal, corner_top_south);
        assert_eq!(
            samples,
            [
                [11, 21, 30], // side U: +Y
                [11, 20, 31], // side V: +Z
                [11, 21, 31]  // corner: +Y +Z
            ]
        );
    }

    fn triangle_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ]
    }

    #[test]
    fn ao_diagonal_selection_preserves_face_winding() {
        let quad_positions = [
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];

        for ao in [
            [1.0, 0.5, 1.0, 0.5], // ao[0] + ao[2] > ao[1] + ao[3] -> [0, 1, 3, 1, 2, 3]
            [0.5, 1.0, 0.5, 1.0], // ao[0] + ao[2] <= ao[1] + ao[3] -> [0, 1, 2, 0, 2, 3]
        ] {
            let indices = quad_indices_for_ao(ao);
            let n1 = triangle_normal(
                quad_positions[indices[0] as usize],
                quad_positions[indices[1] as usize],
                quad_positions[indices[2] as usize],
            );
            let n2 = triangle_normal(
                quad_positions[indices[3] as usize],
                quad_positions[indices[4] as usize],
                quad_positions[indices[5] as usize],
            );
            assert!(
                n1[1] > 0.0,
                "first triangle must face +Y for top quad winding"
            );
            assert!(
                n2[1] > 0.0,
                "second triangle must face +Y for top quad winding"
            );
        }
    }

    #[test]
    fn generated_mesh_writes_ao_for_isolated_and_occluded_vertices() {
        let mut chunk = empty_test_chunk();
        chunk.set_block_local(8, 1, 8, BlockType::Stone);
        chunk.heightmap[8][8] = 1;

        let (vertices, _, _, _) = chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z));
        assert!(
            vertices.iter().all(|vertex| vertex.ao() == 1.0),
            "an isolated stone cube in empty air must have full 1.0 AO across all vertices"
        );

        // Place occluders flanking the top-north-west corner of (8, 1, 8).
        chunk.set_block_local(7, 2, 8, BlockType::Stone);
        chunk.set_block_local(8, 2, 7, BlockType::Stone);
        chunk.heightmap[7][8] = 2;
        chunk.heightmap[8][7] = 2;

        let (vertices, _, _, _) = chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z));
        let top_face_ao_values: Vec<f32> = vertices
            .iter()
            .filter(|vertex| {
                let pos = vertex.local_position();
                (pos[1] - 2.0).abs() < 1e-4
                    && pos[0] >= 8.0
                    && pos[0] <= 9.0
                    && pos[2] >= 8.0
                    && pos[2] <= 9.0
            })
            .map(|vertex| vertex.ao())
            .collect();

        assert!(
            top_face_ao_values.iter().any(|ao| *ao < 1.0),
            "top face near neighbor occluders must have darkened AO values: {top_face_ao_values:?}"
        );
    }

    #[test]
    fn greedy_meshing_merges_equal_faces_and_repeats_the_atlas_tile() {
        let mut chunk = empty_test_chunk();
        for x in 8..10 {
            for z in 8..10 {
                chunk.set_block_local(x, 1, z, BlockType::Stone);
                chunk.heightmap[x][z] = 1;
            }
        }

        let lookup = |x, y, z| test_chunk_lookup(&chunk, x, y, z);
        let (vertices, indices, transparent_vertices, transparent_indices) =
            chunk.generate_mesh(lookup);

        // A 2x1x2 cuboid has six exterior rectangles after greedy merging.
        assert_eq!(vertices.len(), 6 * 4);
        assert_eq!(indices.len(), 6 * 6);
        assert!(transparent_vertices.is_empty());
        assert!(transparent_indices.is_empty());
        assert_eq!(
            vertices
                .iter()
                .flat_map(|vertex| vertex.local_uv_f32())
                .fold(0.0f32, f32::max),
            2.0
        );
    }

    #[test]
    fn greedy_meshing_does_not_merge_different_light_or_material() {
        let mut light_chunk = empty_test_chunk();
        for x in 8..10 {
            light_chunk.set_block_local(x, 1, 8, BlockType::Stone);
            light_chunk.heightmap[x][8] = 1;
        }
        light_chunk.set_sky_light(9, 2, 8, 14);
        let (light_vertices, _, _, _) =
            light_chunk.generate_mesh(|x, y, z| test_chunk_lookup(&light_chunk, x, y, z));
        let light_top_quads = light_vertices
            .chunks_exact(4)
            .filter(|quad| quad.iter().all(|vertex| vertex.local_position()[1] == 2.0))
            .count();
        assert_eq!(light_top_quads, 2);

        let mut material_chunk = empty_test_chunk();
        material_chunk.set_block_local(8, 1, 8, BlockType::Stone);
        material_chunk.set_block_local(9, 1, 8, BlockType::Dirt);
        material_chunk.heightmap[8][8] = 1;
        material_chunk.heightmap[9][8] = 1;
        let (material_vertices, _, _, _) =
            material_chunk.generate_mesh(|x, y, z| test_chunk_lookup(&material_chunk, x, y, z));
        let material_top_quads = material_vertices
            .chunks_exact(4)
            .filter(|quad| quad.iter().all(|vertex| vertex.local_position()[1] == 2.0))
            .count();
        assert_eq!(material_top_quads, 2);
    }

    #[test]
    fn surface_lod_merges_flat_skirts_and_coarsens_varied_terrain() {
        use glam::Vec3;
        let mut flat = empty_test_chunk();
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                flat.set_block_local(x, 1, z, BlockType::Stone);
                flat.heightmap[x][z] = 1;
            }
        }
        let flat_l1 = flat.generate_surface_mesh(|x, y, z| test_chunk_lookup(&flat, x, y, z), 1);
        let flat_l2 = flat.generate_surface_mesh(|x, y, z| test_chunk_lookup(&flat, x, y, z), 4);
        // One top plus four merged boundary skirts at either resolution.
        assert_eq!(flat_l1.opaque.indices.len(), 5 * 6);
        assert_eq!(flat_l2.opaque.indices.len(), 5 * 6);
        let bounds = flat_l2.opaque.bounds.expect("flat LOD should have bounds");
        assert_eq!(bounds.min, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(bounds.max, Vec3::new(16.0, 2.0, 16.0));

        let mut varied = empty_test_chunk();
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let height = 1 + (x + z) % 2;
                for y in 1..=height {
                    varied.set_block_local(x, y as i32, z, BlockType::Stone);
                }
                varied.heightmap[x][z] = height as i16;
            }
        }
        let varied_l1 =
            varied.generate_surface_mesh(|x, y, z| test_chunk_lookup(&varied, x, y, z), 1);
        let varied_l2 =
            varied.generate_surface_mesh(|x, y, z| test_chunk_lookup(&varied, x, y, z), 4);
        assert!(
            varied_l2.opaque.indices.len() < varied_l1.opaque.indices.len(),
            "coarse LOD should submit fewer indices"
        );
    }

    #[test]
    fn snow_layer_mesh_is_one_eighth_of_a_block_high() {
        let mut chunk = Chunk::new(0, 0);
        let min_y = chunk.min_section_y as i32 * 16;
        let max_y = min_y + (chunk.sections.len() as i32) * 16;
        for x in 0..CHUNK_WIDTH {
            for y in min_y..max_y {
                for z in 0..CHUNK_DEPTH {
                    chunk.set_block_local(x, y, z, BlockType::Air);
                }
            }
            for z in 0..CHUNK_DEPTH {
                chunk.heightmap[x][z] = NO_HEIGHT;
            }
        }
        chunk.set_block_local(8, 1, 8, BlockType::SnowLayer);
        chunk.heightmap[8][8] = 1;
        let lookup = |_: i32, _: i32, _: i32| (BlockType::Air, 15, 0, 0, false);
        let (vertices, _, _, _) = chunk.generate_mesh(lookup);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.local_position()[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((max_y - 1.125).abs() < f32::EPSILON);
    }

    #[test]
    fn cross_model_blocks_generate_x_mesh() {
        for plant in [
            BlockType::Dandelion,
            BlockType::Poppy,
            BlockType::TallGrass,
            BlockType::SugarCane,
        ] {
            let mut chunk = empty_test_chunk();
            chunk.set_block_local(8, 64, 8, plant);
            chunk.heightmap[8][8] = 64;

            let (vertices, indices, transparent_vertices, transparent_indices) =
                chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z));

            assert_eq!(
                vertices.len(),
                16,
                "{plant:?} must produce four quads (two double-sided planes)"
            );
            assert_eq!(indices.len(), 24, "{plant:?} must produce eight triangles");
            assert!(
                transparent_vertices.is_empty(),
                "cutout plants belong in the opaque pass"
            );
            assert!(transparent_indices.is_empty());
        }
    }

    #[test]
    fn torch_mesh_has_minecraft_bounds_and_six_outward_faces() {
        let (vertices, indices, transparent_vertices, transparent_indices) =
            single_torch_mesh(BlockType::Torch, 15, 14);

        assert_eq!(
            vertices.len(),
            24,
            "torch must produce six four-vertex quads"
        );
        assert_eq!(
            indices.len(),
            36,
            "torch must produce twelve three-index triangles"
        );
        assert!(transparent_vertices.is_empty());
        assert!(transparent_indices.is_empty());

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for vertex in &vertices {
            let pos = vertex.local_position();
            for axis in 0..3 {
                min[axis] = min[axis].min(pos[axis]);
                max[axis] = max[axis].max(pos[axis]);
            }
        }

        assert_eq!(min, [8.0 + TORCH_MIN, 1.0, 8.0 + TORCH_MIN]);
        assert_eq!(max, [8.0 + TORCH_MAX, 1.0 + TORCH_HEIGHT, 8.0 + TORCH_MAX]);
        assert_eq!(
            [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            [2.0 / 16.0, 10.0 / 16.0, 2.0 / 16.0],
            "torch bounding box must be exactly 2x10x2 texels"
        );
    }

    #[test]
    fn door_mesh_generation_bounds_and_quad_count() {
        let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(BlockType::OakDoor, 15, 0);
        assert!(trans_v.is_empty() && trans_i.is_empty());
        // 6 faces * 4 vertices = 24 vertices, 36 indices
        assert_eq!(opaque_v.len(), 24);
        assert_eq!(opaque_i.len(), 36);

        let min_x = opaque_v
            .iter()
            .map(|v| v.pos[0] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_x = opaque_v
            .iter()
            .map(|v| v.pos[0] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_z = opaque_v
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_z = opaque_v
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);

        // North facing door, closed: X in [8.0, 9.0], Z in [8.0, 8.0 + 3.0/16.0]
        assert!((min_x - 8.0).abs() < 1e-4);
        assert!((max_x - 9.0).abs() < 1e-4);
        assert!((min_z - 8.0).abs() < 1e-4);
        assert!((max_z - (8.0 + 3.0 / 16.0)).abs() < 1e-4);
    }

    #[test]
    fn trapdoor_mesh_generation_open_and_closed_bounds() {
        // Closed trapdoor: bottom 3/16ths
        let mut chunk = empty_test_chunk();
        chunk.set_block_local(8, 1, 8, BlockType::OakTrapdoor);
        chunk.set_block_state(8, 1, 8, BlockState::default().encode()); // closed, north
        chunk.heightmap[8][8] = 1;

        let (opaque_v, _, _, _) = chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z));
        let min_y = opaque_v
            .iter()
            .map(|v| v.pos[1] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_y = opaque_v
            .iter()
            .map(|v| v.pos[1] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);

        assert!((min_y - 1.0).abs() < 1e-4);
        assert!((max_y - (1.0 + 3.0 / 16.0)).abs() < 1e-4);

        // Open trapdoor: against north wall
        let open_state = BlockState {
            is_open: true,
            ..BlockState::default()
        };
        chunk.set_block_state(8, 1, 8, open_state.encode());
        let (opaque_v2, _, _, _) =
            chunk.generate_mesh(|x, y, z| test_chunk_lookup(&chunk, x, y, z));
        let min_z = opaque_v2
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_z = opaque_v2
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);

        assert!((min_z - 8.0).abs() < 1e-4);
        assert!((max_z - (8.0 + 3.0 / 16.0)).abs() < 1e-4);
    }

    #[test]
    fn torch_mesh_uses_inset_face_uvs_inside_its_atlas_tile() {
        let (vertices, _, _, _) = single_torch_mesh(BlockType::Torch, 15, 14);
        let expected_rects = [
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_TOP_UV,
            TORCH_BOTTOM_UV,
        ];

        assert_eq!(
            TORCH_SIDE_UV,
            [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 13.5 / 16.0]
        );
        assert_eq!(
            TORCH_TOP_UV,
            [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 4.5 / 16.0]
        );
        assert_eq!(
            TORCH_BOTTOM_UV,
            [7.5 / 16.0, 13.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0]
        );

        for (face_idx, quad) in vertices.chunks_exact(4).enumerate() {
            let rect = expected_rects[face_idx];
            let mut observed_min = [f32::INFINITY; 2];
            let mut observed_max = [f32::NEG_INFINITY; 2];
            for vertex in quad {
                assert_eq!(vertex.atlas_tile_u32(), (4, 2));
                let uv = vertex.local_uv_f32();
                assert!(
                    (0.0..1.0).contains(&uv[0]) && (0.0..1.0).contains(&uv[1]),
                    "torch UV must remain inside atlas tile (4, 2)"
                );
                observed_min[0] = observed_min[0].min(uv[0]);
                observed_min[1] = observed_min[1].min(uv[1]);
                observed_max[0] = observed_max[0].max(uv[0]);
                observed_max[1] = observed_max[1].max(uv[1]);
            }
            assert_eq!(observed_min, [rect[0], rect[1]]);
            assert_eq!(observed_max, [rect[2], rect[3]]);
        }
    }

    #[test]
    fn torch_mesh_uses_source_light_without_ao_or_face_shading() {
        let sky_light = 9;
        let block_light = 14;
        let expected_packed_light = sky_light as f32 + block_light as f32 * 16.0;
        let (vertices, _, _, _) = single_torch_mesh(BlockType::Torch, sky_light, block_light);

        assert!(vertices.iter().all(|vertex| vertex.ao() == 1.0));
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.light_level() == expected_packed_light),
            "every torch face must use the source cell light without a face multiplier"
        );
    }

    #[test]
    fn redstone_torch_variants_use_thin_torch_mesh_and_redstone_tile() {
        let expected_rects = [
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_SIDE_UV,
            TORCH_TOP_UV,
            TORCH_BOTTOM_UV,
        ];

        for block in [BlockType::RedstoneTorch, BlockType::RedstoneTorchOff] {
            let (vertices, indices, transparent_vertices, transparent_indices) =
                single_torch_mesh(block, 15, block.properties().light_emission);

            assert_eq!(vertices.len(), 24, "{block:?} must have six quads");
            assert_eq!(indices.len(), 36, "{block:?} must have twelve triangles");
            assert!(transparent_vertices.is_empty());
            assert!(transparent_indices.is_empty());

            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for vertex in &vertices {
                let pos = vertex.local_position();
                for axis in 0..3 {
                    min[axis] = min[axis].min(pos[axis]);
                    max[axis] = max[axis].max(pos[axis]);
                }
            }
            assert_eq!(min, [8.0 + TORCH_MIN, 1.0, 8.0 + TORCH_MIN]);
            assert_eq!(max, [8.0 + TORCH_MAX, 1.0 + TORCH_HEIGHT, 8.0 + TORCH_MAX]);
            assert_eq!(
                [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
                [2.0 / 16.0, 10.0 / 16.0, 2.0 / 16.0],
                "{block:?} must not fall back to full-cube geometry"
            );

            for (face_idx, quad) in vertices.chunks_exact(4).enumerate() {
                let rect = expected_rects[face_idx];
                let mut observed_min = [f32::INFINITY; 2];
                let mut observed_max = [f32::NEG_INFINITY; 2];
                for vertex in quad {
                    assert_eq!(
                        vertex.atlas_tile_u32(),
                        (
                            REDSTONE_TORCH_ATLAS_TILE.0 as u32,
                            REDSTONE_TORCH_ATLAS_TILE.1 as u32,
                        )
                    );
                    let uv = vertex.local_uv_f32();
                    observed_min[0] = observed_min[0].min(uv[0]);
                    observed_min[1] = observed_min[1].min(uv[1]);
                    observed_max[0] = observed_max[0].max(uv[0]);
                    observed_max[1] = observed_max[1].max(uv[1]);
                }
                assert_eq!(observed_min, [rect[0], rect[1]]);
                assert_eq!(observed_max, [rect[2], rect[3]]);
            }
        }
    }

    #[test]
    fn torch_properties_and_floor_support_semantics_are_preserved() {
        let properties = BlockType::Torch.properties();
        assert_eq!(properties.render_type, RenderType::Cutout);
        assert!(!properties.is_solid);
        assert!(!properties.is_passable);
        assert_eq!(properties.light_emission, 14);
        assert!(BlockType::Torch.can_stay_on(BlockType::Stone));
        assert!(!BlockType::Torch.can_stay_on(BlockType::Air));

        let mut manager = crate::chunk_manager::ChunkManager::new(2);
        manager.chunks.insert((0, 0), empty_test_chunk());
        manager.set_block(8, 64, 8, BlockType::Stone);
        manager.set_block(8, 65, 8, BlockType::Torch);

        let mut dirty = HashSet::new();
        crate::lighting::update_block_light_after_placed(
            &mut manager,
            8,
            65,
            8,
            properties.light_emission,
            &mut dirty,
        );
        assert_eq!(manager.get_block_light(8, 65, 8), 14);
        assert_eq!(manager.get_block_light(9, 65, 8), 13);

        manager.set_block(8, 64, 8, BlockType::Air);
        let mut broken = Vec::new();
        manager.check_and_break_unsupported_above(8, 64, 8, &mut dirty, |position, block| {
            broken.push((position, block));
        });

        assert_eq!(manager.get_block(8, 65, 8), BlockType::Air);
        assert_eq!(broken, vec![((8, 65, 8), BlockType::Torch)]);
        assert_eq!(manager.get_block_light(8, 65, 8), 0);
        assert_eq!(manager.get_block_light(9, 65, 8), 0);
    }

    #[test]
    fn cactus_mesh_generation_bounds_and_quad_count() {
        let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(BlockType::Cactus, 15, 0);
        assert!(trans_v.is_empty() && trans_i.is_empty());
        // 6 faces * 4 vertices = 24 vertices, 36 indices
        assert_eq!(opaque_v.len(), 24);
        assert_eq!(opaque_i.len(), 36);

        let min_x = opaque_v
            .iter()
            .map(|v| v.pos[0] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_x = opaque_v
            .iter()
            .map(|v| v.pos[0] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_z = opaque_v
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::INFINITY, f32::min);
        let max_z = opaque_v
            .iter()
            .map(|v| v.pos[2] as f32 / 32.0)
            .fold(f32::NEG_INFINITY, f32::max);

        // Cactus placed at (8, 1, 8) -> origin = (8.0, 1.0, 8.0)
        // Inset by 1/16th: min = 8.0625, max = 8.9375
        assert!((min_x - (8.0 + 1.0 / 16.0)).abs() < 1e-4);
        assert!((max_x - (8.0 + 15.0 / 16.0)).abs() < 1e-4);
        assert!((min_z - (8.0 + 1.0 / 16.0)).abs() < 1e-4);
        assert!((max_z - (8.0 + 15.0 / 16.0)).abs() < 1e-4);
    }

    #[test]
    fn end_portal_frame_and_surface_use_lower_minecraft_heights() {
        for block in [BlockType::EndPortalFrame, BlockType::EndPortalFrameFilled] {
            let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(block, 15, 0);
            assert!(trans_v.is_empty() && trans_i.is_empty());
            assert_eq!(opaque_v.len(), 24);
            assert_eq!(opaque_i.len(), 36);
            let max_y = opaque_v
                .iter()
                .map(|vertex| vertex.pos[1] as f32 / 32.0)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((max_y - (1.0 + END_PORTAL_FRAME_HEIGHT)).abs() < 1e-4);
        }

        let (opaque_v, opaque_i, trans_v, trans_i) =
            single_torch_mesh(BlockType::EndPortal, 15, 15);
        assert!(opaque_v.is_empty() && opaque_i.is_empty());
        assert_eq!(trans_v.len(), 8);
        assert_eq!(trans_i.len(), 12);
        assert!(trans_v.iter().all(|vertex| {
            (vertex.pos[1] as f32 / 32.0 - (1.0 + END_PORTAL_SURFACE_HEIGHT)).abs() < 1e-4
        }));
        assert!(END_PORTAL_SURFACE_HEIGHT < END_PORTAL_FRAME_HEIGHT);
    }
}

use super::*;

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

pub(super) type FaceCorner = ([f32; 3], [f32; 2]);

// Face order: south, north, west, east, up, down.
pub(super) const BLOCK_FACES: [([i32; 3], [FaceCorner; 4]); 6] = [
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


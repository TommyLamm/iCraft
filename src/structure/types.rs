use crate::block_entity::BlockEntity;
use crate::world::BlockType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StructureId {
    Dungeon,
    Mineshaft,
    Village,
    Stronghold,
    NetherFortress,
    EndCity,
}

impl StructureId {
    pub fn name(&self) -> &'static str {
        match self {
            StructureId::Dungeon => "Dungeon",
            StructureId::Mineshaft => "Mineshaft",
            StructureId::Village => "Village",
            StructureId::Stronghold => "Stronghold",
            StructureId::NetherFortress => "Fortress",
            StructureId::EndCity => "EndCity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

impl Rotation {
    pub fn rotate_pos(&self, x: i32, z: i32, size_x: i32, size_z: i32) -> (i32, i32) {
        match self {
            Rotation::None => (x, z),
            Rotation::Clockwise90 => (size_z - 1 - z, x),
            Rotation::Clockwise180 => (size_x - 1 - x, size_z - 1 - z),
            Rotation::Clockwise270 => (z, size_x - 1 - x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

impl BoundingBox {
    pub fn new(min_x: i32, min_y: i32, min_z: i32, max_x: i32, max_y: i32, max_z: i32) -> Self {
        Self {
            min_x: min_x.min(max_x),
            min_y: min_y.min(max_y),
            min_z: min_z.min(max_z),
            max_x: min_x.max(max_x),
            max_y: min_y.max(max_y),
            max_z: min_z.max(max_z),
        }
    }

    pub fn intersects_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let c_min_x = chunk_x * 16;
        let c_max_x = c_min_x + 15;
        let c_min_z = chunk_z * 16;
        let c_max_z = c_min_z + 15;

        !(self.max_x < c_min_x
            || self.min_x > c_max_x
            || self.max_z < c_min_z
            || self.min_z > c_max_z)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPlacement {
    pub world_x: i32,
    pub world_y: i32,
    pub world_z: i32,
    pub block_type: BlockType,
    pub block_entity: Option<BlockEntity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructurePiece {
    pub bounding_box: BoundingBox,
    pub blocks: Vec<BlockPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureStart {
    pub id: StructureId,
    pub origin_x: i32,
    pub origin_y: i32,
    pub origin_z: i32,
    pub bounding_box: BoundingBox,
    pub pieces: Vec<StructurePiece>,
}

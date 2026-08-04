use crate::world::BlockType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChestBlockEntity {
    pub custom_name: Option<String>,
    pub inventory: crate::inventory::ContainerInventory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FurnaceStub {
    pub custom_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignStub {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockEntity {
    Chest(ChestBlockEntity),
    Furnace(FurnaceStub),
    Sign(SignStub),
}

impl BlockEntity {
    pub fn matches_block_type(&self, block_type: BlockType) -> bool {
        match self {
            BlockEntity::Chest(_) => {
                matches!(block_type, BlockType::Chest | BlockType::EndCityChest)
            }
            BlockEntity::Furnace(_) => matches!(block_type, BlockType::Furnace),
            BlockEntity::Sign(_) => false,
        }
    }

    pub fn memory_usage(&self) -> usize {
        let base = std::mem::size_of::<Self>();
        let extra = match self {
            BlockEntity::Chest(c) => c.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Furnace(f) => f.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Sign(s) => s.text.capacity(),
        };
        base + extra
    }
}

pub fn default_stub_for_block(block_type: BlockType) -> Option<BlockEntity> {
    match block_type {
        BlockType::Chest | BlockType::EndCityChest => Some(BlockEntity::Chest(ChestBlockEntity {
            custom_name: None,
            inventory: crate::inventory::ContainerInventory::new(),
        })),
        BlockType::Furnace => Some(BlockEntity::Furnace(FurnaceStub { custom_name: None })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_entity_matching() {
        let chest = BlockEntity::Chest(ChestBlockEntity {
            custom_name: None,
            inventory: crate::inventory::ContainerInventory::new(),
        });
        assert!(chest.matches_block_type(BlockType::Chest));
        assert!(chest.matches_block_type(BlockType::EndCityChest));
        assert!(!chest.matches_block_type(BlockType::Furnace));
        assert!(!chest.matches_block_type(BlockType::Dirt));

        let furnace = BlockEntity::Furnace(FurnaceStub { custom_name: None });
        assert!(furnace.matches_block_type(BlockType::Furnace));
        assert!(!furnace.matches_block_type(BlockType::Chest));

        let sign = BlockEntity::Sign(SignStub {
            text: "Hello".to_string(),
        });
        assert!(!sign.matches_block_type(BlockType::Dirt));
    }
}

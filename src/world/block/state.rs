use super::*;
use crate::redstone::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChestType {
    Single,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockState {
    pub facing: Direction,
    pub is_top: bool,
    pub is_right_hinge: bool,
    /// Shared bit 4: door/trapdoor/chest open; lamp/furnace lit; piston extended;
    /// end-portal frame filled; lever/button/plate/repeater/comparator powered;
    /// redstone torch **extinguished** (clear bit = lit, matching legacy id 49).
    pub is_open: bool,
    pub chest_type: ChestType,
}

/// BlockState bit 4 — open / powered / lit / extended / filled (see `is_open`).
pub const BLOCK_STATE_OPEN_BIT: u8 = 1 << 4;

impl Default for BlockState {
    fn default() -> Self {
        Self {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
            chest_type: ChestType::Single,
        }
    }
}

impl BlockState {
    pub fn encode(self) -> u8 {
        let facing_bits = match self.facing {
            Direction::North => 0b00,
            Direction::South => 0b01,
            Direction::West => 0b10,
            _ => 0b11,
        };
        let half_bit = if self.is_top { 1 << 2 } else { 0 };
        let hinge_bit = if self.is_right_hinge { 1 << 3 } else { 0 };
        let open_bit = if self.is_open {
            BLOCK_STATE_OPEN_BIT
        } else {
            0
        };
        let chest_type_bits = match self.chest_type {
            ChestType::Single => 0b00,
            ChestType::Left => 0b01,
            ChestType::Right => 0b10,
        } << 5;
        facing_bits | half_bit | hinge_bit | open_bit | chest_type_bits
    }

    pub fn decode(val: u8) -> Self {
        let facing = match val & 0b11 {
            0 => Direction::North,
            1 => Direction::South,
            2 => Direction::West,
            3 => Direction::East,
            _ => unreachable!(),
        };
        let is_top = (val & (1 << 2)) != 0;
        let is_right_hinge = (val & (1 << 3)) != 0;
        let is_open = (val & BLOCK_STATE_OPEN_BIT) != 0;
        let chest_type = match (val >> 5) & 0b11 {
            0 => ChestType::Single,
            1 => ChestType::Left,
            2 => ChestType::Right,
            _ => ChestType::Single,
        };
        Self {
            facing,
            is_top,
            is_right_hinge,
            is_open,
            chest_type,
        }
    }

    pub fn for_door_placement(
        chunk_manager: &impl crate::chunk_manager::ColumnQuery,
        x: i32,
        y: i32,
        z: i32,
        yaw: f32,
    ) -> (Self, Self) {
        let facing = Direction::from_yaw(yaw);
        let (left_dx, left_dz) = match facing {
            Direction::North => (-1, 0),
            Direction::South => (1, 0),
            Direction::West => (0, 1),
            _ => (0, -1),
        };
        let (right_dx, right_dz) = match facing {
            Direction::North => (1, 0),
            Direction::South => (-1, 0),
            Direction::West => (0, -1),
            _ => (0, 1),
        };

        let left_block = chunk_manager.get_block(x + left_dx, y, z + left_dz);
        let right_block = chunk_manager.get_block(x + right_dx, y, z + right_dz);

        let is_right_hinge = left_block.properties().is_solid && !right_block.properties().is_solid;

        let bottom = Self {
            facing,
            is_top: false,
            is_right_hinge,
            is_open: false,
            chest_type: ChestType::Single,
        };
        let top = Self {
            facing,
            is_top: true,
            is_right_hinge,
            is_open: false,
            chest_type: ChestType::Single,
        };
        (bottom, top)
    }

    pub fn for_trapdoor_placement(yaw: f32) -> Self {
        let facing = Direction::from_yaw(yaw);
        Self {
            facing,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
            chest_type: ChestType::Single,
        }
    }
}


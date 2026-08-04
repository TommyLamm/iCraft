// Container session management for multiplayer chest interactions.
// Host-authoritative: each player has at most one open container session.
// Sessions track player_id, dimension, block position, and revision.
// Clicks are simulated on the host, committed atomically, and broadcast to viewers.

use crate::block_entity::{BlockEntity, ChestBlockEntity};
use crate::chunk_manager::ChunkManager;
use crate::inventory::{apply_stack_click, ContainerInventory, ItemStack};
use crate::network::protocol::PlayerId;
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_WIDTH};

#[derive(Debug, Clone)]
pub struct ContainerSession {
    pub player_id: PlayerId,
    pub dimension: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub revision: u64,
    pub is_double: bool,
}

impl ContainerSession {
    pub fn new(player_id: PlayerId, dimension: u8, x: i32, y: i32, z: i32) -> Self {
        Self {
            player_id,
            dimension,
            x,
            y,
            z,
            revision: 0,
            is_double: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct ContainerSessionManager {
    pub sessions: Vec<ContainerSession>,
}

impl ContainerSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }

    pub fn find_by_player(&self, player_id: PlayerId) -> Option<&ContainerSession> {
        self.sessions.iter().find(|s| s.player_id == player_id)
    }

    pub fn find_by_player_mut(&mut self, player_id: PlayerId) -> Option<&mut ContainerSession> {
        self.sessions.iter_mut().find(|s| s.player_id == player_id)
    }

    pub fn open(&mut self, player_id: PlayerId, dimension: u8, x: i32, y: i32, z: i32) -> bool {
        self.close_by_player(player_id);
        self.sessions
            .push(ContainerSession::new(player_id, dimension, x, y, z));
        true
    }

    pub fn close_by_player(&mut self, player_id: PlayerId) {
        self.sessions.retain(|s| s.player_id != player_id);
    }

    pub fn close_by_block(&mut self, x: i32, y: i32, z: i32) -> Vec<PlayerId> {
        let affected: Vec<PlayerId> = self
            .sessions
            .iter()
            .filter(|s| (s.x - x).abs() <= 1 && s.y == y && (s.z - z).abs() <= 1)
            .map(|s| s.player_id)
            .collect();
        self.sessions
            .retain(|s| !((s.x - x).abs() <= 1 && s.y == y && (s.z - z).abs() <= 1));
        affected
    }

    pub fn close_all_for_player(&mut self, player_id: PlayerId) {
        self.close_by_player(player_id);
    }

    pub fn get_double_chest_partner(
        chunk_manager: &ChunkManager,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<(i32, i32, i32)> {
        let state_raw = chunk_manager.get_block_state(x, y, z);
        let state = crate::world::BlockState::decode(state_raw);
        if state.chest_type == crate::world::ChestType::Single {
            return None;
        }
        let (dx, dz) = match (state.facing, state.chest_type) {
            (crate::redstone::Direction::North, crate::world::ChestType::Left) => (-1, 0),
            (crate::redstone::Direction::North, crate::world::ChestType::Right) => (1, 0),
            (crate::redstone::Direction::East, crate::world::ChestType::Left) => (0, -1),
            (crate::redstone::Direction::East, crate::world::ChestType::Right) => (0, 1),
            (crate::redstone::Direction::South, crate::world::ChestType::Left) => (1, 0),
            (crate::redstone::Direction::South, crate::world::ChestType::Right) => (-1, 0),
            (crate::redstone::Direction::West, crate::world::ChestType::Left) => (0, 1),
            (crate::redstone::Direction::West, crate::world::ChestType::Right) => (0, -1),
            _ => return None,
        };
        let partner_pos = (x + dx, y, z + dz);
        if chunk_manager.get_block(partner_pos.0, partner_pos.1, partner_pos.2) == BlockType::Chest
        {
            Some(partner_pos)
        } else {
            None
        }
    }

    pub fn get_single_chest_inventory(
        chunk_manager: &ChunkManager,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<ContainerInventory> {
        let (cx, cz) = (
            x.div_euclid(CHUNK_WIDTH as i32),
            z.div_euclid(CHUNK_DEPTH as i32),
        );
        let (bx, by, bz) = (
            x.rem_euclid(CHUNK_WIDTH as i32) as u8,
            y as i16,
            z.rem_euclid(CHUNK_DEPTH as i32) as u8,
        );
        chunk_manager.chunks.get(&(cx, cz)).and_then(|chunk| {
            chunk.get_block_entity(bx, by, bz).and_then(|entity| {
                if let BlockEntity::Chest(chest_be) = entity {
                    Some(chest_be.inventory.clone())
                } else {
                    None
                }
            })
        })
    }

    pub fn get_chest_slots(
        chunk_manager: &ChunkManager,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<Vec<Option<ItemStack>>> {
        let primary_inv = Self::get_single_chest_inventory(chunk_manager, x, y, z)?;
        let state_raw = chunk_manager.get_block_state(x, y, z);
        let state = crate::world::BlockState::decode(state_raw);
        if let Some(partner_pos) = Self::get_double_chest_partner(chunk_manager, x, y, z) {
            if let Some(partner_inv) = Self::get_single_chest_inventory(
                chunk_manager,
                partner_pos.0,
                partner_pos.1,
                partner_pos.2,
            ) {
                let mut combined_slots = vec![None; 54];
                if state.chest_type == crate::world::ChestType::Left {
                    combined_slots[..27].clone_from_slice(&primary_inv.slots);
                    combined_slots[27..54].clone_from_slice(&partner_inv.slots);
                } else {
                    combined_slots[..27].clone_from_slice(&partner_inv.slots);
                    combined_slots[27..54].clone_from_slice(&primary_inv.slots);
                }
                return Some(combined_slots);
            }
        }
        Some(primary_inv.slots.to_vec())
    }

    pub fn get_chest_inventory(
        chunk_manager: &ChunkManager,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<ContainerInventory> {
        Self::get_single_chest_inventory(chunk_manager, x, y, z)
    }

    pub fn set_single_chest_inventory(
        chunk_manager: &mut ChunkManager,
        x: i32,
        y: i32,
        z: i32,
        inventory: ContainerInventory,
    ) -> bool {
        let (cx, cz) = (
            x.div_euclid(CHUNK_WIDTH as i32),
            z.div_euclid(CHUNK_DEPTH as i32),
        );
        let (bx, by, bz) = (
            x.rem_euclid(CHUNK_WIDTH as i32) as u8,
            y as i16,
            z.rem_euclid(CHUNK_DEPTH as i32) as u8,
        );
        if let Some(chunk) = chunk_manager.chunks.get_mut(&(cx, cz)) {
            if let Some(entry) = chunk.get_block_entity(bx, by, bz).cloned() {
                if let BlockEntity::Chest(_) = entry {
                    let updated = BlockEntity::Chest(ChestBlockEntity {
                        inventory,
                        custom_name: None,
                    });
                    let _ = chunk.insert_block_entity(bx, by, bz, updated);
                    chunk_manager.dirty_chunks.mark_dirty(cx, cz);
                    return true;
                }
            }
        }
        false
    }

    pub fn set_chest_slots(
        chunk_manager: &mut ChunkManager,
        x: i32,
        y: i32,
        z: i32,
        slots: &[Option<ItemStack>],
    ) -> bool {
        if slots.len() == 54 {
            let state_raw = chunk_manager.get_block_state(x, y, z);
            let state = crate::world::BlockState::decode(state_raw);
            if let Some(partner_pos) = Self::get_double_chest_partner(chunk_manager, x, y, z) {
                let (primary_slice, partner_slice) =
                    if state.chest_type == crate::world::ChestType::Left {
                        (&slots[..27], &slots[27..54])
                    } else {
                        (&slots[27..54], &slots[..27])
                    };
                let mut p_arr = [None; 27];
                p_arr.copy_from_slice(primary_slice);
                let mut pt_arr = [None; 27];
                pt_arr.copy_from_slice(partner_slice);

                let r1 = Self::set_single_chest_inventory(
                    chunk_manager,
                    x,
                    y,
                    z,
                    ContainerInventory { slots: p_arr },
                );
                let r2 = Self::set_single_chest_inventory(
                    chunk_manager,
                    partner_pos.0,
                    partner_pos.1,
                    partner_pos.2,
                    ContainerInventory { slots: pt_arr },
                );
                return r1 && r2;
            }
        }
        if slots.len() == 27 {
            let mut arr = [None; 27];
            arr.copy_from_slice(slots);
            return Self::set_single_chest_inventory(
                chunk_manager,
                x,
                y,
                z,
                ContainerInventory { slots: arr },
            );
        }
        false
    }

    pub fn set_chest_inventory(
        chunk_manager: &mut ChunkManager,
        x: i32,
        y: i32,
        z: i32,
        inventory: ContainerInventory,
    ) -> bool {
        Self::set_single_chest_inventory(chunk_manager, x, y, z, inventory)
    }

    pub fn get_slot_count(chunk_manager: &ChunkManager, x: i32, y: i32, z: i32) -> usize {
        if Self::get_double_chest_partner(chunk_manager, x, y, z).is_some() {
            54
        } else {
            27
        }
    }
}

pub fn simulate_container_click(
    slot: Option<ItemStack>,
    dragged: Option<ItemStack>,
    is_left: bool,
) -> (Option<ItemStack>, Option<ItemStack>) {
    let result = apply_stack_click(slot, dragged, is_left);
    (result.slot, result.dragged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Item;

    #[test]
    fn container_click_left_on_empty_returns_cursor() {
        let (slot, dragged) =
            simulate_container_click(None, Some(ItemStack::new(Item::Stone, 5)), true);
        assert_eq!(slot.unwrap().count, 5);
        assert!(dragged.is_none());
    }

    #[test]
    fn container_click_left_swap_different_items() {
        let slot = Some(ItemStack::new(Item::Stone, 1));
        let dragged = Some(ItemStack::new(Item::Dirt, 1));
        let (result_slot, result_dragged) = simulate_container_click(slot, dragged, true);
        assert_eq!(result_slot.unwrap().item, Item::Dirt);
        assert_eq!(result_dragged.unwrap().item, Item::Stone);
    }

    #[test]
    fn container_click_right_takes_half() {
        let slot = Some(ItemStack::new(Item::Stone, 10));
        let (result_slot, result_dragged) = simulate_container_click(slot, None, false);
        assert_eq!(result_slot.unwrap().count, 5);
        assert_eq!(result_dragged.unwrap().count, 5);
    }

    #[test]
    fn container_session_open_close() {
        let mut manager = ContainerSessionManager::new();
        assert!(manager.open(1, 0, 10, 64, 20));
        assert!(manager.find_by_player(1).is_some());
        manager.close_by_player(1);
        assert!(manager.find_by_player(1).is_none());
    }

    #[test]
    fn container_session_close_by_block() {
        let mut manager = ContainerSessionManager::new();
        manager.open(1, 0, 10, 64, 20);
        manager.open(2, 0, 10, 64, 20);
        let affected = manager.close_by_block(10, 64, 20);
        assert_eq!(affected.len(), 2);
        assert!(manager.find_by_player(1).is_none());
        assert!(manager.find_by_player(2).is_none());
    }
}

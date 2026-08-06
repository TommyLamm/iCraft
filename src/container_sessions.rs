// Container session management for multiplayer chest interactions.
// Host-authoritative: each player has at most one open container session.
// Sessions track player_id, dimension, block position, and revision.
// Clicks are simulated on the host, committed atomically, and broadcast to viewers.

use crate::block_entity::BlockEntity;
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

    pub fn ensure_chest_loot_generated(
        chunk_manager: &mut ChunkManager,
        x: i32,
        y: i32,
        z: i32,
        world_seed: u32,
    ) {
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
            if let Some(BlockEntity::Chest(chest_be)) = chunk.get_block_entity_mut(bx, by, bz) {
                chest_be.ensure_loot_generated(world_seed, (x, y, z));
            }
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
                if let BlockEntity::Chest(mut chest_be) = entry {
                    chest_be.inventory = inventory;
                    chest_be.revision = chest_be.revision.wrapping_add(1);
                    let updated = BlockEntity::Chest(chest_be);
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
        if slots.iter().flatten().any(|stack| {
            stack.count == 0
                || stack.item == crate::inventory::Item::Air
                || stack.count > stack.item.properties().max_stack
        }) {
            return false;
        }
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
                // Validate and prepare both halves before committing either
                // one. A malformed or unloaded partner must never leave a
                // half-updated double chest.
                let primary =
                    chunk_manager
                        .get_block_entity(x, y, z)
                        .and_then(|entity| match entity {
                            BlockEntity::Chest(chest) => Some(chest.clone()),
                            _ => None,
                        });
                let partner = chunk_manager
                    .get_block_entity(partner_pos.0, partner_pos.1, partner_pos.2)
                    .and_then(|entity| match entity {
                        BlockEntity::Chest(chest) => Some(chest.clone()),
                        _ => None,
                    });
                let (Some(mut primary), Some(mut partner)) = (primary, partner) else {
                    return false;
                };
                primary.inventory = ContainerInventory { slots: p_arr };
                primary.revision = primary.revision.wrapping_add(1);
                partner.inventory = ContainerInventory { slots: pt_arr };
                partner.revision = partner.revision.wrapping_add(1);
                chunk_manager.set_block_entity(x, y, z, Some(BlockEntity::Chest(primary)));
                chunk_manager.set_block_entity(
                    partner_pos.0,
                    partner_pos.1,
                    partner_pos.2,
                    Some(BlockEntity::Chest(partner)),
                );
                return true;
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

    pub fn get_furnace_slots(
        chunk_manager: &ChunkManager,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<[Option<ItemStack>; 3]> {
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
                if let BlockEntity::Furnace(furnace_be) = entity {
                    Some(furnace_be.slots)
                } else {
                    None
                }
            })
        })
    }

    pub fn set_furnace_slots(
        chunk_manager: &mut ChunkManager,
        x: i32,
        y: i32,
        z: i32,
        slots: &[Option<ItemStack>; 3],
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
                if let BlockEntity::Furnace(mut furnace_be) = entry {
                    furnace_be.slots = *slots;
                    furnace_be.revision = furnace_be.revision.wrapping_add(1);
                    let _ = chunk.insert_block_entity(bx, by, bz, BlockEntity::Furnace(furnace_be));
                    chunk_manager.dirty_chunks.mark_dirty(cx, cz);
                    return true;
                }
            }
        }
        false
    }

    pub fn get_slot_count(chunk_manager: &ChunkManager, x: i32, y: i32, z: i32) -> usize {
        if let Some(entity) = chunk_manager.get_block_entity(x, y, z) {
            if matches!(entity, BlockEntity::Chest(_))
                && Self::get_double_chest_partner(chunk_manager, x, y, z).is_some()
            {
                54
            } else {
                entity.slot_count()
            }
        } else {
            0
        }
    }

    /// Shared slot view used by UI and automation.  Chest halves retain their
    /// existing deterministic 27/54 ordering; all other containers expose
    /// their native slot count and complete metadata-bearing stacks.
    pub fn get_container_slots(
        chunk_manager: &ChunkManager,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<Vec<Option<ItemStack>>> {
        let entity = chunk_manager.get_block_entity(x, y, z)?;
        if matches!(entity, BlockEntity::Chest(_)) {
            return Self::get_chest_slots(chunk_manager, x, y, z);
        }
        Some(
            (0..entity.slot_count())
                .map(|slot| entity.get_stack(slot).copied())
                .collect(),
        )
    }

    /// Atomically commits a complete container slot vector after validating the
    /// target entity and exact length.  This prevents a malformed/stale click
    /// from partially replacing a multi-slot inventory.
    pub fn set_container_slots(
        chunk_manager: &mut ChunkManager,
        x: i32,
        y: i32,
        z: i32,
        slots: &[Option<ItemStack>],
    ) -> bool {
        if matches!(
            chunk_manager.get_block_entity(x, y, z),
            Some(BlockEntity::Chest(_))
        ) {
            return Self::set_chest_slots(chunk_manager, x, y, z, slots);
        }
        let Some(mut entity) = chunk_manager.get_block_entity(x, y, z).cloned() else {
            return false;
        };
        if slots.len() != entity.slot_count() {
            return false;
        }
        if !entity.replace_slots(slots) {
            return false;
        }
        chunk_manager.set_block_entity(x, y, z, Some(entity));
        true
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

    #[test]
    fn container_slot_commit_rejects_invalid_stack_without_partial_write() {
        let mut chunk_manager = ChunkManager::new(2);
        chunk_manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        chunk_manager.set_block(0, 64, 0, crate::world::BlockType::Chest);
        chunk_manager.set_block_entity(
            0,
            64,
            0,
            Some(BlockEntity::Chest(
                crate::block_entity::ChestBlockEntity::new(),
            )),
        );
        let original =
            ContainerSessionManager::get_container_slots(&chunk_manager, 0, 64, 0).unwrap();
        let mut invalid = original.clone();
        invalid[3] = Some(ItemStack::new(Item::Stone, 0));

        assert!(!ContainerSessionManager::set_container_slots(
            &mut chunk_manager,
            0,
            64,
            0,
            &invalid,
        ));
        assert_eq!(
            ContainerSessionManager::get_container_slots(&chunk_manager, 0, 64, 0).unwrap(),
            original
        );
    }
}

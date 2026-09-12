use super::gen::*;
use super::placement::{
    get_structure_candidate_in_region, origin_y_for, END_CITY_BASE_Y, END_CITY_X, END_CITY_Z,
};
use super::types::*;
use crate::dimension::Dimension;
use crate::world::Chunk;
use crate::world::chunk_origin;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct StructureManager {
    starts: Mutex<HashMap<(u32, Dimension, i32, i32), Vec<StructureStart>>>,
}

impl StructureManager {
    pub fn new() -> Self {
        Self {
            starts: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_or_generate_starts(
        &self,
        dimension: Dimension,
        seed: u32,
        region_x: i32,
        region_z: i32,
    ) -> Vec<StructureStart> {
        let key = (seed, dimension, region_x, region_z);
        {
            let lock = self.starts.lock().unwrap();
            if let Some(starts) = lock.get(&key) {
                return starts.clone();
            }
        }

        let mut generated = Vec::new();
        let ids = [
            StructureId::Dungeon,
            StructureId::Mineshaft,
            StructureId::Village,
            StructureId::Stronghold,
            StructureId::NetherFortress,
            StructureId::EndCity,
        ];

        for &id in &ids {
            if let Some((chunk_x, chunk_z)) =
                get_structure_candidate_in_region(id, dimension, seed, region_x, region_z)
            {
                let origin_x = chunk_origin(chunk_x) + 2;
                let origin_z = chunk_origin(chunk_z) + 2;
                let origin_y = origin_y_for(id, seed, chunk_x, chunk_z);

                let start = match id {
                    StructureId::Dungeon => {
                        dungeon::generate_dungeon(origin_x, origin_y, origin_z, seed)
                    }
                    StructureId::Mineshaft => {
                        mineshaft::generate_mineshaft(origin_x, origin_y, origin_z, seed)
                    }
                    StructureId::Village => {
                        village::generate_village(origin_x, origin_y, origin_z, seed)
                    }
                    StructureId::Stronghold => {
                        stronghold::generate_stronghold(origin_x, origin_y, origin_z, seed)
                    }
                    StructureId::NetherFortress => nether_fortress::generate_nether_fortress(
                        origin_x, origin_y, origin_z, seed,
                    ),
                    StructureId::EndCity => {
                        end_city::generate_end_city(origin_x, origin_y, origin_z, seed)
                    }
                };
                generated.push(start);
            }
        }

        // Pin the familiar fixed End City through the manager (not dimension.rs).
        if dimension == Dimension::End {
            let already = generated.iter().any(|s| {
                s.id == StructureId::EndCity
                    && s.origin_x == END_CITY_X
                    && s.origin_z == END_CITY_Z
            });
            if !already {
                generated.push(end_city::generate_end_city(
                    END_CITY_X,
                    END_CITY_BASE_Y,
                    END_CITY_Z,
                    seed,
                ));
            }
        }

        let mut lock = self.starts.lock().unwrap();
        lock.insert(key, generated.clone());
        generated
    }

    pub fn apply_structures_to_chunk(&self, chunk: &mut Chunk, dimension: Dimension, seed: u32) {
        let chunk_x = chunk.chunk_x;
        let chunk_z = chunk.chunk_z;
        let c_min_x = chunk_origin(chunk_x);
        let c_min_z = chunk_origin(chunk_z);

        let grid_size = 24;
        let reg_x = chunk_x.div_euclid(grid_size);
        let reg_z = chunk_z.div_euclid(grid_size);

        for rx in (reg_x - 1)..=(reg_x + 1) {
            for rz in (reg_z - 1)..=(reg_z + 1) {
                let starts = self.get_or_generate_starts(dimension, seed, rx, rz);
                for start in starts {
                    if !start.bounding_box.intersects_chunk(chunk_x, chunk_z) {
                        continue;
                    }

                    for piece in &start.pieces {
                        if !piece.bounding_box.intersects_chunk(chunk_x, chunk_z) {
                            continue;
                        }
                        apply_piece_to_chunk(chunk, piece, c_min_x, c_min_z);
                    }
                }
            }
        }
    }
}

/// Write placements that already sit in this column. Piece was filtered by
/// `intersects_chunk`; local indices use column origin (no per-block `chunk_xz`).
fn apply_piece_to_chunk(
    chunk: &mut Chunk,
    piece: &StructurePiece,
    c_min_x: i32,
    c_min_z: i32,
) {
    let c_max_x = c_min_x + 15;
    let c_max_z = c_min_z + 15;
    for block in &piece.blocks {
        if block.world_x < c_min_x
            || block.world_x > c_max_x
            || block.world_z < c_min_z
            || block.world_z > c_max_z
        {
            continue;
        }
        let lx = (block.world_x - c_min_x) as usize;
        let lz = (block.world_z - c_min_z) as usize;
        let wy = block.world_y;

        chunk.set_block_local(lx, wy, lz, block.block_type);
        if block.block_state != 0 {
            chunk.set_block_state(lx as i32, wy, lz as i32, block.block_state);
        }
        if let Some(entity) = &block.block_entity {
            let _ = chunk.insert_block_entity(lx as u8, wy as i16, lz as u8, entity.clone());
        }
    }
}

impl Default for StructureManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_entity::BlockEntity;
    use crate::dimension::Dimension;
    use crate::world::{BlockType, Chunk};

    #[test]
    fn test_structure_manager_determinism() {
        let manager = StructureManager::new();
        let seed = 9876543;

        let starts1 = manager.get_or_generate_starts(Dimension::Overworld, seed, 0, 0);
        let starts2 = manager.get_or_generate_starts(Dimension::Overworld, seed, 0, 0);

        assert_eq!(starts1.len(), starts2.len());
        for (s1, s2) in starts1.iter().zip(starts2.iter()) {
            assert_eq!(s1.id, s2.id);
            assert_eq!(s1.origin_x, s2.origin_x);
            assert_eq!(s1.origin_z, s2.origin_z);
            assert_eq!(s1.pieces.len(), s2.pieces.len());
        }
    }

    #[test]
    fn structure_cache_does_not_share_across_seeds() {
        let manager = StructureManager::new();
        let seed_a = 1u32;
        let seed_b = 999_999u32;
        let starts_a = manager.get_or_generate_starts(Dimension::Overworld, seed_a, 0, 0);
        let starts_b = manager.get_or_generate_starts(Dimension::Overworld, seed_b, 0, 0);
        let starts_a_again = manager.get_or_generate_starts(Dimension::Overworld, seed_a, 0, 0);

        let key = |starts: &[StructureStart]| {
            starts
                .iter()
                .map(|s| (s.id, s.origin_x, s.origin_y, s.origin_z))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&starts_a), key(&starts_a_again));
        assert_ne!(
            key(&starts_a),
            key(&starts_b),
            "two seeds must not reuse the same region cache entry"
        );
    }

    #[test]
    fn test_apply_structures_to_chunk() {
        let manager = StructureManager::new();
        let seed = 42;
        let mut chunk = Chunk::new_with_seed(0, 0, seed);

        manager.apply_structures_to_chunk(&mut chunk, Dimension::Overworld, seed);

        let block_entities = chunk.iter_block_entities();
        for ((_x, _wy, _z), entity) in block_entities {
            match entity {
                BlockEntity::Chest(c) => {
                    if let Some(table) = &c.loot_table {
                        assert!(table.starts_with("chests/"));
                    }
                }
                BlockEntity::Spawner(s) => {
                    assert!(s.spawn_delay > 0);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn end_pins_fixed_city_and_apply_writes_chest() {
        let manager = StructureManager::new();
        let seed = 7;
        let starts = manager.get_or_generate_starts(Dimension::End, seed, 2, 0);
        let pinned = starts
            .iter()
            .find(|s| {
                s.id == StructureId::EndCity
                    && s.origin_x == END_CITY_X
                    && s.origin_z == END_CITY_Z
            })
            .expect("pinned End City");
        assert_eq!(pinned.origin_y, END_CITY_BASE_Y);

        let mut chunk = Chunk::empty_in_dimension(Dimension::End, 64, 0);
        manager.apply_structures_to_chunk(&mut chunk, Dimension::End, seed);
        assert_eq!(chunk.get_block_local(11, 89, 11), BlockType::Chest);
    }
}

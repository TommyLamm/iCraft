use super::gen::*;
use super::placement::get_structure_candidate_in_region;
use super::types::*;
use crate::dimension::Dimension;
use crate::world::Chunk;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct StructureManager {
    starts: Mutex<HashMap<(Dimension, i32, i32), Vec<StructureStart>>>,
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
        let key = (dimension, region_x, region_z);
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
                let origin_x = chunk_x * 16 + 2;
                let origin_z = chunk_z * 16 + 2;
                let origin_y = match id {
                    StructureId::Dungeon => 20 + ((seed.wrapping_add(chunk_x as u32) % 30) as i32),
                    StructureId::Mineshaft => 25,
                    StructureId::Village => 64,
                    StructureId::Stronghold => 22,
                    StructureId::NetherFortress => 55,
                    StructureId::EndCity => 64,
                };

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

        let mut lock = self.starts.lock().unwrap();
        lock.insert(key, generated.clone());
        generated
    }

    pub fn apply_structures_to_chunk(&self, chunk: &mut Chunk, dimension: Dimension, seed: u32) {
        let chunk_x = chunk.chunk_x;
        let chunk_z = chunk.chunk_z;

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

                        for block in &piece.blocks {
                            let b_chunk_x = block.world_x.div_euclid(16);
                            let b_chunk_z = block.world_z.div_euclid(16);

                            if b_chunk_x == chunk_x && b_chunk_z == chunk_z {
                                let lx = block.world_x.rem_euclid(16) as usize;
                                let lz = block.world_z.rem_euclid(16) as usize;
                                let wy = block.world_y;

                                // Set block in chunk
                                chunk.set_block_local(lx, wy, lz, block.block_type);

                                // Add block entity if present
                                if let Some(entity) = &block.block_entity {
                                    let _ = chunk.insert_block_entity(
                                        lx as u8,
                                        wy as i16,
                                        lz as u8,
                                        entity.clone(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
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
    use crate::world::Chunk;

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
    fn test_apply_structures_to_chunk() {
        let manager = StructureManager::new();
        let seed = 42;
        let mut chunk = Chunk::new_with_seed(0, 0, seed);

        manager.apply_structures_to_chunk(&mut chunk, Dimension::Overworld, seed);

        // Verify chunk contains block entities if chests/spawners were placed
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
}

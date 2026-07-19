use crate::world::{BlockType, CHUNK_HEIGHT};
use crate::chunk_manager::ChunkManager;
use std::collections::{HashSet, VecDeque, HashMap};

pub fn tick_fluids(chunk_manager: &mut ChunkManager, is_lava: bool) -> HashSet<(i32, i32)> {
    let mut dirty_chunks = HashSet::new();
    let target_type = if is_lava { BlockType::Lava } else { BlockType::Water };
    let other_type = if is_lava { BlockType::Water } else { BlockType::Lava };
    let flow_limit = 7; // Max level is 7 (thinnest)

    // 1. Handle Infinite Water Sources (only for water)
    if !is_lava {
        let mut new_sources = Vec::new();
        // Check all columns in loaded chunks
        let chunk_coords: Vec<(i32, i32)> = chunk_manager.chunks.keys().cloned().collect();
        for &(cx, cz) in &chunk_coords {
            for x in 0..16 {
                for z in 0..16 {
                    let wx = cx * 16 + x as i32;
                    let wz = cz * 16 + z as i32;
                    // Scan vertically
                    for wy in 1..CHUNK_HEIGHT as i32 {
                        let block = chunk_manager.get_block(wx, wy, wz);
                        // Can only become source if it's currently air or flowing water
                        if block == BlockType::Air || (block == BlockType::Water && chunk_manager.get_fluid_level(wx, wy, wz) > 0) {
                            // Check if the block below is solid or water
                            let below = chunk_manager.get_block(wx, wy - 1, wz);
                            if below != BlockType::Air && below != BlockType::Lava {
                                // Count horizontal source water neighbors
                                let mut source_count = 0;
                                let neighbors = [
                                    (wx + 1, wy, wz),
                                    (wx - 1, wy, wz),
                                    (wx, wy, wz + 1),
                                    (wx, wy, wz - 1),
                                ];
                                for &(nx, ny, nz) in &neighbors {
                                    if chunk_manager.get_block(nx, ny, nz) == BlockType::Water
                                       && chunk_manager.get_fluid_level(nx, ny, nz) == 0
                                       && !chunk_manager.get_fluid_falling(nx, ny, nz)
                                    {
                                        source_count += 1;
                                    }
                                }
                                if source_count >= 2 {
                                    new_sources.push((wx, wy, wz));
                                }
                            }
                        }
                    }
                }
            }
        }
        for (wx, wy, wz) in new_sources {
            chunk_manager.set_block(wx, wy, wz, BlockType::Water);
            chunk_manager.set_fluid_level(wx, wy, wz, 0);
            chunk_manager.set_fluid_falling(wx, wy, wz, false);
            dirty_chunks.insert((wx.div_euclid(16), wz.div_euclid(16)));
        }
    }

    // 2. Gather all fluid source blocks (level == 0, falling == false)
    let mut queue = VecDeque::new();
    let mut visited = HashMap::new(); // (wx, wy, wz) -> (level, falling)

    for (&(cx, cz), chunk) in chunk_manager.chunks.iter() {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..CHUNK_HEIGHT {
                    let block = chunk.blocks[x][y][z];
                    if block == target_type {
                        let wx = cx * 16 + x as i32;
                        let wy = y as i32;
                        let wz = cz * 16 + z as i32;
                        let level = chunk.fluid_levels[x][y][z] & 0x07;
                        let falling = (chunk.fluid_levels[x][y][z] & 0x08) != 0;

                        // Source blocks are level 0 and not falling
                        if level == 0 && !falling {
                            queue.push_back((wx, wy, wz, 0, false));
                            visited.insert((wx, wy, wz), (0, false));
                        }
                    }
                }
            }
        }
    }

    // 3. Run BFS to propagate fluids
    while let Some((wx, wy, wz, level, falling)) = queue.pop_front() {
        // A. Flow downwards
        if wy > 0 {
            let below_x = wx;
            let below_y = wy - 1;
            let below_z = wz;
            let below_cx = below_x.div_euclid(16);
            let below_cz = below_z.div_euclid(16);

            if chunk_manager.chunks.contains_key(&(below_cx, below_cz)) {
                let below_block = chunk_manager.get_block(below_x, below_y, below_z);
                if below_block == BlockType::Air 
                   || below_block.properties().is_passable 
                   || below_block == target_type 
                   || below_block == other_type 
                {
                    let next_level = 0;
                    let next_falling = true;
                    let entry = visited.get(&(below_x, below_y, below_z));
                    if entry.is_none() || entry.unwrap().0 > next_level {
                        visited.insert((below_x, below_y, below_z), (next_level, next_falling));
                        queue.push_back((below_x, below_y, below_z, next_level, next_falling));
                    }
                }
            }
        }

        // B. Flow horizontally if we are not falling, OR if the block below is solid (cannot flow down)
        let can_flow_horizontally = if falling {
            if wy > 0 {
                let below_block = chunk_manager.get_block(wx, wy - 1, wz);
                // Cannot flow down further -> must flow horizontally
                below_block != BlockType::Air 
                && !below_block.properties().is_passable 
                && below_block != target_type 
                && below_block != other_type
            } else {
                true // Bottom of the world
            }
        } else {
            true
        };

        if can_flow_horizontally && level < flow_limit {
            let neighbors = [
                (wx + 1, wy, wz),
                (wx - 1, wy, wz),
                (wx, wy, wz + 1),
                (wx, wy, wz - 1),
            ];
            for &(nx, ny, nz) in &neighbors {
                let ncx = nx.div_euclid(16);
                let ncz = nz.div_euclid(16);
                if chunk_manager.chunks.contains_key(&(ncx, ncz)) {
                    let n_block = chunk_manager.get_block(nx, ny, nz);
                    if n_block == BlockType::Air 
                       || n_block.properties().is_passable 
                       || n_block == target_type 
                       || n_block == other_type 
                    {
                        let next_level = level + 1;
                        let next_falling = false;
                        let entry = visited.get(&(nx, ny, nz));
                        if entry.is_none() || entry.unwrap().0 > next_level {
                            visited.insert((nx, ny, nz), (next_level, next_falling));
                            queue.push_back((nx, ny, nz, next_level, next_falling));
                        }
                    }
                }
            }
        }
    }

    // 4. Update the world based on the BFS results
    let chunk_coords: Vec<(i32, i32)> = chunk_manager.chunks.keys().cloned().collect();
    for &(cx, cz) in &chunk_coords {
        let mut chunk_dirty = false;
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..CHUNK_HEIGHT {
                    let wx = cx * 16 + x as i32;
                    let wy = y as i32;
                    let wz = cz * 16 + z as i32;
                    let current_block = chunk_manager.get_block(wx, wy, wz);

                    if current_block == target_type {
                        let current_level = chunk_manager.get_fluid_level(wx, wy, wz);
                        let current_falling = chunk_manager.get_fluid_falling(wx, wy, wz);
                        // Is it a permanent source? (placed by player/generated, level 0 and not falling)
                        if current_level == 0 && !current_falling {
                            continue;
                        }

                        if let Some(&(new_level, new_falling)) = visited.get(&(wx, wy, wz)) {
                            if current_level != new_level || current_falling != new_falling {
                                chunk_manager.set_fluid_level(wx, wy, wz, new_level);
                                chunk_manager.set_fluid_falling(wx, wy, wz, new_falling);
                                chunk_dirty = true;
                            }
                        } else {
                            // Decayed completely
                            chunk_manager.set_block(wx, wy, wz, BlockType::Air);
                            chunk_manager.set_fluid_level(wx, wy, wz, 0);
                            chunk_manager.set_fluid_falling(wx, wy, wz, false);
                            chunk_dirty = true;
                        }
                    } else if current_block == BlockType::Air || current_block.properties().is_passable || current_block == other_type {
                        if let Some(&(new_level, new_falling)) = visited.get(&(wx, wy, wz)) {
                            // Water/Lava interaction checks
                            let mut resolved = false;
                            if current_block == other_type {
                                let other_level = chunk_manager.get_fluid_level(wx, wy, wz);
                                if target_type == BlockType::Water {
                                    // Water flows into Lava
                                    if other_level == 0 {
                                        chunk_manager.set_block(wx, wy, wz, BlockType::Obsidian);
                                    } else {
                                        chunk_manager.set_block(wx, wy, wz, BlockType::Cobblestone);
                                    }
                                    chunk_manager.set_fluid_level(wx, wy, wz, 0);
                                    chunk_manager.set_fluid_falling(wx, wy, wz, false);
                                    resolved = true;
                                } else {
                                    // Lava flows into Water
                                    if other_level == 0 {
                                        chunk_manager.set_block(wx, wy, wz, BlockType::Stone);
                                    } else {
                                        chunk_manager.set_block(wx, wy, wz, BlockType::Cobblestone);
                                    }
                                    chunk_manager.set_fluid_level(wx, wy, wz, 0);
                                    chunk_manager.set_fluid_falling(wx, wy, wz, false);
                                    resolved = true;
                                }
                            }

                            if !resolved {
                                chunk_manager.set_block(wx, wy, wz, target_type);
                                chunk_manager.set_fluid_level(wx, wy, wz, new_level);
                                chunk_manager.set_fluid_falling(wx, wy, wz, new_falling);
                            }
                            chunk_dirty = true;
                        }
                    }
                }
            }
        }
        if chunk_dirty {
            dirty_chunks.insert((cx, cz));
        }
    }

    dirty_chunks
}

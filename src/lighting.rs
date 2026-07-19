use crate::chunk_manager::ChunkManager;
use crate::world::{RenderType, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use std::collections::{HashSet, VecDeque};

pub struct LightNode {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

pub struct LightRemovalNode {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub val: u8,
}

pub fn propagate_sky_light(
    chunk_manager: &mut ChunkManager,
    queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let dirs = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    while let Some(node) = queue.pop_front() {
        let current_light = chunk_manager.get_sky_light(node.x, node.y, node.z);
        if current_light <= 1 {
            continue;
        }

        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if ny < 0 || ny >= CHUNK_HEIGHT as i32 {
                continue;
            }

            let neighbor_block = chunk_manager.get_block(nx, ny, nz);
            if neighbor_block.properties().render_type == RenderType::Opaque {
                continue;
            }

            let neighbor_light = chunk_manager.get_sky_light(nx, ny, nz);
            let expected_light = current_light - 1;

            if neighbor_light < expected_light {
                chunk_manager.set_sky_light(nx, ny, nz, expected_light);

                let cx = nx.div_euclid(CHUNK_WIDTH as i32);
                let cz = nz.div_euclid(CHUNK_DEPTH as i32);
                dirty_chunks.insert((cx, cz));

                // Mark neighbors on boundaries dirty
                let lx = nx.rem_euclid(CHUNK_WIDTH as i32);
                let lz = nz.rem_euclid(CHUNK_DEPTH as i32);
                if lx == 0 {
                    dirty_chunks.insert((cx - 1, cz));
                }
                if lx == 15 {
                    dirty_chunks.insert((cx + 1, cz));
                }
                if lz == 0 {
                    dirty_chunks.insert((cx, cz - 1));
                }
                if lz == 15 {
                    dirty_chunks.insert((cx, cz + 1));
                }

                queue.push_back(LightNode {
                    x: nx,
                    y: ny,
                    z: nz,
                });
            }
        }
    }
}

pub fn remove_sky_light(
    chunk_manager: &mut ChunkManager,
    removal_queue: &mut VecDeque<LightRemovalNode>,
    propagate_queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let dirs = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    while let Some(node) = removal_queue.pop_front() {
        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if ny < 0 || ny >= CHUNK_HEIGHT as i32 {
                continue;
            }

            let neighbor_light = chunk_manager.get_sky_light(nx, ny, nz);
            if neighbor_light != 0 && neighbor_light < node.val {
                chunk_manager.set_sky_light(nx, ny, nz, 0);

                let cx = nx.div_euclid(CHUNK_WIDTH as i32);
                let cz = nz.div_euclid(CHUNK_DEPTH as i32);
                dirty_chunks.insert((cx, cz));
                let lx = nx.rem_euclid(CHUNK_WIDTH as i32);
                let lz = nz.rem_euclid(CHUNK_DEPTH as i32);
                if lx == 0 {
                    dirty_chunks.insert((cx - 1, cz));
                }
                if lx == 15 {
                    dirty_chunks.insert((cx + 1, cz));
                }
                if lz == 0 {
                    dirty_chunks.insert((cx, cz - 1));
                }
                if lz == 15 {
                    dirty_chunks.insert((cx, cz + 1));
                }

                removal_queue.push_back(LightRemovalNode {
                    x: nx,
                    y: ny,
                    z: nz,
                    val: neighbor_light,
                });
            } else if neighbor_light >= node.val {
                propagate_queue.push_back(LightNode {
                    x: nx,
                    y: ny,
                    z: nz,
                });
            }
        }
    }
}

pub fn propagate_block_light(
    chunk_manager: &mut ChunkManager,
    queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let dirs = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    while let Some(node) = queue.pop_front() {
        let current_light = chunk_manager.get_block_light(node.x, node.y, node.z);
        if current_light <= 1 {
            continue;
        }

        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if ny < 0 || ny >= CHUNK_HEIGHT as i32 {
                continue;
            }

            let neighbor_block = chunk_manager.get_block(nx, ny, nz);
            if neighbor_block.properties().render_type == RenderType::Opaque {
                continue;
            }

            let neighbor_light = chunk_manager.get_block_light(nx, ny, nz);
            let expected_light = current_light - 1;

            if neighbor_light < expected_light {
                chunk_manager.set_block_light(nx, ny, nz, expected_light);

                let cx = nx.div_euclid(CHUNK_WIDTH as i32);
                let cz = nz.div_euclid(CHUNK_DEPTH as i32);
                dirty_chunks.insert((cx, cz));
                let lx = nx.rem_euclid(CHUNK_WIDTH as i32);
                let lz = nz.rem_euclid(CHUNK_DEPTH as i32);
                if lx == 0 {
                    dirty_chunks.insert((cx - 1, cz));
                }
                if lx == 15 {
                    dirty_chunks.insert((cx + 1, cz));
                }
                if lz == 0 {
                    dirty_chunks.insert((cx, cz - 1));
                }
                if lz == 15 {
                    dirty_chunks.insert((cx, cz + 1));
                }

                queue.push_back(LightNode {
                    x: nx,
                    y: ny,
                    z: nz,
                });
            }
        }
    }
}

pub fn remove_block_light(
    chunk_manager: &mut ChunkManager,
    removal_queue: &mut VecDeque<LightRemovalNode>,
    propagate_queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let dirs = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    while let Some(node) = removal_queue.pop_front() {
        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if ny < 0 || ny >= CHUNK_HEIGHT as i32 {
                continue;
            }

            let neighbor_light = chunk_manager.get_block_light(nx, ny, nz);
            if neighbor_light != 0 && neighbor_light < node.val {
                chunk_manager.set_block_light(nx, ny, nz, 0);

                let cx = nx.div_euclid(CHUNK_WIDTH as i32);
                let cz = nz.div_euclid(CHUNK_DEPTH as i32);
                dirty_chunks.insert((cx, cz));
                let lx = nx.rem_euclid(CHUNK_WIDTH as i32);
                let lz = nz.rem_euclid(CHUNK_DEPTH as i32);
                if lx == 0 {
                    dirty_chunks.insert((cx - 1, cz));
                }
                if lx == 15 {
                    dirty_chunks.insert((cx + 1, cz));
                }
                if lz == 0 {
                    dirty_chunks.insert((cx, cz - 1));
                }
                if lz == 15 {
                    dirty_chunks.insert((cx, cz + 1));
                }

                removal_queue.push_back(LightRemovalNode {
                    x: nx,
                    y: ny,
                    z: nz,
                    val: neighbor_light,
                });
            } else if neighbor_light >= node.val {
                propagate_queue.push_back(LightNode {
                    x: nx,
                    y: ny,
                    z: nz,
                });
            }
        }
    }
}

pub fn update_sky_light_after_placed(
    chunk_manager: &mut ChunkManager,
    wx: i32,
    wy: i32,
    wz: i32,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let mut removal_queue = VecDeque::new();
    let mut propagate_queue = VecDeque::new();

    let block = chunk_manager.get_block(wx, wy, wz);
    if block.properties().render_type != RenderType::Opaque {
        propagate_queue.push_back(LightNode {
            x: wx,
            y: wy,
            z: wz,
        });
        propagate_sky_light(chunk_manager, &mut propagate_queue, dirty_chunks);
        return;
    }

    let old_val = chunk_manager.get_sky_light(wx, wy, wz);
    if old_val > 0 {
        chunk_manager.set_sky_light(wx, wy, wz, 0);
        removal_queue.push_back(LightRemovalNode {
            x: wx,
            y: wy,
            z: wz,
            val: old_val,
        });

        if old_val == 15 {
            for y in (0..wy).rev() {
                let val = chunk_manager.get_sky_light(wx, y, wz);
                if val == 0 {
                    break;
                }
                chunk_manager.set_sky_light(wx, y, wz, 0);
                removal_queue.push_back(LightRemovalNode {
                    x: wx,
                    y: y,
                    z: wz,
                    val,
                });
            }
        }

        remove_sky_light(
            chunk_manager,
            &mut removal_queue,
            &mut propagate_queue,
            dirty_chunks,
        );
        propagate_sky_light(chunk_manager, &mut propagate_queue, dirty_chunks);
    }
}

pub fn update_sky_light_after_removed(
    chunk_manager: &mut ChunkManager,
    wx: i32,
    wy: i32,
    wz: i32,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let mut propagate_queue = VecDeque::new();

    let above_sky = if wy == CHUNK_HEIGHT as i32 - 1 {
        true
    } else {
        chunk_manager.get_sky_light(wx, wy + 1, wz) == 15
    };

    if above_sky {
        for y in (0..=wy).rev() {
            let block = chunk_manager.get_block(wx, y, wz);
            if block.properties().render_type == RenderType::Opaque {
                break;
            }
            chunk_manager.set_sky_light(wx, y, wz, 15);

            let cx = wx.div_euclid(CHUNK_WIDTH as i32);
            let cz = wz.div_euclid(CHUNK_DEPTH as i32);
            dirty_chunks.insert((cx, cz));

            propagate_queue.push_back(LightNode { x: wx, y: y, z: wz });
        }
    } else {
        chunk_manager.set_sky_light(wx, wy, wz, 0);
        let mut max_neighbor = 0;
        let dirs = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        for &(dx, dy, dz) in &dirs {
            let ny = wy + dy;
            if ny >= 0 && ny < CHUNK_HEIGHT as i32 {
                let val = chunk_manager.get_sky_light(wx + dx, ny, wz + dz);
                if val > max_neighbor {
                    max_neighbor = val;
                }
            }
        }
        if max_neighbor > 1 {
            chunk_manager.set_sky_light(wx, wy, wz, max_neighbor - 1);
            propagate_queue.push_back(LightNode {
                x: wx,
                y: wy,
                z: wz,
            });
        }
    }

    propagate_sky_light(chunk_manager, &mut propagate_queue, dirty_chunks);
}

pub fn update_block_light_after_placed(
    chunk_manager: &mut ChunkManager,
    wx: i32,
    wy: i32,
    wz: i32,
    emission: u8,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let mut propagate_queue = VecDeque::new();

    if emission > 0 {
        chunk_manager.set_block_light(wx, wy, wz, emission);
        propagate_queue.push_back(LightNode {
            x: wx,
            y: wy,
            z: wz,
        });
        propagate_block_light(chunk_manager, &mut propagate_queue, dirty_chunks);
    } else {
        let block = chunk_manager.get_block(wx, wy, wz);
        if block.properties().render_type == RenderType::Opaque {
            let old_val = chunk_manager.get_block_light(wx, wy, wz);
            if old_val > 0 {
                chunk_manager.set_block_light(wx, wy, wz, 0);
                let mut removal_queue = VecDeque::new();
                removal_queue.push_back(LightRemovalNode {
                    x: wx,
                    y: wy,
                    z: wz,
                    val: old_val,
                });
                remove_block_light(
                    chunk_manager,
                    &mut removal_queue,
                    &mut propagate_queue,
                    dirty_chunks,
                );
                propagate_block_light(chunk_manager, &mut propagate_queue, dirty_chunks);
            }
        }
    }
}

pub fn update_block_light_after_removed(
    chunk_manager: &mut ChunkManager,
    wx: i32,
    wy: i32,
    wz: i32,
    old_emission: u8,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let mut propagate_queue = VecDeque::new();

    if old_emission > 0 {
        chunk_manager.set_block_light(wx, wy, wz, 0);
        let mut removal_queue = VecDeque::new();
        removal_queue.push_back(LightRemovalNode {
            x: wx,
            y: wy,
            z: wz,
            val: old_emission,
        });
        remove_block_light(
            chunk_manager,
            &mut removal_queue,
            &mut propagate_queue,
            dirty_chunks,
        );
        propagate_block_light(chunk_manager, &mut propagate_queue, dirty_chunks);
    } else {
        chunk_manager.set_block_light(wx, wy, wz, 0);
        let mut max_neighbor = 0;
        let dirs = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        for &(dx, dy, dz) in &dirs {
            let ny = wy + dy;
            if ny >= 0 && ny < CHUNK_HEIGHT as i32 {
                let val = chunk_manager.get_block_light(wx + dx, ny, wz + dz);
                if val > max_neighbor {
                    max_neighbor = val;
                }
            }
        }
        if max_neighbor > 1 {
            chunk_manager.set_block_light(wx, wy, wz, max_neighbor - 1);
            propagate_queue.push_back(LightNode {
                x: wx,
                y: wy,
                z: wz,
            });
        }
        propagate_block_light(chunk_manager, &mut propagate_queue, dirty_chunks);
    }
}

pub fn propagate_chunk_lighting(
    chunk_manager: &mut ChunkManager,
    cx: i32,
    cz: i32,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let mut sky_queue = VecDeque::new();
    let mut block_queue = VecDeque::new();

    let start_x = cx * CHUNK_WIDTH as i32;
    let start_z = cz * CHUNK_DEPTH as i32;

    let dirs = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];

    if let Some(chunk) = chunk_manager.chunks.get(&(cx, cz)) {
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let wx = start_x + x as i32;
                let wz = start_z + z as i32;

                for y in 0..CHUNK_HEIGHT {
                    let wy = y as i32;

                    // 1. Seed sky light only where a loaded, transparent
                    // neighbor actually needs light. Treating every chunk
                    // boundary as dirty creates thousands of useless nodes.
                    let sky_val = chunk.sky_light[x][y][z];
                    if sky_val > 1 {
                        let mut has_darker_neighbor = false;

                        for &(dx, dy, dz) in &dirs {
                            let local_nx = x as i32 + dx;
                            let local_ny = y as i32 + dy;
                            let local_nz = z as i32 + dz;
                            if local_ny < 0 || local_ny >= CHUNK_HEIGHT as i32 {
                                continue;
                            }

                            let (neighbor_block, neighbor_light) = if local_nx >= 0
                                && local_nx < CHUNK_WIDTH as i32
                                && local_nz >= 0
                                && local_nz < CHUNK_DEPTH as i32
                            {
                                (
                                    chunk.blocks[local_nx as usize][local_ny as usize]
                                        [local_nz as usize],
                                    chunk.sky_light[local_nx as usize][local_ny as usize]
                                        [local_nz as usize],
                                )
                            } else {
                                let nx = wx + dx;
                                let nz = wz + dz;
                                let neighbor_cx = nx.div_euclid(CHUNK_WIDTH as i32);
                                let neighbor_cz = nz.div_euclid(CHUNK_DEPTH as i32);
                                if !chunk_manager
                                    .chunks
                                    .contains_key(&(neighbor_cx, neighbor_cz))
                                {
                                    continue;
                                }
                                (
                                    chunk_manager.get_block(nx, local_ny, nz),
                                    chunk_manager.get_sky_light(nx, local_ny, nz),
                                )
                            };

                            if neighbor_block.properties().render_type != RenderType::Opaque
                                && neighbor_light < sky_val - 1
                            {
                                has_darker_neighbor = true;
                                break;
                            }
                        }

                        if has_darker_neighbor {
                            sky_queue.push_back(LightNode {
                                x: wx,
                                y: wy,
                                z: wz,
                            });
                        }
                    }

                    // 2. Apply the same loaded-neighbor check to block light.
                    let block_val = chunk.block_light[x][y][z];
                    if block_val > 1 {
                        let mut has_darker_neighbor = false;

                        for &(dx, dy, dz) in &dirs {
                            let local_nx = x as i32 + dx;
                            let local_ny = y as i32 + dy;
                            let local_nz = z as i32 + dz;
                            if local_ny < 0 || local_ny >= CHUNK_HEIGHT as i32 {
                                continue;
                            }

                            let (neighbor_block, neighbor_light) = if local_nx >= 0
                                && local_nx < CHUNK_WIDTH as i32
                                && local_nz >= 0
                                && local_nz < CHUNK_DEPTH as i32
                            {
                                (
                                    chunk.blocks[local_nx as usize][local_ny as usize]
                                        [local_nz as usize],
                                    chunk.block_light[local_nx as usize][local_ny as usize]
                                        [local_nz as usize],
                                )
                            } else {
                                let nx = wx + dx;
                                let nz = wz + dz;
                                let neighbor_cx = nx.div_euclid(CHUNK_WIDTH as i32);
                                let neighbor_cz = nz.div_euclid(CHUNK_DEPTH as i32);
                                if !chunk_manager
                                    .chunks
                                    .contains_key(&(neighbor_cx, neighbor_cz))
                                {
                                    continue;
                                }
                                (
                                    chunk_manager.get_block(nx, local_ny, nz),
                                    chunk_manager.get_block_light(nx, local_ny, nz),
                                )
                            };

                            if neighbor_block.properties().render_type != RenderType::Opaque
                                && neighbor_light < block_val - 1
                            {
                                has_darker_neighbor = true;
                                break;
                            }
                        }

                        if has_darker_neighbor {
                            block_queue.push_back(LightNode {
                                x: wx,
                                y: wy,
                                z: wz,
                            });
                        }
                    }
                }
            }
        }
    }

    propagate_sky_light(chunk_manager, &mut sky_queue, dirty_chunks);
    propagate_block_light(chunk_manager, &mut block_queue, dirty_chunks);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{BlockType, Chunk};

    #[test]
    fn initial_lighting_reaches_horizontal_cave_entrance() {
        let mut chunk_manager = ChunkManager::new(0);
        let mut chunk = Chunk::new(0, 0);

        // Build a controlled landscape with a directly-lit surface above a
        // cave that is not reached by the vertical initialization pass.
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 0..CHUNK_HEIGHT {
                    if y >= 64 {
                        chunk.blocks[x][y][z] = BlockType::Air;
                        chunk.sky_light[x][y][z] = 15;
                    } else {
                        chunk.blocks[x][y][z] = BlockType::Stone;
                        chunk.sky_light[x][y][z] = 0;
                    }
                    chunk.block_light[x][y][z] = 0;
                }
            }
        }

        // A cave entrance whose light source is queued after those boundary
        // cells, followed by a short horizontal tunnel.
        chunk.blocks[8][63][8] = BlockType::Air;
        for x in 8..=12 {
            chunk.blocks[x][62][8] = BlockType::Air;
        }

        chunk_manager.chunks.insert((0, 0), chunk);
        let mut dirty_chunks = HashSet::new();
        propagate_chunk_lighting(&mut chunk_manager, 0, 0, &mut dirty_chunks);

        assert_eq!(chunk_manager.get_sky_light(8, 63, 8), 14);
        assert_eq!(chunk_manager.get_sky_light(12, 62, 8), 9);
    }

    #[test]
    fn propagation_does_not_discard_work_after_five_thousand_nodes() {
        let mut chunk_manager = ChunkManager::new(0);
        let mut chunk = Chunk::new(0, 0);

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 0..CHUNK_HEIGHT {
                    chunk.blocks[x][y][z] = BlockType::Stone;
                    chunk.sky_light[x][y][z] = 0;
                    chunk.block_light[x][y][z] = 0;
                }
            }
        }

        chunk.blocks[8][64][8] = BlockType::Air;
        chunk.blocks[8][63][8] = BlockType::Air;
        chunk.sky_light[8][64][8] = 15;
        chunk.block_light[8][64][8] = 14;
        chunk_manager.chunks.insert((0, 0), chunk);

        let mut dirty_chunks = HashSet::new();
        let mut sky_queue = VecDeque::new();
        for _ in 0..5_000 {
            sky_queue.push_back(LightNode { x: 0, y: 0, z: 0 });
        }
        sky_queue.push_back(LightNode { x: 8, y: 64, z: 8 });
        propagate_sky_light(&mut chunk_manager, &mut sky_queue, &mut dirty_chunks);
        assert!(sky_queue.is_empty());
        assert_eq!(chunk_manager.get_sky_light(8, 63, 8), 14);

        let mut block_queue = VecDeque::new();
        for _ in 0..5_000 {
            block_queue.push_back(LightNode { x: 0, y: 0, z: 0 });
        }
        block_queue.push_back(LightNode { x: 8, y: 64, z: 8 });
        propagate_block_light(&mut chunk_manager, &mut block_queue, &mut dirty_chunks);
        assert!(block_queue.is_empty());
        assert_eq!(chunk_manager.get_block_light(8, 63, 8), 13);
    }
}

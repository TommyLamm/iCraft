use crate::chunk_manager::ChunkManager;
use crate::world::{Chunk, RenderType, CHUNK_DEPTH, CHUNK_WIDTH, SECTION_SIZE};
use std::collections::{HashSet, VecDeque};

const LIGHT_DIRS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

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
    let height = chunk_manager.dimension.height();

    while let Some(node) = queue.pop_front() {
        let current_light = chunk_manager.get_sky_light(node.x, node.y, node.z);
        if current_light <= 1 {
            continue;
        }

        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if !height.contains_y(ny) {
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
    let height = chunk_manager.dimension.height();

    while let Some(node) = removal_queue.pop_front() {
        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if !height.contains_y(ny) {
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
    let height = chunk_manager.dimension.height();

    while let Some(node) = queue.pop_front() {
        let current_light = chunk_manager.get_block_light(node.x, node.y, node.z);
        if current_light <= 1 {
            continue;
        }

        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if !height.contains_y(ny) {
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
    let height = chunk_manager.dimension.height();

    while let Some(node) = removal_queue.pop_front() {
        for &(dx, dy, dz) in &dirs {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if !height.contains_y(ny) {
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
            let height = chunk_manager.dimension.height();
            for y in (height.min_y()..wy).rev() {
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
    let height = chunk_manager.dimension.height();

    let above_sky = if wy + 1 >= height.max_y_exclusive() {
        chunk_manager.dimension.has_sky_light()
    } else {
        chunk_manager.get_sky_light(wx, wy + 1, wz) == 15
    };

    if above_sky {
        for y in (height.min_y()..=wy).rev() {
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
            if height.contains_y(ny) {
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
            if chunk_manager.dimension.height().contains_y(ny) {
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

fn column_at<'a>(
    neighborhood: &[[Option<&'a Chunk>; 3]; 3],
    origin_cx: i32,
    origin_cz: i32,
    wx: i32,
    wz: i32,
) -> Option<&'a Chunk> {
    let cx = wx.div_euclid(CHUNK_WIDTH as i32);
    let cz = wz.div_euclid(CHUNK_DEPTH as i32);
    let dx = cx - origin_cx;
    let dz = cz - origin_cz;
    if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
        return None;
    }
    neighborhood[(dz + 1) as usize][(dx + 1) as usize]
}

fn neighbor_needs_propagation(
    neighborhood: &[[Option<&Chunk>; 3]; 3],
    origin_cx: i32,
    origin_cz: i32,
    wx: i32,
    wy: i32,
    wz: i32,
    light: u8,
    height: crate::dimension::WorldHeight,
    get_light: fn(&Chunk, usize, i32, usize) -> u8,
) -> bool {
    if light <= 1 {
        return false;
    }
    for &(dx, dy, dz) in &LIGHT_DIRS {
        let nx = wx + dx;
        let ny = wy + dy;
        let nz = wz + dz;
        if !height.contains_y(ny) {
            continue;
        }
        let Some(chunk) = column_at(neighborhood, origin_cx, origin_cz, nx, nz) else {
            continue;
        };
        let bx = nx.rem_euclid(CHUNK_WIDTH as i32) as usize;
        let bz = nz.rem_euclid(CHUNK_DEPTH as i32) as usize;
        if chunk.get_block_local(bx, ny, bz).properties().render_type == RenderType::Opaque {
            continue;
        }
        if get_light(chunk, bx, ny, bz) < light - 1 {
            return true;
        }
    }
    false
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
    let height = chunk_manager.dimension.height();
    {
        let neighborhood = chunk_manager.column_neighborhood(cx, cz);
        let Some(chunk) = neighborhood[1][1] else {
            // Unloaded columns have nothing to seed; BFS below is a no-op.
            return;
        };

        for (sec_idx, section) in chunk.sections.iter().enumerate() {
            if section.is_none() {
                continue;
            }
            let base_y = chunk.section_y_at_index(sec_idx) as i32 * SECTION_SIZE as i32;
            for x in 0..CHUNK_WIDTH {
                for z in 0..CHUNK_DEPTH {
                    let wx = start_x + x as i32;
                    let wz = start_z + z as i32;
                    for ly in 0..SECTION_SIZE {
                        let wy = base_y + ly as i32;
                        if !height.contains_y(wy) {
                            continue;
                        }
                        let sky_val = chunk.get_sky_light(x, wy, z);
                        if neighbor_needs_propagation(
                            &neighborhood,
                            cx,
                            cz,
                            wx,
                            wy,
                            wz,
                            sky_val,
                            height,
                            Chunk::get_sky_light,
                        ) {
                            sky_queue.push_back(LightNode {
                                x: wx,
                                y: wy,
                                z: wz,
                            });
                        }
                        let block_val = chunk.get_block_light(x, wy, z);
                        if neighbor_needs_propagation(
                            &neighborhood,
                            cx,
                            cz,
                            wx,
                            wy,
                            wz,
                            block_val,
                            height,
                            Chunk::get_block_light,
                        ) {
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
    use crate::world::{BlockType, Chunk, CHUNK_HEIGHT};

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
                        chunk.set_block_local(x, y as i32, z, BlockType::Air);
                        chunk.set_sky_light(x, y as i32, z, 15);
                    } else {
                        chunk.set_block_local(x, y as i32, z, BlockType::Stone);
                        chunk.set_sky_light(x, y as i32, z, 0);
                    }
                    chunk.set_block_light(x, y as i32, z, 0);
                }
            }
        }

        // A cave entrance whose light source is queued after those boundary
        // cells, followed by a short horizontal tunnel.
        chunk.set_block_local(8, 63, 8, BlockType::Air);
        for x in 8..=12 {
            chunk.set_block_local(x, 62, 8, BlockType::Air);
        }

        chunk_manager.chunks.insert((0, 0), chunk);
        let mut dirty_chunks = HashSet::new();
        propagate_chunk_lighting(&mut chunk_manager, 0, 0, &mut dirty_chunks);

        assert_eq!(chunk_manager.get_sky_light(8, 63, 8), 14);
        assert_eq!(chunk_manager.get_sky_light(12, 62, 8), 9);
    }

    #[test]
    fn load_lighting_seeds_across_chunk_faces() {
        let mut chunk_manager = ChunkManager::new(0);
        let mut west = Chunk::empty(0, 0);
        let mut east = Chunk::empty(1, 0);
        for z in 0..CHUNK_DEPTH {
            for y in 60..70 {
                west.set_block_local(15, y, z, BlockType::Air);
                west.set_sky_light(15, y, z, 15);
                east.set_block_local(0, y, z, BlockType::Air);
                east.set_sky_light(0, y, z, 0);
            }
        }
        chunk_manager.chunks.insert((0, 0), west);
        chunk_manager.chunks.insert((1, 0), east);
        let mut dirty = HashSet::new();
        propagate_chunk_lighting(&mut chunk_manager, 0, 0, &mut dirty);
        assert_eq!(chunk_manager.get_sky_light(16, 64, 8), 14);
    }

    #[test]
    fn propagation_does_not_discard_work_after_five_thousand_nodes() {
        let mut chunk_manager = ChunkManager::new(0);
        let mut chunk = Chunk::new(0, 0);

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 0..CHUNK_HEIGHT {
                    chunk.set_block_local(x, y as i32, z, BlockType::Stone);
                    chunk.set_sky_light(x, y as i32, z, 0);
                    chunk.set_block_light(x, y as i32, z, 0);
                }
            }
        }

        chunk.set_block_local(8, 64, 8, BlockType::Air);
        chunk.set_block_local(8, 63, 8, BlockType::Air);
        chunk.set_sky_light(8, 64, 8, 15);
        chunk.set_block_light(8, 64, 8, 14);
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

    fn empty_fully_lit_overworld_column() -> ChunkManager {
        let mut chunk_manager = ChunkManager::new(0);
        let mut chunk = Chunk::empty(0, 0);
        let height = crate::dimension::WorldHeight::OVERWORLD;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for wy in height.min_y()..height.max_y_exclusive() {
                    chunk.set_block_local(x, wy, z, BlockType::Air);
                    chunk.set_sky_light(x, wy, z, 15);
                    chunk.set_block_light(x, wy, z, 0);
                }
            }
        }
        chunk_manager.chunks.insert((0, 0), chunk);
        chunk_manager
    }

    #[test]
    fn placing_opaque_at_y5_zeros_sky_below_zero() {
        let mut chunk_manager = empty_fully_lit_overworld_column();
        // A full Y=5 slab so neighboring columns cannot refill sky from the side.
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                chunk_manager.set_block(x as i32, 5, z as i32, BlockType::Stone);
            }
        }
        let mut dirty_chunks = HashSet::new();
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                update_sky_light_after_placed(
                    &mut chunk_manager,
                    x as i32,
                    5,
                    z as i32,
                    &mut dirty_chunks,
                );
            }
        }
        assert_eq!(chunk_manager.get_sky_light(8, 5, 8), 0);
        assert_eq!(chunk_manager.get_sky_light(8, -8, 8), 0);
    }

    #[test]
    fn breaking_block_at_y256_receives_sky_from_y257() {
        let mut chunk_manager = empty_fully_lit_overworld_column();
        chunk_manager.set_block(8, 256, 8, BlockType::Stone);
        chunk_manager.set_sky_light(8, 256, 8, 0);
        for y in crate::dimension::WorldHeight::OVERWORLD.min_y()..256 {
            chunk_manager.set_sky_light(8, y, 8, 0);
        }
        assert_eq!(chunk_manager.get_sky_light(8, 257, 8), 15);

        chunk_manager.set_block(8, 256, 8, BlockType::Air);
        let mut dirty_chunks = HashSet::new();
        update_sky_light_after_removed(&mut chunk_manager, 8, 256, 8, &mut dirty_chunks);
        assert_eq!(chunk_manager.get_sky_light(8, 256, 8), 15);
    }

    #[test]
    fn debug_network_restore_cannot_reseed_sky() {
        let mut src = empty_fully_lit_overworld_column();
        src.set_block(8, 70, 8, BlockType::Stone);
        src.set_sky_light(8, 70, 8, 0);
        let chunk = src.chunks.get(&(0, 0)).unwrap();
        let save = crate::save::ChunkSaveData::from_chunk(chunk).unwrap();
        let mut dst = Chunk::empty(0, 0);
        crate::save::format::ChunkSaveData::restore_network_payload(
            &mut dst,
            &save.blocks,
            &save.block_states,
            &save.fluid_levels,
            &save.block_entities,
        )
        .unwrap();
        let sky_after_restore = dst.get_sky_light(8, 71, 8);
        let mut restored_manager = ChunkManager::new(0);
        restored_manager.chunks.insert((0, 0), dst);
        let mut dirty = HashSet::new();
        propagate_chunk_lighting(&mut restored_manager, 0, 0, &mut dirty);
        let sky_after_propagate = restored_manager.get_sky_light(8, 71, 8);
        assert_eq!(sky_after_restore, 15);
        assert_eq!(sky_after_propagate, 15);
    }
}

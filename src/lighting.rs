use crate::chunk_manager::ChunkManager;
use crate::world::{CHUNK_WIDTH, CHUNK_HEIGHT, CHUNK_DEPTH, RenderType};
use std::collections::{VecDeque, HashSet};

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
        (1, 0, 0), (-1, 0, 0),
        (0, 1, 0), (0, -1, 0),
        (0, 0, 1), (0, 0, -1),
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
                if lx == 0 { dirty_chunks.insert((cx - 1, cz)); }
                if lx == 15 { dirty_chunks.insert((cx + 1, cz)); }
                if lz == 0 { dirty_chunks.insert((cx, cz - 1)); }
                if lz == 15 { dirty_chunks.insert((cx, cz + 1)); }

                queue.push_back(LightNode { x: nx, y: ny, z: nz });
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
        (1, 0, 0), (-1, 0, 0),
        (0, 1, 0), (0, -1, 0),
        (0, 0, 1), (0, 0, -1),
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
                if lx == 0 { dirty_chunks.insert((cx - 1, cz)); }
                if lx == 15 { dirty_chunks.insert((cx + 1, cz)); }
                if lz == 0 { dirty_chunks.insert((cx, cz - 1)); }
                if lz == 15 { dirty_chunks.insert((cx, cz + 1)); }

                removal_queue.push_back(LightRemovalNode { x: nx, y: ny, z: nz, val: neighbor_light });
            } else if neighbor_light >= node.val {
                propagate_queue.push_back(LightNode { x: nx, y: ny, z: nz });
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
        (1, 0, 0), (-1, 0, 0),
        (0, 1, 0), (0, -1, 0),
        (0, 0, 1), (0, 0, -1),
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
                if lx == 0 { dirty_chunks.insert((cx - 1, cz)); }
                if lx == 15 { dirty_chunks.insert((cx + 1, cz)); }
                if lz == 0 { dirty_chunks.insert((cx, cz - 1)); }
                if lz == 15 { dirty_chunks.insert((cx, cz + 1)); }

                queue.push_back(LightNode { x: nx, y: ny, z: nz });
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
        (1, 0, 0), (-1, 0, 0),
        (0, 1, 0), (0, -1, 0),
        (0, 0, 1), (0, 0, -1),
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
                if lx == 0 { dirty_chunks.insert((cx - 1, cz)); }
                if lx == 15 { dirty_chunks.insert((cx + 1, cz)); }
                if lz == 0 { dirty_chunks.insert((cx, cz - 1)); }
                if lz == 15 { dirty_chunks.insert((cx, cz + 1)); }

                removal_queue.push_back(LightRemovalNode { x: nx, y: ny, z: nz, val: neighbor_light });
            } else if neighbor_light >= node.val {
                propagate_queue.push_back(LightNode { x: nx, y: ny, z: nz });
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
        propagate_queue.push_back(LightNode { x: wx, y: wy, z: wz });
        propagate_sky_light(chunk_manager, &mut propagate_queue, dirty_chunks);
        return;
    }

    let old_val = chunk_manager.get_sky_light(wx, wy, wz);
    if old_val > 0 {
        chunk_manager.set_sky_light(wx, wy, wz, 0);
        removal_queue.push_back(LightRemovalNode { x: wx, y: wy, z: wz, val: old_val });

        if old_val == 15 {
            for y in (0..wy).rev() {
                let val = chunk_manager.get_sky_light(wx, y, wz);
                if val == 0 {
                    break;
                }
                chunk_manager.set_sky_light(wx, y, wz, 0);
                removal_queue.push_back(LightRemovalNode { x: wx, y: y, z: wz, val });
            }
        }

        remove_sky_light(chunk_manager, &mut removal_queue, &mut propagate_queue, dirty_chunks);
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
            (1, 0, 0), (-1, 0, 0),
            (0, 1, 0), (0, -1, 0),
            (0, 0, 1), (0, 0, -1),
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
            propagate_queue.push_back(LightNode { x: wx, y: wy, z: wz });
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
        propagate_queue.push_back(LightNode { x: wx, y: wy, z: wz });
        propagate_block_light(chunk_manager, &mut propagate_queue, dirty_chunks);
    } else {
        let block = chunk_manager.get_block(wx, wy, wz);
        if block.properties().render_type == RenderType::Opaque {
            let old_val = chunk_manager.get_block_light(wx, wy, wz);
            if old_val > 0 {
                chunk_manager.set_block_light(wx, wy, wz, 0);
                let mut removal_queue = VecDeque::new();
                removal_queue.push_back(LightRemovalNode { x: wx, y: wy, z: wz, val: old_val });
                remove_block_light(chunk_manager, &mut removal_queue, &mut propagate_queue, dirty_chunks);
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
        removal_queue.push_back(LightRemovalNode { x: wx, y: wy, z: wz, val: old_emission });
        remove_block_light(chunk_manager, &mut removal_queue, &mut propagate_queue, dirty_chunks);
        propagate_block_light(chunk_manager, &mut propagate_queue, dirty_chunks);
    } else {
        chunk_manager.set_block_light(wx, wy, wz, 0);
        let mut max_neighbor = 0;
        let dirs = [
            (1, 0, 0), (-1, 0, 0),
            (0, 1, 0), (0, -1, 0),
            (0, 0, 1), (0, 0, -1),
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
            propagate_queue.push_back(LightNode { x: wx, y: wy, z: wz });
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

    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            let wx = start_x + x as i32;
            let wz = start_z + z as i32;
            
            for y in 0..CHUNK_HEIGHT {
                let wy = y as i32;
                
                let sky_val = chunk_manager.get_sky_light(wx, wy, wz);
                if sky_val > 1 {
                    sky_queue.push_back(LightNode { x: wx, y: wy, z: wz });
                  }

                let block_val = chunk_manager.get_block_light(wx, wy, wz);
                if block_val > 1 {
                    block_queue.push_back(LightNode { x: wx, y: wy, z: wz });
                }
            }
        }
    }

    propagate_sky_light(chunk_manager, &mut sky_queue, dirty_chunks);
    propagate_block_light(chunk_manager, &mut block_queue, dirty_chunks);
}

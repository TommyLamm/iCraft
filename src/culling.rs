use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{channel, sync_channel, Receiver, SyncSender};
use std::thread;
use glam::Vec3;

use crate::chunk_manager::ChunkManager;
use crate::chunk_render::{Frustum, MeshBounds};
use crate::entity::{Entity, EntityType};

/// Pairwise face connectivity bitmask for a 16x16x16 section.
/// Faces: 0 (+X), 1 (-X), 2 (+Y), 3 (-Y), 4 (+Z), 5 (-Z).
/// Bit (in_face * 6 + out_face) is 1 if there is a path through passable/transparent voxels.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SectionConnectivity {
    pub mask: u64,
}

impl Default for SectionConnectivity {
    fn default() -> Self {
        Self::FULL
    }
}

impl SectionConnectivity {
    pub const FULL: Self = Self { mask: u64::MAX };
    pub const NONE: Self = Self { mask: 0 };

    #[inline]
    pub fn is_connected(&self, in_face: u8, out_face: u8) -> bool {
        if in_face > 5 || out_face > 5 {
            return true;
        }
        let bit = (in_face as u64) * 6 + (out_face as u64);
        (self.mask & (1 << bit)) != 0
    }

    #[inline]
    pub fn set_connected(&mut self, in_face: u8, out_face: u8) {
        if in_face <= 5 && out_face <= 5 {
            let bit = (in_face as u64) * 6 + (out_face as u64);
            self.mask |= 1 << bit;
        }
    }
}

/// Helper to check if a block type is a solid occluder for section visibility flood fill.
/// Conservative rules:
/// Opaque solid full-cubes act as occluders.
/// Glass, Leaves, Water, Lava, Cutout (flowers, saplings, torches, doors, trapdoors, ladders),
/// and Air are passable.
#[inline]
pub fn is_section_occluder(block: crate::world::BlockType) -> bool {
    use crate::world::BlockType;
    if block == BlockType::Air {
        return false;
    }
    let props = block.properties();
    if !props.is_solid {
        return false;
    }
    if block == BlockType::Glass {
        return false;
    }
    if matches!(
        block,
        BlockType::OakLeaves | BlockType::BirchLeaves | BlockType::SpruceLeaves
    ) {
        return false;
    }
    true
}

/// Compute pairwise face connectivity for a 16x16x16 section inside a Chunk.
pub fn compute_section_connectivity(
    chunk: &crate::world::Chunk,
    sec_y: usize,
) -> SectionConnectivity {
    let base_y = sec_y * crate::world::SECTION_SIZE;
    let mut any_passable = false;
    let mut any_occluder = false;
    let mut is_passable = [false; 4096];

    for ly in 0..crate::world::SECTION_SIZE {
        let y = base_y + ly;
        for z in 0..crate::world::SECTION_SIZE {
            for x in 0..crate::world::SECTION_SIZE {
                let block = chunk.get_block_local(x, y, z);
                let occluder = is_section_occluder(block);
                let index = ly * 256 + z * 16 + x;
                is_passable[index] = !occluder;
                if occluder {
                    any_occluder = true;
                } else {
                    any_passable = true;
                }
            }
        }
    }

    if !any_occluder {
        return SectionConnectivity::FULL;
    }
    if !any_passable {
        return SectionConnectivity::NONE;
    }

    let mut visited = [false; 4096];
    let mut connectivity = SectionConnectivity { mask: 0 };
    let mut queue = Vec::with_capacity(256);

    for start_idx in 0..4096 {
        if !is_passable[start_idx] || visited[start_idx] {
            continue;
        }

        visited[start_idx] = true;
        queue.clear();
        queue.push(start_idx);

        let mut touched_faces = 0u8;
        let mut head = 0;

        while head < queue.len() {
            let idx = queue[head];
            head += 1;

            let ly = idx / 256;
            let rem = idx % 256;
            let z = rem / 16;
            let x = rem % 16;

            if x == 15 { touched_faces |= 1 << 0; } // +X
            if x == 0  { touched_faces |= 1 << 1; } // -X
            if ly == 15 { touched_faces |= 1 << 2; } // +Y
            if ly == 0  { touched_faces |= 1 << 3; } // -Y
            if z == 15 { touched_faces |= 1 << 4; } // +Z
            if z == 0  { touched_faces |= 1 << 5; } // -Z

            let neighbors = [
                if x < 15 { Some(idx + 1) } else { None },
                if x > 0 { Some(idx - 1) } else { None },
                if ly < 15 { Some(idx + 256) } else { None },
                if ly > 0 { Some(idx - 256) } else { None },
                if z < 15 { Some(idx + 16) } else { None },
                if z > 0 { Some(idx - 16) } else { None },
            ];

            for neighbor in neighbors.into_iter().flatten() {
                if is_passable[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push(neighbor);
                }
            }
        }

        for f1 in 0..6u8 {
            if (touched_faces & (1 << f1)) != 0 {
                for f2 in 0..6u8 {
                    if (touched_faces & (1 << f2)) != 0 {
                        connectivity.set_connected(f1, f2);
                    }
                }
            }
        }
    }

    connectivity
}

struct SectionNode {
    x: i32,
    sec_y: usize,
    z: i32,
    entry_face: Option<u8>,
}

/// Perform bounded Graph Traversal starting from camera section.
/// Returns a set of visible section coordinates `(x, sec_y, z)`.
pub fn traverse_section_visibility<F>(
    cam_sec_x: i32,
    cam_sec_y: usize,
    cam_sec_z: i32,
    render_distance: i32,
    frustum: &Frustum,
    get_connectivity: F,
    visible_sections: &mut HashSet<(i32, usize, i32)>,
) where
    F: Fn(i32, usize, i32) -> Option<SectionConnectivity>,
{
    visible_sections.clear();
    let mut visited_entry = HashMap::new();
    let mut queue = VecDeque::new();

    let start = (cam_sec_x, cam_sec_y, cam_sec_z);
    visible_sections.insert(start);
    visited_entry.insert(start, 0x3F);

    queue.push_back(SectionNode {
        x: cam_sec_x,
        sec_y: cam_sec_y,
        z: cam_sec_z,
        entry_face: None,
    });

    while let Some(node) = queue.pop_front() {
        let connectivity = get_connectivity(node.x, node.sec_y, node.z)
            .unwrap_or(SectionConnectivity::FULL);

        for out_face in 0..6u8 {
            if node.entry_face.map_or(true, |in_f| connectivity.is_connected(in_f, out_face)) {
                let (target_x, target_y_raw, target_z, opposite_entry) = match out_face {
                    0 => (node.x + 1, node.sec_y as i32, node.z, 1u8),
                    1 => (node.x - 1, node.sec_y as i32, node.z, 0u8),
                    2 => (node.x, node.sec_y as i32 + 1, node.z, 3u8),
                    3 => (node.x, node.sec_y as i32 - 1, node.z, 2u8),
                    4 => (node.x, node.sec_y as i32, node.z + 1, 5u8),
                    5 => (node.x, node.sec_y as i32, node.z - 1, 4u8),
                    _ => continue,
                };

                if target_y_raw < 0 || target_y_raw >= 16 {
                    continue;
                }
                let target_sec_y = target_y_raw as usize;

                if (target_x - cam_sec_x).abs() > render_distance
                    || (target_z - cam_sec_z).abs() > render_distance
                {
                    continue;
                }

                let min_pos = Vec3::new(
                    target_x as f32 * 16.0,
                    target_sec_y as f32 * 16.0,
                    target_z as f32 * 16.0,
                );
                let bounds = MeshBounds::new(min_pos, min_pos + Vec3::splat(16.0));
                if !frustum.intersects_aabb(&bounds) {
                    continue;
                }

                let target_key = (target_x, target_sec_y, target_z);
                visible_sections.insert(target_key);

                let entry_mask = visited_entry.entry(target_key).or_insert(0u8);
                if (*entry_mask & (1 << opposite_entry)) == 0 {
                    *entry_mask |= 1 << opposite_entry;
                    queue.push_back(SectionNode {
                        x: target_x,
                        sec_y: target_sec_y,
                        z: target_z,
                        entry_face: Some(opposite_entry),
                    });
                }
            }
        }
    }
}

/// Fast 3D DDA voxel line-of-sight raycast.
pub fn is_los_blocked<F>(origin: Vec3, target: Vec3, mut is_occluder: F) -> bool
where
    F: FnMut(i32, i32, i32) -> bool,
{
    let dir = target - origin;
    let dist = dir.length();
    if dist < 0.001 {
        return false;
    }
    let norm = dir / dist;

    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;

    let target_x = target.x.floor() as i32;
    let target_y = target.y.floor() as i32;
    let target_z = target.z.floor() as i32;

    let step_x = if norm.x > 0.0 { 1 } else if norm.x < 0.0 { -1 } else { 0 };
    let step_y = if norm.y > 0.0 { 1 } else if norm.y < 0.0 { -1 } else { 0 };
    let step_z = if norm.z > 0.0 { 1 } else if norm.z < 0.0 { -1 } else { 0 };

    let delta_x = if step_x != 0 { (1.0 / norm.x.abs()).min(100.0) } else { 100.0 };
    let delta_y = if step_y != 0 { (1.0 / norm.y.abs()).min(100.0) } else { 100.0 };
    let delta_z = if step_z != 0 { (1.0 / norm.z.abs()).min(100.0) } else { 100.0 };

    let mut t_max_x = if step_x > 0 {
        (x as f32 + 1.0 - origin.x) * delta_x
    } else if step_x < 0 {
        (origin.x - x as f32) * delta_x
    } else {
        f32::INFINITY
    };
    let mut t_max_y = if step_y > 0 {
        (y as f32 + 1.0 - origin.y) * delta_y
    } else if step_y < 0 {
        (origin.y - y as f32) * delta_y
    } else {
        f32::INFINITY
    };
    let mut t_max_z = if step_z > 0 {
        (z as f32 + 1.0 - origin.z) * delta_z
    } else if step_z < 0 {
        (origin.z - z as f32) * delta_z
    } else {
        f32::INFINITY
    };

    let start_x = x;
    let start_y = y;
    let start_z = z;

    for _ in 0..48 {
        if x == target_x && y == target_y && z == target_z {
            return false;
        }

        if !(x == start_x && y == start_y && z == start_z) {
            if is_occluder(x, y, z) {
                return true;
            }
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                x += step_x;
                t_max_x += delta_x;
            } else {
                z += step_z;
                t_max_z += delta_z;
            }
        } else {
            if t_max_y < t_max_z {
                y += step_y;
                t_max_y += delta_y;
            } else {
                z += step_z;
                t_max_z += delta_z;
            }
        }
    }

    false
}

pub struct EntityLosRequest {
    pub entity_id: u64,
    pub camera_pos: Vec3,
    pub target_pos: Vec3,
    pub camera_cell: (i32, i32, i32),
}

pub struct EntityLosResult {
    pub entity_id: u64,
    pub camera_cell: (i32, i32, i32),
    pub is_visible: bool,
}

#[derive(Clone, Copy)]
struct LosCacheEntry {
    camera_cell: (i32, i32, i32),
    is_visible: bool,
    ttl: u8,
    hysteresis_count: u8,
}

pub struct EntityLosManager {
    request_tx: SyncSender<EntityLosRequest>,
    result_rx: Receiver<EntityLosResult>,
    cache: HashMap<u64, LosCacheEntry>,
}

impl EntityLosManager {
    pub fn new() -> Self {
        let (req_tx, req_rx) = sync_channel::<EntityLosRequest>(64);
        let (res_tx, res_rx) = channel::<EntityLosResult>();

        thread::Builder::new()
            .name("entity_los_worker".to_string())
            .spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    // For async thread LOS check, run bounded raycast
                    let blocked = is_los_blocked(req.camera_pos, req.target_pos, |_x, _y, _z| {
                        // In background worker thread, if no spatial lookup is passed,
                        // we can perform a conservative test or assume raycast test
                        false
                    });

                    let _ = res_tx.send(EntityLosResult {
                        entity_id: req.entity_id,
                        camera_cell: req.camera_cell,
                        is_visible: !blocked,
                    });
                }
            })
            .expect("Failed to spawn entity_los_worker thread");

        Self {
            request_tx: req_tx,
            result_rx: res_rx,
            cache: HashMap::new(),
        }
    }

    pub fn poll_results(&mut self) {
        while let Ok(res) = self.result_rx.try_recv() {
            let entry = self.cache.entry(res.entity_id).or_insert(LosCacheEntry {
                camera_cell: res.camera_cell,
                is_visible: true,
                ttl: 30,
                hysteresis_count: 0,
            });

            entry.camera_cell = res.camera_cell;
            entry.is_visible = res.is_visible;
            entry.ttl = 30;

            if res.is_visible {
                entry.hysteresis_count = 0;
            } else {
                entry.hysteresis_count = entry.hysteresis_count.saturating_add(1);
            }
        }
    }

    pub fn is_entity_visible(
        &mut self,
        entity: &Entity,
        cam_pos: Vec3,
        cam_cell: (i32, i32, i32),
        chunk_manager: &ChunkManager,
    ) -> bool {
        let dist_sq = entity.position.distance_squared(cam_pos);
        if dist_sq <= 16.0 * 16.0 {
            return true;
        }

        if entity.entity_type.is_projectile()
            || matches!(entity.entity_type, EntityType::EnderDragon | EntityType::Wither | EntityType::EndCrystal)
        {
            return true;
        }

        if entity.entity_type == EntityType::RemotePlayer && dist_sq <= 32.0 * 32.0 {
            return true;
        }

        if let Some(entry) = self.cache.get_mut(&entity.id) {
            if entry.camera_cell == cam_cell && entry.ttl > 0 {
                entry.ttl -= 1;
                if !entry.is_visible && entry.hysteresis_count >= 3 {
                    return false;
                }
                return true;
            }
        }

        // Fast synchronous check using DDA against chunk manager
        let target_pos = entity.position + Vec3::new(0.0, 0.8, 0.0);
        let blocked = is_los_blocked(cam_pos, target_pos, |x, y, z| {
            let block = chunk_manager.get_block(x, y, z);
            is_section_occluder(block)
        });

        let entry = self.cache.entry(entity.id).or_insert(LosCacheEntry {
            camera_cell: cam_cell,
            is_visible: true,
            ttl: 30,
            hysteresis_count: 0,
        });

        entry.camera_cell = cam_cell;
        entry.is_visible = !blocked;
        entry.ttl = 30;
        if blocked {
            entry.hysteresis_count = entry.hysteresis_count.saturating_add(1);
        } else {
            entry.hysteresis_count = 0;
        }

        if blocked && entry.hysteresis_count >= 3 {
            return false;
        }

        let _ = self.request_tx.try_send(EntityLosRequest {
            entity_id: entity.id,
            camera_pos: cam_pos,
            target_pos,
            camera_cell: cam_cell,
        });

        true
    }
}

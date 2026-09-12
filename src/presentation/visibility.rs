#[cfg(test)]
use glam::Mat4;
use glam::Vec3;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::chunk_render::{Frustum, MeshBounds};
use crate::dimension::WorldHeight;
use icraft::culling::connectivity::SectionConnectivity;

#[derive(Debug)]
struct SectionNode {
    x: i32,
    sec_y: i8,
    z: i32,
    entry_face: Option<u8>,
}

/// Reusable temporary storage for section visibility traversal.
///
/// The visibility set itself remains caller-owned because the render pass
/// queries it after traversal. This scratch owns only the queue and per-entry
/// visitation masks that used to be allocated afresh for every frame.
#[derive(Debug, Default)]
pub struct SectionVisibilityScratch {
    visited_entry: HashMap<(i32, i8, i32), u8>,
    queue: VecDeque<SectionNode>,
}

impl SectionVisibilityScratch {
    /// Pre-reserve traversal storage when the render-distance budget is known.
    pub fn with_capacity(visited_capacity: usize, queue_capacity: usize) -> Self {
        Self {
            visited_entry: HashMap::with_capacity(visited_capacity),
            queue: VecDeque::with_capacity(queue_capacity),
        }
    }

    #[cfg(test)]
    fn capacities(&self) -> (usize, usize) {
        (self.visited_entry.capacity(), self.queue.capacity())
    }
}

/// Perform bounded section visibility traversal using caller-owned scratch.
///
/// Callers that invoke this once per frame should retain one
/// [`SectionVisibilityScratch`] and pass it back on every call. Its internal
/// `HashMap` and `VecDeque` then retain their peak capacities, so steady-state
/// traversal does not allocate.
pub fn traverse_section_visibility_with_scratch<F>(
    cam_sec_x: i32,
    cam_sec_y: i8,
    cam_sec_z: i32,
    render_distance: i32,
    world_height: WorldHeight,
    frustum: &Frustum,
    get_connectivity: F,
    visible_sections: &mut HashSet<(i32, i8, i32)>,
    scratch: &mut SectionVisibilityScratch,
) where
    F: Fn(i32, i8, i32) -> Option<SectionConnectivity>,
{
    visible_sections.clear();
    scratch.visited_entry.clear();
    scratch.queue.clear();

    let min_sec = world_height.min_section_y() as i32;
    let max_sec = world_height.max_section_y_exclusive() as i32;

    let start = (cam_sec_x, cam_sec_y, cam_sec_z);
    visible_sections.insert(start);
    scratch.visited_entry.insert(start, 0x3F);

    scratch.queue.push_back(SectionNode {
        x: cam_sec_x,
        sec_y: cam_sec_y,
        z: cam_sec_z,
        entry_face: None,
    });

    while let Some(node) = scratch.queue.pop_front() {
        let connectivity =
            get_connectivity(node.x, node.sec_y, node.z).unwrap_or(SectionConnectivity::FULL);

        for out_face in 0..6u8 {
            if node
                .entry_face
                .map_or(true, |in_f| connectivity.is_connected(in_f, out_face))
            {
                let (target_x, target_y_raw, target_z, opposite_entry) = match out_face {
                    0 => (node.x + 1, node.sec_y as i32, node.z, 1u8),
                    1 => (node.x - 1, node.sec_y as i32, node.z, 0u8),
                    2 => (node.x, node.sec_y as i32 + 1, node.z, 3u8),
                    3 => (node.x, node.sec_y as i32 - 1, node.z, 2u8),
                    4 => (node.x, node.sec_y as i32, node.z + 1, 5u8),
                    5 => (node.x, node.sec_y as i32, node.z - 1, 4u8),
                    _ => continue,
                };

                if target_y_raw < min_sec || target_y_raw >= max_sec {
                    continue;
                }
                let target_sec_y = target_y_raw as i8;

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

                let entry_mask = scratch.visited_entry.entry(target_key).or_insert(0u8);
                if (*entry_mask & (1 << opposite_entry)) == 0 {
                    *entry_mask |= 1 << opposite_entry;
                    scratch.queue.push_back(SectionNode {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_visibility_reuses_traversal_scratch_capacity() {
        let frustum = Frustum::from_view_projection(Mat4::orthographic_lh(
            -64.0, 64.0, -64.0, 64.0, 0.0, 128.0,
        ));
        let mut visible_sections = HashSet::with_capacity(512);
        let mut scratch = SectionVisibilityScratch::with_capacity(512, 512);

        traverse_section_visibility_with_scratch(
            0,
            0,
            0,
            1,
            WorldHeight::OVERWORLD,
            &frustum,
            |_, _, _| Some(SectionConnectivity::FULL),
            &mut visible_sections,
            &mut scratch,
        );
        let first_len = visible_sections.len();
        let visible_capacity = visible_sections.capacity();
        let scratch_capacities = scratch.capacities();

        for _ in 0..8 {
            traverse_section_visibility_with_scratch(
                0,
                0,
                0,
                1,
                WorldHeight::OVERWORLD,
                &frustum,
                |_, _, _| Some(SectionConnectivity::FULL),
                &mut visible_sections,
                &mut scratch,
            );
            assert_eq!(visible_sections.len(), first_len);
        }

        assert!(first_len > 1);
        assert_eq!(visible_sections.capacity(), visible_capacity);
        assert_eq!(scratch.capacities(), scratch_capacities);
    }
}

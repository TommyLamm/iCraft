use crate::chunk_manager::ChunkManager;
use glam::Vec3;
use std::collections::{BinaryHeap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
struct PathNode {
    pos: (i32, i32, i32),
    cost: i32,
    heuristic: i32,
}

impl Ord for PathNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order for min-heap
        (other.cost + other.heuristic).cmp(&(self.cost + self.heuristic))
    }
}

impl PartialOrd for PathNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct BoundedPathfinder {
    pub max_nodes: usize,
}

impl Default for BoundedPathfinder {
    fn default() -> Self {
        Self::new(100)
    }
}

impl BoundedPathfinder {
    pub fn new(max_nodes: usize) -> Self {
        Self { max_nodes }
    }

    pub fn find_path(&self, chunk_manager: &ChunkManager, start: Vec3, target: Vec3) -> Vec<Vec3> {
        let sx = start.x.floor() as i32;
        let sy = start.y.floor() as i32;
        let sz = start.z.floor() as i32;

        let tx = target.x.floor() as i32;
        let ty = target.y.floor() as i32;
        let tz = target.z.floor() as i32;

        let mut open_set = BinaryHeap::new();
        let mut visited = HashSet::new();
        let mut nodes_evaluated = 0;

        let start_h = (sx - tx).abs() + (sy - ty).abs() + (sz - tz).abs();
        open_set.push(PathNode {
            pos: (sx, sy, sz),
            cost: 0,
            heuristic: start_h,
        });

        let mut best_node = (sx, sy, sz);
        let mut min_h = start_h;

        while let Some(current) = open_set.pop() {
            if current.pos == (tx, ty, tz) {
                best_node = current.pos;
                break;
            }

            if current.heuristic < min_h {
                min_h = current.heuristic;
                best_node = current.pos;
            }

            nodes_evaluated += 1;
            if nodes_evaluated >= self.max_nodes {
                break;
            }

            visited.insert(current.pos);

            let (cx, cy, cz) = current.pos;

            // 4 cardinal directions + step up/down
            let neighbors = [
                (cx + 1, cy, cz),
                (cx - 1, cy, cz),
                (cx, cy, cz + 1),
                (cx, cy, cz - 1),
                (cx + 1, cy + 1, cz),
                (cx - 1, cy + 1, cz),
                (cx, cy + 1, cz + 1),
                (cx, cy + 1, cz - 1),
                (cx + 1, cy - 1, cz),
                (cx - 1, cy - 1, cz),
                (cx, cy - 1, cz + 1),
                (cx, cy - 1, cz - 1),
            ];

            for &(nx, ny, nz) in &neighbors {
                if visited.contains(&(nx, ny, nz)) {
                    continue;
                }

                // Check chunk loaded
                let chunk_pos = ((nx >> 4), (nz >> 4));
                if !chunk_manager.chunks.contains_key(&chunk_pos) {
                    continue;
                }

                // Check walkable (solid below, air at foot and head)
                let ground = chunk_manager.get_block(nx, ny - 1, nz);
                let foot = chunk_manager.get_block(nx, ny, nz);
                let head = chunk_manager.get_block(nx, ny + 1, nz);

                if ground == crate::world::BlockType::Air || ground == crate::world::BlockType::Lava
                {
                    continue;
                }
                if foot != crate::world::BlockType::Air || head != crate::world::BlockType::Air {
                    continue;
                }

                let h = (nx - tx).abs() + (ny - ty).abs() + (nz - tz).abs();
                open_set.push(PathNode {
                    pos: (nx, ny, nz),
                    cost: current.cost + 1,
                    heuristic: h,
                });
            }
        }

        // Return direct waypoint towards best node found within budget
        vec![Vec3::new(
            best_node.0 as f32 + 0.5,
            best_node.1 as f32,
            best_node.2 as f32 + 0.5,
        )]
    }
}

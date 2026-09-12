use crate::chunk_manager::ChunkManager;
use crate::dimension::WorldHeight;
use crate::world::{BlockType, Chunk, RenderType, CHUNK_DEPTH, CHUNK_WIDTH, SECTION_SIZE};
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

#[derive(Clone, Copy)]
enum LightKind {
    Sky,
    Block,
}

impl LightKind {
    fn get(self, chunk: &Chunk, bx: usize, wy: i32, bz: usize) -> u8 {
        match self {
            Self::Sky => chunk.get_sky_light(bx, wy, bz),
            Self::Block => chunk.get_block_light(bx, wy, bz),
        }
    }

    fn set(self, chunk: &mut Chunk, bx: usize, wy: i32, bz: usize, val: u8) {
        match self {
            Self::Sky => chunk.set_sky_light(bx, wy, bz, val),
            Self::Block => chunk.set_block_light(bx, wy, bz, val),
        }
    }
}

fn mark_light_dirty(dirty: &mut HashSet<(i32, i32)>, nx: i32, nz: i32) {
    let cx = nx.div_euclid(CHUNK_WIDTH as i32);
    let cz = nz.div_euclid(CHUNK_DEPTH as i32);
    dirty.insert((cx, cz));
    let lx = nx.rem_euclid(CHUNK_WIDTH as i32);
    let lz = nz.rem_euclid(CHUNK_DEPTH as i32);
    if lx == 0 {
        dirty.insert((cx - 1, cz));
    }
    if lx == 15 {
        dirty.insert((cx + 1, cz));
    }
    if lz == 0 {
        dirty.insert((cx, cz - 1));
    }
    if lz == 15 {
        dirty.insert((cx, cz + 1));
    }
}

/// Owned 3×3 column scratch so BFS never does a per-voxel `HashMap` get.
struct LightNeighborhood {
    origin_cx: i32,
    origin_cz: i32,
    height: WorldHeight,
    has_sky_light: bool,
    columns: [[Option<Chunk>; 3]; 3],
    /// World cells whose light value changed; applied to ChunkManager after restore.
    changed_cells: Vec<(i32, i32, i32)>,
}

impl LightNeighborhood {
    fn take(chunk_manager: &mut ChunkManager, origin_cx: i32, origin_cz: i32) -> Self {
        let mut columns: [[Option<Chunk>; 3]; 3] = std::array::from_fn(|_| std::array::from_fn(|_| None));
        for dz in 0..3i32 {
            for dx in 0..3i32 {
                let key = (origin_cx + dx - 1, origin_cz + dz - 1);
                columns[dz as usize][dx as usize] = chunk_manager.chunks.remove(&key);
            }
        }
        Self {
            origin_cx,
            origin_cz,
            height: chunk_manager.dimension.height(),
            has_sky_light: chunk_manager.dimension.has_sky_light(),
            columns,
            changed_cells: Vec::new(),
        }
    }

    fn restore(self, chunk_manager: &mut ChunkManager) {
        let Self {
            origin_cx,
            origin_cz,
            mut columns,
            changed_cells,
            ..
        } = self;
        for dz in 0..3i32 {
            for dx in 0..3i32 {
                if let Some(chunk) = columns[dz as usize][dx as usize].take() {
                    chunk_manager
                        .chunks
                        .insert((origin_cx + dx - 1, origin_cz + dz - 1), chunk);
                }
            }
        }
        for (wx, wy, wz) in changed_cells {
            chunk_manager.note_light_cell_change(wx, wy, wz);
        }
    }

    fn column_index(&self, wx: i32, wz: i32) -> Option<(usize, usize, usize, usize)> {
        let cx = wx.div_euclid(CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(CHUNK_DEPTH as i32);
        let dx = cx - self.origin_cx;
        let dz = cz - self.origin_cz;
        if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
            return None;
        }
        let bx = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
        let bz = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
        Some(((dz + 1) as usize, (dx + 1) as usize, bx, bz))
    }

    fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType {
        if !self.height.contains_y(wy) {
            return BlockType::Air;
        }
        let Some((iz, ix, bx, bz)) = self.column_index(wx, wz) else {
            return BlockType::Air;
        };
        match &self.columns[iz][ix] {
            Some(chunk) => chunk.get_block_local(bx, wy, bz),
            None => BlockType::Air,
        }
    }

    fn get_light(&self, kind: LightKind, wx: i32, wy: i32, wz: i32) -> u8 {
        if !self.height.contains_y(wy) {
            if matches!(kind, LightKind::Sky)
                && wy >= self.height.max_y_exclusive()
                && self.has_sky_light
            {
                return 15;
            }
            return 0;
        }
        let Some((iz, ix, bx, bz)) = self.column_index(wx, wz) else {
            return 0;
        };
        match &self.columns[iz][ix] {
            Some(chunk) => kind.get(chunk, bx, wy, bz),
            None => 0,
        }
    }

    fn set_light(
        &mut self,
        kind: LightKind,
        wx: i32,
        wy: i32,
        wz: i32,
        val: u8,
        dirty_chunks: &mut HashSet<(i32, i32)>,
    ) {
        if !self.height.contains_y(wy) {
            return;
        }
        let Some((iz, ix, bx, bz)) = self.column_index(wx, wz) else {
            return;
        };
        let Some(chunk) = self.columns[iz][ix].as_mut() else {
            return;
        };
        if kind.get(chunk, bx, wy, bz) == val {
            return;
        }
        kind.set(chunk, bx, wy, bz, val);
        self.changed_cells.push((wx, wy, wz));
        mark_light_dirty(dirty_chunks, wx, wz);
    }
}

fn propagate(
    nb: &mut LightNeighborhood,
    kind: LightKind,
    queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    while let Some(node) = queue.pop_front() {
        let current_light = nb.get_light(kind, node.x, node.y, node.z);
        if current_light <= 1 {
            continue;
        }

        for &(dx, dy, dz) in &LIGHT_DIRS {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if !nb.height.contains_y(ny) {
                continue;
            }

            if nb.get_block(nx, ny, nz).def().properties.render_type == RenderType::Opaque {
                continue;
            }

            let neighbor_light = nb.get_light(kind, nx, ny, nz);
            let expected_light = current_light - 1;

            if neighbor_light < expected_light {
                nb.set_light(kind, nx, ny, nz, expected_light, dirty_chunks);
                queue.push_back(LightNode {
                    x: nx,
                    y: ny,
                    z: nz,
                });
            }
        }
    }
}

fn remove_light(
    nb: &mut LightNeighborhood,
    kind: LightKind,
    removal_queue: &mut VecDeque<LightRemovalNode>,
    propagate_queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    while let Some(node) = removal_queue.pop_front() {
        for &(dx, dy, dz) in &LIGHT_DIRS {
            let nx = node.x + dx;
            let ny = node.y + dy;
            let nz = node.z + dz;

            if !nb.height.contains_y(ny) {
                continue;
            }

            let neighbor_light = nb.get_light(kind, nx, ny, nz);
            if neighbor_light != 0 && neighbor_light < node.val {
                nb.set_light(kind, nx, ny, nz, 0, dirty_chunks);
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

fn run_propagate(
    chunk_manager: &mut ChunkManager,
    origin_cx: i32,
    origin_cz: i32,
    kind: LightKind,
    queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    if queue.is_empty() {
        return;
    }
    let mut nb = LightNeighborhood::take(chunk_manager, origin_cx, origin_cz);
    propagate(&mut nb, kind, queue, dirty_chunks);
    nb.restore(chunk_manager);
}

fn run_remove(
    chunk_manager: &mut ChunkManager,
    origin_cx: i32,
    origin_cz: i32,
    kind: LightKind,
    removal_queue: &mut VecDeque<LightRemovalNode>,
    propagate_queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    if removal_queue.is_empty() {
        return;
    }
    let mut nb = LightNeighborhood::take(chunk_manager, origin_cx, origin_cz);
    remove_light(
        &mut nb,
        kind,
        removal_queue,
        propagate_queue,
        dirty_chunks,
    );
    nb.restore(chunk_manager);
}

fn origin_from_node(x: i32, z: i32) -> (i32, i32) {
    (
        x.div_euclid(CHUNK_WIDTH as i32),
        z.div_euclid(CHUNK_DEPTH as i32),
    )
}

pub fn propagate_sky_light(
    chunk_manager: &mut ChunkManager,
    queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let Some(first) = queue.front() else {
        return;
    };
    let (ox, oz) = origin_from_node(first.x, first.z);
    run_propagate(
        chunk_manager,
        ox,
        oz,
        LightKind::Sky,
        queue,
        dirty_chunks,
    );
}

pub fn remove_sky_light(
    chunk_manager: &mut ChunkManager,
    removal_queue: &mut VecDeque<LightRemovalNode>,
    propagate_queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let Some(first) = removal_queue.front() else {
        return;
    };
    let (ox, oz) = origin_from_node(first.x, first.z);
    run_remove(
        chunk_manager,
        ox,
        oz,
        LightKind::Sky,
        removal_queue,
        propagate_queue,
        dirty_chunks,
    );
}

pub fn propagate_block_light(
    chunk_manager: &mut ChunkManager,
    queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let Some(first) = queue.front() else {
        return;
    };
    let (ox, oz) = origin_from_node(first.x, first.z);
    run_propagate(
        chunk_manager,
        ox,
        oz,
        LightKind::Block,
        queue,
        dirty_chunks,
    );
}

pub fn remove_block_light(
    chunk_manager: &mut ChunkManager,
    removal_queue: &mut VecDeque<LightRemovalNode>,
    propagate_queue: &mut VecDeque<LightNode>,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let Some(first) = removal_queue.front() else {
        return;
    };
    let (ox, oz) = origin_from_node(first.x, first.z);
    run_remove(
        chunk_manager,
        ox,
        oz,
        LightKind::Block,
        removal_queue,
        propagate_queue,
        dirty_chunks,
    );
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
    if block.def().properties.render_type != RenderType::Opaque {
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
                    y,
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
            if block.def().properties.render_type == RenderType::Opaque {
                break;
            }
            chunk_manager.set_sky_light(wx, y, wz, 15);

            let cx = wx.div_euclid(CHUNK_WIDTH as i32);
            let cz = wz.div_euclid(CHUNK_DEPTH as i32);
            dirty_chunks.insert((cx, cz));

            propagate_queue.push_back(LightNode { x: wx, y, z: wz });
        }
    } else {
        chunk_manager.set_sky_light(wx, wy, wz, 0);
        let mut max_neighbor = 0;
        for &(dx, dy, dz) in &LIGHT_DIRS {
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
        if block.def().properties.render_type == RenderType::Opaque {
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
        for &(dx, dy, dz) in &LIGHT_DIRS {
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
    height: WorldHeight,
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
        if chunk.get_block_local(bx, ny, bz).def().properties.render_type == RenderType::Opaque {
            continue;
        }
        if get_light(chunk, bx, ny, bz) < light - 1 {
            return true;
        }
    }
    false
}

fn seed_cell(
    neighborhood: &[[Option<&Chunk>; 3]; 3],
    origin_cx: i32,
    origin_cz: i32,
    wx: i32,
    wy: i32,
    wz: i32,
    chunk: &Chunk,
    bx: usize,
    bz: usize,
    height: WorldHeight,
    sky_queue: &mut VecDeque<LightNode>,
    block_queue: &mut VecDeque<LightNode>,
) {
    let sky_val = chunk.get_sky_light(bx, wy, bz);
    if neighbor_needs_propagation(
        neighborhood,
        origin_cx,
        origin_cz,
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
    let block_val = chunk.get_block_light(bx, wy, bz);
    if neighbor_needs_propagation(
        neighborhood,
        origin_cx,
        origin_cz,
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

/// Seed faces, emitters, and cells that already border a darker neighbor.
/// Skips dark volume cells (`light <= 1`) instead of probing every voxel's neighbors.
fn seed_column_from_emitters_and_lit(
    neighborhood: &[[Option<&Chunk>; 3]; 3],
    cx: i32,
    cz: i32,
    height: WorldHeight,
    sky_queue: &mut VecDeque<LightNode>,
    block_queue: &mut VecDeque<LightNode>,
) {
    let Some(chunk) = neighborhood[1][1] else {
        return;
    };
    let start_x = cx * CHUNK_WIDTH as i32;
    let start_z = cz * CHUNK_DEPTH as i32;

    for (sec_idx, section) in chunk.sections.iter().enumerate() {
        if section.is_none() {
            continue;
        }
        let base_y = chunk.section_y_at_index(sec_idx) as i32 * SECTION_SIZE as i32;
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                let wx = start_x + x as i32;
                let wz = start_z + z as i32;
                let on_face = x == 0 || x == CHUNK_WIDTH - 1 || z == 0 || z == CHUNK_DEPTH - 1;
                for ly in 0..SECTION_SIZE {
                    let wy = base_y + ly as i32;
                    if !height.contains_y(wy) {
                        continue;
                    }
                    let sky_val = chunk.get_sky_light(x, wy, z);
                    let block_val = chunk.get_block_light(x, wy, z);
                    // Face cells always considered; interior only when lit / emitting.
                    if !on_face && sky_val <= 1 && block_val <= 1 {
                        continue;
                    }
                    seed_cell(
                        neighborhood,
                        cx,
                        cz,
                        wx,
                        wy,
                        wz,
                        chunk,
                        x,
                        z,
                        height,
                        sky_queue,
                        block_queue,
                    );
                }
            }
        }
    }
}

/// Seed only the shared vertical face between `cx,cz` and a cardinal neighbor.
fn seed_shared_face(
    neighborhood: &[[Option<&Chunk>; 3]; 3],
    origin_cx: i32,
    origin_cz: i32,
    cx: i32,
    cz: i32,
    neighbor_cx: i32,
    neighbor_cz: i32,
    height: WorldHeight,
    sky_queue: &mut VecDeque<LightNode>,
    block_queue: &mut VecDeque<LightNode>,
) {
    let dx = neighbor_cx - cx;
    let dz = neighbor_cz - cz;
    if dx.abs() + dz.abs() != 1 {
        return;
    }
    let iz = (cz - origin_cz + 1) as usize;
    let ix = (cx - origin_cx + 1) as usize;
    if iz > 2 || ix > 2 {
        return;
    }
    let Some(chunk) = neighborhood[iz][ix] else {
        return;
    };

    let start_x = cx * CHUNK_WIDTH as i32;
    let start_z = cz * CHUNK_DEPTH as i32;

    for (sec_idx, section) in chunk.sections.iter().enumerate() {
        if section.is_none() {
            continue;
        }
        let base_y = chunk.section_y_at_index(sec_idx) as i32 * SECTION_SIZE as i32;
        let x_range = if dx != 0 {
            let x = if dx == 1 { CHUNK_WIDTH - 1 } else { 0 };
            x..=x
        } else {
            0..=(CHUNK_WIDTH - 1)
        };
        let z_range = if dz != 0 {
            let z = if dz == 1 { CHUNK_DEPTH - 1 } else { 0 };
            z..=z
        } else {
            0..=(CHUNK_DEPTH - 1)
        };
        for x in x_range {
            for z in z_range.clone() {
                let wx = start_x + x as i32;
                let wz = start_z + z as i32;
                for ly in 0..SECTION_SIZE {
                    let wy = base_y + ly as i32;
                    if !height.contains_y(wy) {
                        continue;
                    }
                    seed_cell(
                        neighborhood,
                        origin_cx,
                        origin_cz,
                        wx,
                        wy,
                        wz,
                        chunk,
                        x,
                        z,
                        height,
                        sky_queue,
                        block_queue,
                    );
                }
            }
        }
    }
}

/// Propagate lighting for a newly integrated column.
///
/// Seeds the center from faces / emitters / lit cells that border darkness, then
/// seeds only the shared faces of the four cardinal neighbors (no neighbor volume
/// scan). One BFS runs on a taken 3×3 neighborhood.
pub fn propagate_chunk_lighting(
    chunk_manager: &mut ChunkManager,
    cx: i32,
    cz: i32,
    dirty_chunks: &mut HashSet<(i32, i32)>,
) {
    let mut sky_queue = VecDeque::new();
    let mut block_queue = VecDeque::new();
    let height = chunk_manager.dimension.height();
    {
        let neighborhood = chunk_manager.column_neighborhood(cx, cz);
        if neighborhood[1][1].is_none() {
            return;
        }

        seed_column_from_emitters_and_lit(
            &neighborhood,
            cx,
            cz,
            height,
            &mut sky_queue,
            &mut block_queue,
        );

        for (nx, nz) in [(cx - 1, cz), (cx + 1, cz), (cx, cz - 1), (cx, cz + 1)] {
            if neighborhood[(nz - cz + 1) as usize][(nx - cx + 1) as usize].is_none() {
                continue;
            }
            // Seed the neighbor's shared face (light flowing into the new column)
            // and the center's shared face (already covered by face cells in the
            // center seed, but neighbor face is the missing half of the old
            // five-column volume scan).
            seed_shared_face(
                &neighborhood,
                cx,
                cz,
                nx,
                nz,
                cx,
                cz,
                height,
                &mut sky_queue,
                &mut block_queue,
            );
        }
    }

    let mut nb = LightNeighborhood::take(chunk_manager, cx, cz);
    propagate(&mut nb, LightKind::Sky, &mut sky_queue, dirty_chunks);
    propagate(&mut nb, LightKind::Block, &mut block_queue, dirty_chunks);
    nb.restore(chunk_manager);
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
                for y in chunk.world_y_range() {
                    if y >= 64 {
                        chunk.set_block_local(x, y, z, BlockType::Air);
                        chunk.set_sky_light(x, y, z, 15);
                    } else {
                        chunk.set_block_local(x, y, z, BlockType::Stone);
                        chunk.set_sky_light(x, y, z, 0);
                    }
                    chunk.set_block_light(x, y, z, 0);
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
                for y in chunk.world_y_range() {
                    chunk.set_block_local(x, y, z, BlockType::Stone);
                    chunk.set_sky_light(x, y, z, 0);
                    chunk.set_block_light(x, y, z, 0);
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

    fn lighting_checksum(chunk_manager: &ChunkManager, cx: i32, cz: i32) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let chunk = chunk_manager.chunks.get(&(cx, cz)).expect("column present");
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for wy in chunk.world_y_range() {
                    let sky = chunk.get_sky_light(x, wy, z) as u64;
                    let block = chunk.get_block_light(x, wy, z) as u64;
                    hash ^= sky.wrapping_add(0x9e37_79b9_7f4a_7c15);
                    hash = hash.rotate_left(13).wrapping_mul(0x100_0000_01b3);
                    hash ^= block.wrapping_add(0x1656_67b1_9e37_79b9);
                    hash = hash.rotate_left(17).wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        hash
    }

    #[test]
    fn fixed_seed_place_break_lighting_checksum_stable() {
        let mut chunk_manager = ChunkManager::new(0);
        let mut chunk = Chunk::new_with_seed(0, 0, 0x4c17_17c0u32);
        chunk.recompute_direct_column_lighting();
        chunk_manager.chunks.insert((0, 0), chunk);
        let mut dirty = HashSet::new();
        propagate_chunk_lighting(&mut chunk_manager, 0, 0, &mut dirty);

        // Fixed place / break sequence.
        let ops = [
            (8, 70, 8, BlockType::Stone, true),
            (8, 71, 8, BlockType::Glowstone, true),
            (9, 70, 8, BlockType::Stone, true),
            (8, 71, 8, BlockType::Air, false),
            (8, 70, 8, BlockType::Air, false),
        ];
        for &(x, y, z, block, place) in &ops {
            let old = chunk_manager.get_block(x, y, z);
            let old_emission = old.def().properties.light_emission;
            let new_emission = block.def().properties.light_emission;
            chunk_manager.set_block(x, y, z, block);
            if place {
                if block.def().properties.render_type == RenderType::Opaque {
                    update_sky_light_after_placed(&mut chunk_manager, x, y, z, &mut dirty);
                } else {
                    update_sky_light_after_removed(&mut chunk_manager, x, y, z, &mut dirty);
                }
                if old_emission != new_emission {
                    update_block_light_after_removed(
                        &mut chunk_manager,
                        x,
                        y,
                        z,
                        old_emission,
                        &mut dirty,
                    );
                    if new_emission > 0 {
                        update_block_light_after_placed(
                            &mut chunk_manager,
                            x,
                            y,
                            z,
                            new_emission,
                            &mut dirty,
                        );
                    }
                }
            } else {
                update_sky_light_after_removed(&mut chunk_manager, x, y, z, &mut dirty);
                if old_emission > 0 {
                    update_block_light_after_removed(
                        &mut chunk_manager,
                        x,
                        y,
                        z,
                        old_emission,
                        &mut dirty,
                    );
                } else {
                    update_block_light_after_removed(
                        &mut chunk_manager,
                        x,
                        y,
                        z,
                        0,
                        &mut dirty,
                    );
                }
            }
        }

        let checksum = lighting_checksum(&chunk_manager, 0, 0);
        // Locked against the post-refactor engine; bump only with an intentional
        // lighting semantics change.
        assert_eq!(checksum, 0xf9bf_c8d6_aab7_4170);
    }

    #[test]
    fn load_lighting_skips_neighbor_volume_scan_but_fills_shared_face() {
        let mut chunk_manager = ChunkManager::new(0);
        let mut west = Chunk::empty(0, 0);
        let mut east = Chunk::empty(1, 0);
        for z in 0..CHUNK_DEPTH {
            for y in 60..70 {
                west.set_block_local(15, y, z, BlockType::Air);
                west.set_sky_light(15, y, z, 15);
                // Darken the east band so face fill is measurable (empty
                // sections default to full sky).
                for x in 0..CHUNK_WIDTH {
                    east.set_block_local(x, y, z, BlockType::Air);
                    east.set_sky_light(x, y, z, 0);
                }
            }
        }
        chunk_manager.chunks.insert((0, 0), west);
        chunk_manager.chunks.insert((1, 0), east);
        let mut dirty = HashSet::new();
        propagate_chunk_lighting(&mut chunk_manager, 0, 0, &mut dirty);
        assert_eq!(chunk_manager.get_sky_light(16, 64, 8), 14);
        // Light should continue a few steps into the east column from the face.
        assert_eq!(chunk_manager.get_sky_light(17, 64, 8), 13);
    }

    #[test]
    fn load_lighting_timing_smoke() {
        use std::time::Instant;
        let mut chunk_manager = ChunkManager::new(0);
        // Center + four neighbors so face seeding is exercised.
        for (cx, cz) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
            let mut chunk = Chunk::new_with_seed(cx, cz, 42);
            chunk.recompute_direct_column_lighting();
            chunk_manager.chunks.insert((cx, cz), chunk);
        }
        let mut dirty = HashSet::new();
        let started = Instant::now();
        propagate_chunk_lighting(&mut chunk_manager, 0, 0, &mut dirty);
        let elapsed = started.elapsed();
        // Soft budget: post-refactor single-call path should stay well under the
        // old 1–8 ms/column hitch band even in debug. Release is faster.
        eprintln!(
            "load_lighting_timing_smoke: {:?} (dirty columns={})",
            elapsed,
            dirty.len()
        );
        assert!(
            elapsed.as_millis() < 500,
            "unexpectedly slow load lighting: {elapsed:?}"
        );
    }
}

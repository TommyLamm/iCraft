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

/// A dirty mesh must never expose connectivity computed for an older
/// revision. Invalid entries deliberately resolve to FULL so visibility
/// traversal fails open until the matching worker result is integrated.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SectionConnectivityState {
    Invalid,
    Valid(SectionConnectivity),
}

impl SectionConnectivityState {
    #[inline]
    pub fn fail_open(self) -> SectionConnectivity {
        match self {
            Self::Invalid => SectionConnectivity::FULL,
            Self::Valid(connectivity) => connectivity,
        }
    }
}

impl Default for SectionConnectivityState {
    fn default() -> Self {
        Self::Invalid
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
    // Keep this allow-list deliberately conservative: only blocks whose model is
    // a complete opaque cube may seal a section. New block types fail open.
    matches!(
        block,
        BlockType::Grass
            | BlockType::Dirt
            | BlockType::Stone
            | BlockType::Sand
            | BlockType::Gravel
            | BlockType::OakLog
            | BlockType::OakPlanks
            | BlockType::Cobblestone
            | BlockType::Bedrock
            | BlockType::CoalOre
            | BlockType::IronOre
            | BlockType::GoldOre
            | BlockType::DiamondOre
            | BlockType::RedstoneOre
            | BlockType::Brick
            | BlockType::StoneBrick
            | BlockType::Snow
            | BlockType::Clay
            | BlockType::Sandstone
            | BlockType::Obsidian
            | BlockType::BirchLog
            | BlockType::BirchPlanks
            | BlockType::SpruceLog
            | BlockType::SprucePlanks
            | BlockType::Pumpkin
            | BlockType::Melon
            | BlockType::Dispenser
            | BlockType::Dropper
            | BlockType::NoteBlock
            | BlockType::Netherrack
            | BlockType::SoulSand
            | BlockType::Glowstone
            | BlockType::EndStone
            | BlockType::Purpur
            | BlockType::NetherBrick
    )
}

/// Compute pairwise face connectivity for a 16x16x16 section inside a Chunk.
pub fn compute_section_connectivity(chunk: &crate::world::Chunk, sec_y: i8) -> SectionConnectivity {
    compute_section_connectivity_with(|x, ly, z| {
        let wy = crate::world::section_and_local_y_to_world_y(sec_y, ly as u8);
        chunk.get_block_local(x, wy, z)
    })
}

/// Computes connectivity from the exact immutable halo used by a section
/// mesh worker. Core section voxels occupy halo coordinates 1..=16.
pub fn compute_section_connectivity_snapshot(
    snapshot: &crate::world::SectionHaloSnapshot,
) -> SectionConnectivity {
    compute_section_connectivity_with(|x, y, z| snapshot.get_block(x + 1, y + 1, z + 1))
}

fn compute_section_connectivity_with(
    mut block_at: impl FnMut(usize, usize, usize) -> crate::world::BlockType,
) -> SectionConnectivity {
    let mut any_passable = false;
    let mut any_occluder = false;
    let mut is_passable = [false; 4096];

    for ly in 0..crate::world::SECTION_SIZE {
        for z in 0..crate::world::SECTION_SIZE {
            for x in 0..crate::world::SECTION_SIZE {
                let block = block_at(x, ly, z);
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

            if x == 15 {
                touched_faces |= 1 << 0;
            } // +X
            if x == 0 {
                touched_faces |= 1 << 1;
            } // -X
            if ly == 15 {
                touched_faces |= 1 << 2;
            } // +Y
            if ly == 0 {
                touched_faces |= 1 << 3;
            } // -Y
            if z == 15 {
                touched_faces |= 1 << 4;
            } // +Z
            if z == 0 {
                touched_faces |= 1 << 5;
            } // -Z

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_connectivity_fails_open() {
        let connectivity = SectionConnectivityState::Invalid.fail_open();
        for in_face in 0..6 {
            for out_face in 0..6 {
                assert!(connectivity.is_connected(in_face, out_face));
            }
        }
    }

    #[test]
    fn valid_connectivity_preserves_the_computed_mask() {
        let state = SectionConnectivityState::Valid(SectionConnectivity::NONE);
        assert_eq!(state.fail_open(), SectionConnectivity::NONE);
    }

    #[test]
    fn transparent_and_partial_blocks_fail_open() {
        use crate::world::BlockType;
        for block in [
            BlockType::Air,
            BlockType::Glass,
            BlockType::Ice,
            BlockType::Water,
            BlockType::Lava,
            BlockType::OakLeaves,
            BlockType::BirchLeaves,
            BlockType::SpruceLeaves,
            BlockType::Torch,
            BlockType::OakDoor,
            BlockType::OakTrapdoor,
            BlockType::Chest,
            BlockType::Cactus,
            BlockType::TallGrass,
            BlockType::EndPortal,
            BlockType::NetherPortal,
            BlockType::Fire,
            BlockType::DragonEgg,
            BlockType::BrewingStand,
        ] {
            assert!(!is_section_occluder(block), "{block:?} must fail open");
        }
    }
}

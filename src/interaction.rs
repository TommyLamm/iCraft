use crate::chunk_manager::ChunkManager;
use crate::world::BlockType;
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaycastTargetPolicy {
    /// Select the solid face that a newly placed block should attach to.
    ///
    /// Passable vegetation and environmental blocks do not provide that face,
    /// so the ray continues through them.
    Place,
    /// Select blocks that the normal mining path is allowed to break.
    ///
    /// Passable decorations opt in explicitly. This keeps fluids, fire, and
    /// portals out of mining without coupling breakability to collision.
    Break,
}

impl RaycastTargetPolicy {
    fn targets(self, block: BlockType) -> bool {
        match self {
            Self::Place => {
                is_explicit_interaction_target(block)
                    || (block != BlockType::Air && !block.properties().is_passable)
            }
            Self::Break => {
                is_explicit_breakable_decoration(block)
                    || (block != BlockType::Air && !block.properties().is_passable)
            }
        }
    }
}

fn is_explicit_interaction_target(block: BlockType) -> bool {
    // Open doors and trapdoors are passable for collision, but right-click
    // raycasts must still be able to select them so they can be closed again.
    matches!(block, BlockType::OakDoorOpen | BlockType::OakTrapdoorOpen)
}

fn is_explicit_breakable_decoration(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::Torch
            | BlockType::TallGrass
            | BlockType::Dandelion
            | BlockType::Poppy
            | BlockType::SugarCane
            | BlockType::RedstoneWire
            | BlockType::RedstoneTorch
            | BlockType::RedstoneTorchOff
            | BlockType::Repeater
            | BlockType::RepeaterPowered
            | BlockType::Comparator
            | BlockType::ComparatorPowered
            | BlockType::StoneButton
            | BlockType::StoneButtonPressed
            | BlockType::Lever
            | BlockType::LeverOn
            | BlockType::PressurePlate
            | BlockType::PressurePlatePowered
            | BlockType::SnowLayer
            | BlockType::WitherSkeletonSkull
            | BlockType::EndPortal
    )
}

pub struct RaycastResult {
    pub block_pos: Vec3, // 命中的方塊整數座標
    pub normal: Vec3,    // 命中的表面法線（用於放置新方塊）
}

pub fn raycast(
    origin: Vec3,
    direction: Vec3,
    max_dist: f32,
    chunk_manager: &ChunkManager,
    target_policy: RaycastTargetPolicy,
) -> Option<RaycastResult> {
    // Avoid division by zero/NaN by ensuring direction components are non-zero
    let eps = 1e-8;
    let dx = if direction.x.abs() < eps {
        direction.x.signum() * eps
    } else {
        direction.x
    };
    let dy = if direction.y.abs() < eps {
        direction.y.signum() * eps
    } else {
        direction.y
    };
    let dz = if direction.z.abs() < eps {
        direction.z.signum() * eps
    } else {
        direction.z
    };

    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;

    let step_x = if dx > 0.0 { 1 } else { -1 };
    let step_y = if dy > 0.0 { 1 } else { -1 };
    let step_z = if dz > 0.0 { 1 } else { -1 };

    let t_delta_x = (1.0 / dx).abs();
    let t_delta_y = (1.0 / dy).abs();
    let t_delta_z = (1.0 / dz).abs();

    let mut t_max_x = if dx > 0.0 {
        (x as f32 + 1.0 - origin.x) * t_delta_x
    } else {
        (origin.x - x as f32) * t_delta_x
    };
    let mut t_max_y = if dy > 0.0 {
        (y as f32 + 1.0 - origin.y) * t_delta_y
    } else {
        (origin.y - y as f32) * t_delta_y
    };
    let mut t_max_z = if dz > 0.0 {
        (z as f32 + 1.0 - origin.z) * t_delta_z
    } else {
        (origin.z - z as f32) * t_delta_z
    };

    let mut t = 0.0;
    let mut last_face = Vec3::ZERO;
    let ray_dir = Vec3::new(dx, dy, dz).normalize_or_zero();

    while t < max_dist {
        let block = chunk_manager.get_block(x, y, z);
        if target_policy.targets(block) {
            let state = chunk_manager.get_block_state(x, y, z);
            let sel_shape = crate::voxel_shape::block_selection_shape(
                block,
                state,
                (x, y, z),
                Some(chunk_manager),
            );
            if let Some((hit_t, hit_norm)) = sel_shape.ray_intersects(origin, ray_dir, max_dist) {
                if hit_t <= max_dist {
                    let norm = if hit_norm != Vec3::ZERO {
                        hit_norm
                    } else {
                        last_face
                    };
                    return Some(RaycastResult {
                        block_pos: Vec3::new(x as f32, y as f32, z as f32),
                        normal: norm,
                    });
                }
            }
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                t = t_max_x;
                x += step_x;
                t_max_x += t_delta_x;
                last_face = Vec3::new(-step_x as f32, 0.0, 0.0);
            } else {
                t = t_max_z;
                z += step_z;
                t_max_z += t_delta_z;
                last_face = Vec3::new(0.0, 0.0, -step_z as f32);
            }
        } else {
            if t_max_y < t_max_z {
                t = t_max_y;
                y += step_y;
                t_max_y += t_delta_y;
                last_face = Vec3::new(0.0, -step_y as f32, 0.0);
            } else {
                t = t_max_z;
                z += step_z;
                t_max_z += t_delta_z;
                last_face = Vec3::new(0.0, 0.0, -step_z as f32);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_manager::ChunkManager;
    use crate::world::{BlockType, Chunk};
    use glam::Vec3;

    #[test]
    fn test_raycast_air() {
        let mut chunk_manager = ChunkManager::new(8);
        chunk_manager.chunks.insert((0, 0), Chunk::new(0, 0));
        // Look up into the sky from the surface
        let hit = raycast(
            Vec3::new(8.0, 70.0, 8.0),
            Vec3::new(0.0, 1.0, 0.0),
            10.0,
            &chunk_manager,
            RaycastTargetPolicy::Place,
        );
        assert!(hit.is_none());
    }

    #[test]
    fn test_raycast_hit() {
        let mut chunk_manager = ChunkManager::new(8);
        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(8, 72, 8, BlockType::Stone);
        chunk_manager.chunks.insert((0, 0), chunk);

        let hit = raycast(
            Vec3::new(8.5, 70.5, 8.5),
            Vec3::new(0.0, 1.0, 0.0),
            5.0,
            &chunk_manager,
            RaycastTargetPolicy::Place,
        );
        assert!(hit.is_some());
        let res = hit.unwrap();
        assert_eq!(res.block_pos, Vec3::new(8.0, 72.0, 8.0));
        assert_eq!(res.normal, Vec3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn test_raycast_hits_passable_plants_when_breaking() {
        let mut chunk_manager = ChunkManager::new(8);
        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(8, 72, 8, BlockType::TallGrass);
        chunk_manager.chunks.insert((0, 0), chunk);

        let hit = raycast(
            Vec3::new(8.5, 70.5, 8.5),
            Vec3::new(0.0, 1.0, 0.0),
            5.0,
            &chunk_manager,
            RaycastTargetPolicy::Break,
        );
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().block_pos, Vec3::new(8.0, 72.0, 8.0));
    }

    #[test]
    fn place_raycast_can_target_open_door_and_trapdoor_for_interaction() {
        for block in [BlockType::OakDoorOpen, BlockType::OakTrapdoorOpen] {
            let mut chunk_manager = ChunkManager::new(8);
            let mut chunk = Chunk::new(0, 0);
            chunk.set_block_local(8, 72, 8, block);
            chunk_manager.chunks.insert((0, 0), chunk);

            let hit = raycast(
                Vec3::new(8.5, 70.5, 8.5),
                Vec3::new(0.0, 1.0, 0.0),
                5.0,
                &chunk_manager,
                RaycastTargetPolicy::Place,
            );

            assert_eq!(
                hit.map(|result| result.block_pos),
                Some(Vec3::new(8.0, 72.0, 8.0))
            );
        }
    }

    #[test]
    fn break_policy_targets_decorations_but_not_environmental_passables() {
        for block in [
            BlockType::TallGrass,
            BlockType::Dandelion,
            BlockType::Poppy,
            BlockType::SugarCane,
            BlockType::Torch,
            BlockType::RedstoneWire,
            BlockType::RedstoneTorch,
            BlockType::PressurePlate,
            BlockType::SnowLayer,
            BlockType::WitherSkeletonSkull,
        ] {
            assert!(
                RaycastTargetPolicy::Break.targets(block),
                "{block:?} should opt into break targeting"
            );
        }

        for block in [
            BlockType::Air,
            BlockType::Water,
            BlockType::Lava,
            BlockType::Fire,
            BlockType::NetherPortal,
        ] {
            assert!(
                !RaycastTargetPolicy::Break.targets(block),
                "{block:?} is environmental, not a mineable target"
            );
        }

        assert!(
            RaycastTargetPolicy::Break.targets(BlockType::EndPortal),
            "End portals must be targetable so Creative mode can remove them"
        );
    }

    #[test]
    fn break_raycast_hits_each_explicit_passable_decoration() {
        let mut chunk_manager = ChunkManager::new(8);
        chunk_manager.chunks.insert((0, 0), Chunk::new(0, 0));

        for block in [
            BlockType::TallGrass,
            BlockType::Dandelion,
            BlockType::Poppy,
            BlockType::SugarCane,
            BlockType::RedstoneWire,
            BlockType::RedstoneTorch,
            BlockType::PressurePlate,
            BlockType::SnowLayer,
            BlockType::WitherSkeletonSkull,
        ] {
            chunk_manager.set_block(8, 72, 8, block);
            let hit = raycast(
                Vec3::new(8.5, 70.5, 8.5),
                Vec3::Y,
                5.0,
                &chunk_manager,
                RaycastTargetPolicy::Break,
            )
            .unwrap_or_else(|| panic!("{block:?} should be selected for breaking"));
            assert_eq!(hit.block_pos, Vec3::new(8.0, 72.0, 8.0));
        }
    }

    #[test]
    fn break_raycast_skips_water_and_lava_for_solid_behind_them() {
        let mut chunk_manager = ChunkManager::new(8);
        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(8, 71, 8, BlockType::Water);
        chunk.set_block_local(8, 72, 8, BlockType::Lava);
        chunk.set_block_local(8, 73, 8, BlockType::Stone);
        chunk_manager.chunks.insert((0, 0), chunk);

        let hit = raycast(
            Vec3::new(8.5, 70.5, 8.5),
            Vec3::Y,
            5.0,
            &chunk_manager,
            RaycastTargetPolicy::Break,
        )
        .expect("solid behind fluids should remain mineable");

        assert_eq!(hit.block_pos, Vec3::new(8.0, 73.0, 8.0));
        assert_eq!(hit.normal, Vec3::NEG_Y);
    }

    #[test]
    fn break_raycast_returns_none_for_only_environmental_passables() {
        let mut chunk_manager = ChunkManager::new(8);
        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(8, 71, 8, BlockType::Water);
        chunk.set_block_local(8, 72, 8, BlockType::Lava);
        chunk.set_block_local(8, 73, 8, BlockType::Fire);
        chunk_manager.chunks.insert((0, 0), chunk);

        assert!(raycast(
            Vec3::new(8.5, 70.5, 8.5),
            Vec3::Y,
            4.0,
            &chunk_manager,
            RaycastTargetPolicy::Break,
        )
        .is_none());
    }

    #[test]
    fn place_raycast_ignores_passable_vegetation_and_fluids() {
        let mut chunk_manager = ChunkManager::new(8);
        let mut chunk = Chunk::new(0, 0);
        chunk.set_block_local(8, 71, 8, BlockType::TallGrass);
        chunk.set_block_local(8, 72, 8, BlockType::Water);
        chunk.set_block_local(8, 73, 8, BlockType::Lava);
        chunk.set_block_local(8, 74, 8, BlockType::Stone);
        chunk_manager.chunks.insert((0, 0), chunk);

        let hit = raycast(
            Vec3::new(8.5, 70.5, 8.5),
            Vec3::new(0.0, 1.0, 0.0),
            5.0,
            &chunk_manager,
            RaycastTargetPolicy::Place,
        );
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().block_pos, Vec3::new(8.0, 74.0, 8.0));
    }

    #[test]
    fn dda_crosses_exact_boundary_into_negative_coordinates() {
        let mut chunk_manager = ChunkManager::new(8);
        let mut chunk = Chunk::new(-1, 0);
        for x in 0..crate::world::CHUNK_WIDTH {
            for y in 0..crate::world::CHUNK_HEIGHT {
                for z in 0..crate::world::CHUNK_DEPTH {
                    chunk.set_block_local(x, y as i32, z, BlockType::Air);
                }
            }
        }
        chunk_manager.chunks.insert((-1, 0), chunk);
        chunk_manager.set_block(-1, 64, 0, BlockType::Stone);

        let from_boundary = raycast(
            Vec3::new(0.0, 64.5, 0.5),
            Vec3::NEG_X,
            2.0,
            &chunk_manager,
            RaycastTargetPolicy::Place,
        )
        .expect("negative cell touching the origin boundary should be visited");
        assert_eq!(from_boundary.block_pos, Vec3::new(-1.0, 64.0, 0.0));
        assert_eq!(from_boundary.normal, Vec3::X);

        let from_negative_cell = raycast(
            Vec3::new(-2.5, 64.5, 0.5),
            Vec3::X,
            3.0,
            &chunk_manager,
            RaycastTargetPolicy::Break,
        )
        .expect("DDA should traverse negative world cells using floor coordinates");
        assert_eq!(from_negative_cell.block_pos, Vec3::new(-1.0, 64.0, 0.0));
        assert_eq!(from_negative_cell.normal, Vec3::NEG_X);
    }
}

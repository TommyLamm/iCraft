use crate::chunk_manager::ChunkManager;
use crate::voxel_shape::VoxelShape;
use crate::world::BlockType;
use glam::Vec3;

const CREATIVE_FLY_SPEED: f32 = 10.0;
const CREATIVE_FLY_SPRINT_MULTIPLIER: f32 = 2.0;
const CREATIVE_FLY_VERTICAL_SPEED: f32 = 8.0;
const MAX_COLLISION_STEP: f32 = 0.25;
/// The simulation advances the player on a fixed 50 ms tick.  A tick is
/// subdivided into four 12.5 ms integration steps so that a collision query
/// never has to account for a full frame-sized displacement.  Larger caller
/// timesteps are subdivided as well (up to the bounded catch-up budget) to
/// preserve the public API's elapsed-time semantics.
pub const PLAYER_PHYSICS_TICK_DT: f32 = 1.0 / 20.0;
pub const PLAYER_PHYSICS_SUBSTEP_DT: f32 = PLAYER_PHYSICS_TICK_DT / 4.0;
pub const PLAYER_PHYSICS_MAX_SUBSTEPS: usize = 4;
/// Maximum collision probes per axis in one integration substep.  The
/// displacement is clamped to this many MAX_COLLISION_STEP-sized probes, so
/// bounding the loop cannot skip a solid block (tunneling).
pub const PLAYER_MAX_COLLISION_ITERATIONS: usize = 32;
pub const PLAYER_WIDTH: f32 = 0.6;
pub const PLAYER_STANDING_HEIGHT: f32 = 1.8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AABB {
    pub min: Vec3,
    pub max: Vec3,
}

impl AABB {
    pub fn new(center: Vec3, size: Vec3) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn intersects(&self, other: &AABB) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
            && self.min.z < other.max.z
            && self.max.z > other.min.z
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPlacementDecision {
    Allowed,
    BlockedByPlayer,
}

/// Build the standing player collision box from a foot-centred world position.
///
/// Remote movement snapshots use this canonical standing size because the
/// multiplayer protocol does not currently transmit crouching state.
pub fn player_aabb_at(feet_position: Vec3) -> AABB {
    let size = Vec3::new(PLAYER_WIDTH, PLAYER_STANDING_HEIGHT, PLAYER_WIDTH);
    AABB::new(
        feet_position + Vec3::new(0.0, PLAYER_STANDING_HEIGHT * 0.5, 0.0),
        size,
    )
}

/// Build the full-cube collision box occupied by one world block cell.
pub fn unit_block_aabb((x, y, z): (i32, i32, i32)) -> AABB {
    AABB::new(
        Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
        Vec3::ONE,
    )
}

/// Pure placement policy shared by local preflight and host authority.
///
/// The current physics model treats every solid block as one full cube. A
/// placement is blocked only when that cube and a player box have positive
/// overlap on all three axes; merely touching a face, edge, or corner remains
/// legal through `AABB::intersects`' strict comparisons. Non-solid blocks do
/// not displace players and are therefore always allowed by this policy.
/// Build the collision box for a block, taking into account block states (e.g. doors, trapdoors).
/// Build the VoxelShape for a block, taking into account block states and shape.
/// Returns a VoxelShape that can contain multiple AABBs for complex blocks.
pub fn block_shape(block: BlockType, state_raw: u8, pos: (i32, i32, i32)) -> VoxelShape {
    crate::voxel_shape::block_collision_shape(block, state_raw, pos, None)
}

/// Backward-compatible wrapper that returns the first AABB of a block shape.
pub fn block_aabb(block: BlockType, state_raw: u8, pos: (i32, i32, i32)) -> AABB {
    let shape = block_shape(block, state_raw, pos);
    if shape.count > 0 {
        shape.boxes[0]
    } else {
        AABB {
            min: Vec3::ZERO,
            max: Vec3::ZERO,
        }
    }
}

pub fn block_placement_decision(
    block: BlockType,
    block_state: u8,
    block_pos: (i32, i32, i32),
    player_aabbs: impl IntoIterator<Item = AABB>,
) -> BlockPlacementDecision {
    if !block.properties().is_solid {
        return BlockPlacementDecision::Allowed;
    }

    let shape = block_shape(block, block_state, block_pos);
    if player_aabbs
        .into_iter()
        .any(|player_aabb| shape.intersects(&player_aabb))
    {
        BlockPlacementDecision::BlockedByPlayer
    } else {
        BlockPlacementDecision::Allowed
    }
}

pub struct PlayerPhysics {
    pub position: Vec3,
    pub velocity: Vec3,
    pub size: Vec3,
    pub on_ground: bool,
    pub highest_y: f32,
    is_flying: bool,
}

impl PlayerPhysics {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            size: Vec3::new(PLAYER_WIDTH, PLAYER_STANDING_HEIGHT, PLAYER_WIDTH),
            on_ground: false,
            highest_y: position.y,
            is_flying: false,
        }
    }

    pub fn is_flying(&self) -> bool {
        self.is_flying
    }

    pub fn persistent_velocity(&self) -> Vec3 {
        if self.is_flying {
            Vec3::ZERO
        } else {
            self.velocity
        }
    }

    pub fn set_flying(&mut self, flying: bool) {
        if self.is_flying == flying {
            return;
        }
        self.is_flying = flying;
        self.velocity.y = 0.0;
        self.highest_y = self.position.y;
        if flying {
            self.on_ground = false;
        }
    }

    pub fn get_aabb(&self) -> AABB {
        AABB::new(
            self.position + Vec3::new(0.0, self.size.y * 0.5, 0.0),
            self.size,
        )
    }

    pub fn update(
        &mut self,
        dt: f32,
        chunk_manager: &ChunkManager,
        movement_input: Vec3,
        is_sneaking: bool,
        is_sprinting: bool,
    ) -> f32 {
        // Keep player integration bounded even when a caller hands us a long
        // render hitch.  A normal 50 ms tick takes four substeps; unusually
        // large timesteps are capped to the fixed catch-up budget while still
        // preserving their elapsed duration in the per-substep dt.
        let integration_dt = dt.max(0.0);
        let substeps = ((integration_dt / PLAYER_PHYSICS_SUBSTEP_DT).ceil() as usize)
            .clamp(1, PLAYER_PHYSICS_MAX_SUBSTEPS);
        let substep_dt = integration_dt / substeps as f32;
        let edge_guard_origin = if is_sneaking && !self.is_flying && self.on_ground {
            Some((self.position.x, self.position.z))
        } else {
            None
        };
        let mut edge_guard_blocked = (false, false);
        let mut fall_damage = 0.0;
        for _ in 0..substeps {
            fall_damage += self.update_substep(
                substep_dt,
                chunk_manager,
                movement_input,
                is_sneaking,
                is_sprinting,
                edge_guard_origin,
                &mut edge_guard_blocked,
            );
        }
        if edge_guard_blocked.0 {
            self.velocity.x = 0.0;
        }
        if edge_guard_blocked.1 {
            self.velocity.z = 0.0;
        }
        fall_damage
    }

    fn update_substep(
        &mut self,
        dt: f32,
        chunk_manager: &ChunkManager,
        movement_input: Vec3,
        is_sneaking: bool,
        is_sprinting: bool,
        edge_guard_origin: Option<(f32, f32)>,
        edge_guard_blocked: &mut (bool, bool),
    ) -> f32 {
        // Hitbox size adjustment
        if is_sneaking {
            self.size.y = 1.5;
        } else {
            self.size.y = 1.8;
        }

        let was_on_ground = self.on_ground;
        let is_flying = self.is_flying;

        let px = self.position.x.floor() as i32;
        let py = self.position.y.floor() as i32;
        let pz = self.position.z.floor() as i32;
        let block_at_feet = chunk_manager.get_block(px, py, pz);
        let block_at_eyes =
            chunk_manager.get_block(px, (self.position.y + 1.62).floor() as i32, pz);

        let is_in_water = block_at_feet == crate::world::BlockType::Water
            || block_at_eyes == crate::world::BlockType::Water;
        let is_in_lava = block_at_feet == crate::world::BlockType::Lava
            || block_at_eyes == crate::world::BlockType::Lava;

        // 1. 套用玩家移動控制
        let mut speed = if is_flying { CREATIVE_FLY_SPEED } else { 8.0 };
        if is_flying {
            if is_sprinting {
                speed *= CREATIVE_FLY_SPRINT_MULTIPLIER;
            }
        } else {
            if is_sprinting {
                speed *= 1.3;
            } else if is_sneaking {
                speed *= 0.3;
            }
            if is_in_water {
                speed *= 0.6;
            } else if is_in_lava {
                speed *= 0.3;
            }
        }
        self.velocity.x = movement_input.x * speed;
        self.velocity.z = movement_input.z * speed;

        // 2. 套用重力與跳躍
        let is_on_ladder = block_at_feet == crate::world::BlockType::OakLadder
            || block_at_eyes == crate::world::BlockType::OakLadder;

        if is_flying {
            self.velocity.y = movement_input.y.clamp(-1.0, 1.0) * CREATIVE_FLY_VERTICAL_SPEED;
        } else if is_on_ladder {
            self.highest_y = self.position.y;
            if is_sneaking {
                self.velocity.y = 0.0;
            } else if movement_input.y > 0.0 || movement_input.x != 0.0 || movement_input.z != 0.0 {
                self.velocity.y = 3.5;
            } else {
                self.velocity.y = self.velocity.y.max(-2.5);
            }
        } else if is_in_water {
            if movement_input.y > 0.0 {
                self.velocity.y = 2.5; // Swim up buoyancy
            } else {
                self.velocity.y -= 12.0 * dt;
            }
            self.velocity.y = self.velocity.y.max(-2.0); // Terminal velocity cap in water
        } else if is_in_lava {
            if movement_input.y > 0.0 {
                self.velocity.y = 1.0; // Swim up buoyancy in lava
            } else {
                self.velocity.y -= 8.0 * dt;
            }
            self.velocity.y = self.velocity.y.max(-0.5); // Terminal velocity cap in lava
        } else {
            if movement_input.y > 0.0 && self.on_ground {
                self.velocity.y = 10.0;
            }
            self.velocity.y -= 32.0 * dt;
            if self.velocity.y < -50.0 {
                self.velocity.y = -50.0; // 終端速度
            }
        }

        // 3. 沿 X 軸位移並處理碰撞
        let old_x = self.position.x;
        if !edge_guard_blocked.0 {
            let x_displacement = self.velocity.x * dt;
            self.move_axis_with_collisions(chunk_manager, 0, x_displacement);
            if !is_flying && is_sneaking && self.on_ground {
                if !self.is_block_below(chunk_manager) {
                    self.position.x = edge_guard_origin.map_or(old_x, |origin| origin.0);
                    self.velocity.x = 0.0;
                    edge_guard_blocked.0 = true;
                }
            }
        }

        // 4. 沿 Z 軸位移並處理碰撞
        let old_z = self.position.z;
        if !edge_guard_blocked.1 {
            let z_displacement = self.velocity.z * dt;
            self.move_axis_with_collisions(chunk_manager, 2, z_displacement);
            if !is_flying && is_sneaking && self.on_ground {
                if !self.is_block_below(chunk_manager) {
                    self.position.z = edge_guard_origin.map_or(old_z, |origin| origin.1);
                    self.velocity.z = 0.0;
                    edge_guard_blocked.1 = true;
                }
            }
        }

        // 5. 沿 Y 軸位移並處理碰撞
        let y_displacement = self.velocity.y * dt;
        self.on_ground = false;
        self.move_axis_with_collisions(chunk_manager, 1, y_displacement);

        if is_flying {
            self.highest_y = self.position.y;
            return 0.0;
        }

        // Calculate fall damage on landing
        let mut fall_damage = 0.0;
        if !was_on_ground && self.on_ground {
            let fall_distance = self.highest_y - self.position.y;
            if fall_distance > 3.0 {
                fall_damage = fall_distance - 3.0;
            }
        }

        if self.on_ground || is_in_water || is_in_lava || is_on_ladder {
            self.highest_y = self.position.y;
        } else {
            self.highest_y = self.highest_y.max(self.position.y);
        }

        fall_damage
    }

    fn move_axis_with_collisions(
        &mut self,
        chunk_manager: &ChunkManager,
        axis: usize,
        displacement: f32,
    ) {
        let max_displacement = MAX_COLLISION_STEP * PLAYER_MAX_COLLISION_ITERATIONS as f32;
        let bounded_displacement = displacement.clamp(-max_displacement, max_displacement);
        let step_count = (bounded_displacement.abs() / MAX_COLLISION_STEP)
            .ceil()
            .max(1.0) as usize;
        let step = bounded_displacement / step_count as f32;

        for _ in 0..step_count {
            match axis {
                0 => self.position.x += step,
                1 => self.position.y += step,
                2 => self.position.z += step,
                _ => unreachable!("invalid movement axis"),
            }
            self.resolve_collisions(chunk_manager, axis);

            let blocked = match axis {
                0 => self.velocity.x == 0.0,
                1 => self.velocity.y == 0.0,
                2 => self.velocity.z == 0.0,
                _ => unreachable!("invalid movement axis"),
            };
            if bounded_displacement != 0.0 && blocked {
                break;
            }
        }
    }

    fn resolve_collisions(&mut self, chunk_manager: &ChunkManager, axis: usize) {
        let player_aabb = self.get_aabb();
        let height = chunk_manager.dimension.height();

        // 檢測玩家周圍可能相交的方塊
        let min_x = player_aabb.min.x.floor() as i32;
        let max_x = player_aabb.max.x.floor() as i32;
        let min_y =
            (player_aabb.min.y.floor() as i32).clamp(height.min_y, height.max_y_exclusive() - 1);
        let max_y =
            (player_aabb.max.y.floor() as i32).clamp(height.min_y, height.max_y_exclusive() - 1);
        let min_z = player_aabb.min.z.floor() as i32;
        let max_z = player_aabb.max.z.floor() as i32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let block = chunk_manager.get_block(x, y, z);
                    if block.properties().is_solid {
                        let state = chunk_manager.get_block_state(x, y, z);
                        let shape = crate::voxel_shape::block_collision_shape(
                            block,
                            state,
                            (x, y, z),
                            Some(chunk_manager),
                        );

                        for block_aabb in shape.iter() {
                            if self.get_aabb().intersects(block_aabb) {
                                if axis == 0 {
                                    // X 軸
                                    if self.velocity.x > 0.0 {
                                        self.position.x = block_aabb.min.x - self.size.x * 0.5;
                                    } else {
                                        self.position.x = block_aabb.max.x + self.size.x * 0.5;
                                    }
                                    self.velocity.x = 0.0;
                                } else if axis == 2 {
                                    // Z 軸
                                    if self.velocity.z > 0.0 {
                                        self.position.z = block_aabb.min.z - self.size.z * 0.5;
                                    } else {
                                        self.position.z = block_aabb.max.z + self.size.z * 0.5;
                                    }
                                    self.velocity.z = 0.0;
                                } else if axis == 1 {
                                    // Y 軸
                                    if self.velocity.y > 0.0 {
                                        self.position.y = block_aabb.min.y - self.size.y;
                                    } else {
                                        self.position.y = block_aabb.max.y;
                                        self.on_ground = true;
                                    }
                                    self.velocity.y = 0.0;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn is_block_below(&self, chunk_manager: &ChunkManager) -> bool {
        let mut check_aabb = self.get_aabb();
        check_aabb.min.y -= 0.05;
        check_aabb.max.y = self.position.y;

        let min_x = check_aabb.min.x.floor() as i32;
        let max_x = check_aabb.max.x.floor() as i32;
        let height = chunk_manager.dimension.height();
        let min_y =
            (check_aabb.min.y.floor() as i32).clamp(height.min_y, height.max_y_exclusive() - 1);
        let max_y =
            (check_aabb.max.y.floor() as i32).clamp(height.min_y, height.max_y_exclusive() - 1);
        let min_z = check_aabb.min.z.floor() as i32;
        let max_z = check_aabb.max.z.floor() as i32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let block = chunk_manager.get_block(x, y, z);
                    if block.properties().is_solid {
                        let state = chunk_manager.get_block_state(x, y, z);
                        let shape = crate::voxel_shape::block_collision_shape(
                            block,
                            state,
                            (x, y, z),
                            Some(chunk_manager),
                        );
                        if shape.intersects(&check_aabb) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ChestType;
    use crate::world::{BlockType, Chunk};

    fn empty_chunk_manager() -> ChunkManager {
        let mut chunk_manager = ChunkManager::new(2);
        let chunk = Chunk::new(0, 0);
        chunk_manager.chunks.insert((0, 0), chunk);
        chunk_manager
    }

    #[test]
    fn test_aabb_intersection() {
        let box1 = AABB::new(Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        let box2 = AABB::new(Vec3::new(0.8, 0.0, 0.0), Vec3::ONE);
        let box3 = AABB::new(Vec3::new(1.5, 0.0, 0.0), Vec3::ONE);

        assert!(box1.intersects(&box2));
        assert!(!box1.intersects(&box3));
    }

    #[test]
    fn player_and_unit_block_aabbs_use_foot_and_cell_coordinates() {
        let player = player_aabb_at(Vec3::new(4.5, 20.0, -2.5));
        assert!((player.min.x - 4.2).abs() < 1.0e-6);
        assert!((player.max.x - 4.8).abs() < 1.0e-6);
        assert!((player.min.y - 20.0).abs() < 1.0e-6);
        assert!((player.max.y - 21.8).abs() < 1.0e-6);
        assert!((player.min.z + 2.8).abs() < 1.0e-6);
        assert!((player.max.z + 2.2).abs() < 1.0e-6);

        assert_eq!(
            unit_block_aabb((-3, 7, 11)),
            AABB {
                min: Vec3::new(-3.0, 7.0, 11.0),
                max: Vec3::new(-2.0, 8.0, 12.0),
            }
        );
    }

    #[test]
    fn solid_placement_is_blocked_by_positive_player_overlap() {
        assert_eq!(
            block_placement_decision(
                BlockType::Stone,
                0,
                (0, 0, 0),
                [player_aabb_at(Vec3::new(0.5, 0.0, 0.5))]
            ),
            BlockPlacementDecision::BlockedByPlayer
        );
    }

    #[test]
    fn solid_placement_allows_face_edge_and_corner_touching() {
        let face_touch = AABB {
            min: Vec3::new(0.2, 1.0, 0.2),
            max: Vec3::new(0.8, 2.8, 0.8),
        };
        let edge_touch = AABB {
            min: Vec3::new(1.0, 1.0, 0.2),
            max: Vec3::new(1.6, 2.8, 0.8),
        };
        let corner_touch = AABB {
            min: Vec3::new(1.0, 1.0, 1.0),
            max: Vec3::new(1.6, 2.8, 1.6),
        };

        for player in [face_touch, edge_touch, corner_touch] {
            assert_eq!(
                block_placement_decision(BlockType::Stone, 0, (0, 0, 0), [player]),
                BlockPlacementDecision::Allowed
            );
        }
    }

    #[test]
    fn non_solid_placement_is_allowed_inside_player_aabb() {
        assert_eq!(
            block_placement_decision(
                BlockType::Torch,
                0,
                (0, 0, 0),
                [player_aabb_at(Vec3::new(0.5, 0.0, 0.5))]
            ),
            BlockPlacementDecision::Allowed
        );
    }

    #[test]
    fn door_and_trapdoor_block_aabbs_use_thin_slab_bounds() {
        use crate::redstone::Direction;
        use crate::world::BlockState;

        let closed_door_state = BlockState {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
            chest_type: ChestType::Single,
        };
        let aabb = block_aabb(BlockType::OakDoor, closed_door_state.encode(), (2, 10, 2));
        assert_eq!(aabb.min, Vec3::new(2.0, 10.0, 2.0));
        assert_eq!(aabb.max, Vec3::new(3.0, 11.0, 2.1875));

        let open_door_state = BlockState {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: true,
            chest_type: ChestType::Single,
        };
        let open_aabb = block_aabb(BlockType::OakDoor, open_door_state.encode(), (2, 10, 2));
        assert_eq!(open_aabb.min, Vec3::new(2.0, 10.0, 2.0));
        assert_eq!(open_aabb.max, Vec3::new(2.1875, 11.0, 3.0));

        let closed_trapdoor = BlockState {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
            chest_type: ChestType::Single,
        };
        let trap_aabb = block_aabb(BlockType::OakTrapdoor, closed_trapdoor.encode(), (0, 64, 0));
        assert_eq!(trap_aabb.min, Vec3::new(0.0, 64.0, 0.0));
        assert_eq!(trap_aabb.max, Vec3::new(1.0, 64.1875, 1.0));
    }

    #[test]
    fn test_player_sneaking_speed() {
        let chunk_manager = ChunkManager::new(2);
        let mut physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0));
        physics.on_ground = false;
        let dt = 0.1;

        physics.update(dt, &chunk_manager, Vec3::new(1.0, 0.0, 0.0), true, false);
        // Sneak speed: 8.0 * 0.3 = 2.4
        assert_eq!(physics.velocity.x, 2.4);
    }

    #[test]
    fn test_player_sprinting_speed() {
        let chunk_manager = ChunkManager::new(2);
        let mut physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0));
        physics.on_ground = true;
        let dt = 0.1;

        physics.update(dt, &chunk_manager, Vec3::new(1.0, 0.0, 0.0), false, true);
        // Sprint speed: 8.0 * 1.3 = 10.4
        assert_eq!(physics.velocity.x, 10.4);
    }

    #[test]
    fn test_player_edge_guard() {
        let mut chunk_manager = empty_chunk_manager();
        // Set one stone block at (8, 70, 8)
        chunk_manager
            .chunks
            .get_mut(&(0, 0))
            .unwrap()
            .set_block_local(8, 70, 8, BlockType::Stone);

        let mut physics = PlayerPhysics::new(Vec3::new(8.5, 71.0, 8.5));
        physics.on_ground = true;
        // dt = 0.5, speed = 2.4 => displacement = 1.2.
        // Walking to X = 9.7 (min X = 9.4), which is off the block.
        // Edge guard should prevent it and revert position to 8.5.
        let dt = 0.5;

        physics.update(dt, &chunk_manager, Vec3::new(1.0, 0.0, 0.0), true, false);
        assert_eq!(physics.position.x, 8.5);
        assert_eq!(physics.velocity.x, 0.0);
    }

    #[test]
    fn creative_flight_toggle_clears_vertical_momentum_and_fall_distance() {
        let mut physics = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
        physics.velocity.y = -30.0;
        physics.highest_y = 120.0;
        physics.on_ground = true;

        physics.set_flying(true);
        assert!(physics.is_flying());
        assert_eq!(physics.velocity.y, 0.0);
        physics.velocity = Vec3::new(10.0, 8.0, -4.0);
        assert_eq!(physics.persistent_velocity(), Vec3::ZERO);
        assert_eq!(physics.highest_y, 80.0);
        assert!(!physics.on_ground);

        physics.set_flying(false);
        assert!(!physics.is_flying());
        assert_eq!(physics.velocity.y, 0.0);
        assert_eq!(physics.persistent_velocity(), Vec3::new(10.0, 0.0, -4.0));
        assert_eq!(physics.highest_y, 80.0);
    }

    #[test]
    fn creative_flight_hovers_and_moves_vertically_without_fall_damage() {
        let mut chunk_manager = empty_chunk_manager();
        chunk_manager
            .chunks
            .get_mut(&(0, 0))
            .unwrap()
            .set_block_local(8, 80, 8, BlockType::Water);
        let mut physics = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
        physics.set_flying(true);

        let start = physics.position;
        assert_eq!(
            physics.update(0.25, &chunk_manager, Vec3::ZERO, false, false),
            0.0
        );
        assert_eq!(physics.position, start);
        assert_eq!(physics.velocity, Vec3::ZERO);

        assert_eq!(
            physics.update(0.25, &chunk_manager, Vec3::Y, false, false),
            0.0
        );
        assert!((physics.position.y - (start.y + 2.0)).abs() < 1.0e-5);
        assert_eq!(physics.velocity.y, CREATIVE_FLY_VERTICAL_SPEED);

        assert_eq!(
            physics.update(0.25, &chunk_manager, -Vec3::Y, false, false),
            0.0
        );
        assert!((physics.position.y - start.y).abs() < 1.0e-5);
        assert_eq!(physics.velocity.y, -CREATIVE_FLY_VERTICAL_SPEED);
        assert_eq!(physics.highest_y, physics.position.y);
    }

    #[test]
    fn creative_flight_keeps_solid_collision_on_every_axis() {
        let mut chunk_manager = empty_chunk_manager();
        let chunk = chunk_manager.chunks.get_mut(&(0, 0)).unwrap();
        chunk.set_block_local(9, 80, 8, BlockType::Stone);
        chunk.set_block_local(9, 81, 8, BlockType::Stone);
        chunk.set_block_local(8, 82, 8, BlockType::Stone);
        chunk.set_block_local(8, 79, 8, BlockType::Stone);

        let mut wall = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
        wall.set_flying(true);
        wall.update(0.1, &chunk_manager, Vec3::X, false, false);
        assert!((wall.position.x - 8.7).abs() < 1.0e-5);
        assert_eq!(wall.velocity.x, 0.0);

        let mut ceiling = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
        ceiling.set_flying(true);
        ceiling.update(0.1, &chunk_manager, Vec3::Y, false, false);
        assert!((ceiling.position.y - 80.2).abs() < 1.0e-5);
        assert_eq!(ceiling.velocity.y, 0.0);
        assert!(ceiling.is_flying());
        assert!(!ceiling.on_ground);

        let mut landing = PlayerPhysics::new(Vec3::new(8.5, 80.1, 8.5));
        landing.set_flying(true);
        landing.update(0.05, &chunk_manager, -Vec3::Y, false, false);
        assert!((landing.position.y - 80.0).abs() < 1.0e-5);
        assert_eq!(landing.velocity.y, 0.0);
        assert!(landing.on_ground);
    }

    #[test]
    fn creative_flight_sprint_changes_horizontal_speed_without_fluid_drag() {
        let mut chunk_manager = empty_chunk_manager();
        chunk_manager
            .chunks
            .get_mut(&(0, 0))
            .unwrap()
            .set_block_local(8, 80, 8, BlockType::Lava);
        let mut physics = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
        physics.set_flying(true);

        physics.update(0.0, &chunk_manager, Vec3::X, true, false);
        assert_eq!(physics.velocity.x, CREATIVE_FLY_SPEED);

        physics.update(0.0, &chunk_manager, Vec3::X, false, true);
        assert_eq!(
            physics.velocity.x,
            CREATIVE_FLY_SPEED * CREATIVE_FLY_SPRINT_MULTIPLIER
        );
    }

    #[test]
    fn high_speed_motion_cannot_tunnel_through_one_block_barriers_on_any_axis() {
        let mut chunk_manager = empty_chunk_manager();
        let chunk = chunk_manager.chunks.get_mut(&(0, 0)).unwrap();
        chunk.set_block_local(9, 80, 8, BlockType::Stone);
        chunk.set_block_local(9, 81, 8, BlockType::Stone);
        chunk.set_block_local(8, 80, 9, BlockType::Stone);
        chunk.set_block_local(8, 81, 9, BlockType::Stone);
        chunk.set_block_local(8, 83, 8, BlockType::Stone);

        let mut x_motion = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
        x_motion.set_flying(true);
        x_motion.update(0.1, &chunk_manager, Vec3::X, false, true);
        assert!((x_motion.position.x - 8.7).abs() < 1.0e-5);
        assert_eq!(x_motion.velocity.x, 0.0);

        let mut z_motion = PlayerPhysics::new(Vec3::new(8.5, 80.0, 8.5));
        z_motion.set_flying(true);
        z_motion.update(0.1, &chunk_manager, Vec3::Z, false, true);
        assert!((z_motion.position.z - 8.7).abs() < 1.0e-5);
        assert_eq!(z_motion.velocity.z, 0.0);

        let mut falling = PlayerPhysics::new(Vec3::new(8.5, 86.0, 8.5));
        falling.velocity.y = -50.0;
        falling.highest_y = falling.position.y;
        let fall_damage = falling.update(0.1, &chunk_manager, Vec3::ZERO, false, false);
        assert!((falling.position.y - 84.0).abs() < 1.0e-5);
        assert_eq!(falling.velocity.y, 0.0);
        assert!(falling.on_ground);
        assert_eq!(fall_damage, 0.0);
    }

    #[test]
    fn extreme_input_is_displacement_capped_without_tunneling() {
        let mut chunk_manager = empty_chunk_manager();
        let chunk = chunk_manager.chunks.get_mut(&(0, 0)).unwrap();
        chunk.set_block_local(3, 80, 8, BlockType::Stone);
        chunk.set_block_local(3, 81, 8, BlockType::Stone);

        let mut physics = PlayerPhysics::new(Vec3::new(0.5, 80.0, 8.5));
        physics.set_flying(true);
        physics.update(
            PLAYER_PHYSICS_TICK_DT,
            &chunk_manager,
            Vec3::new(1_000.0, 0.0, 0.0),
            false,
            true,
        );

        // The wall's near face is x=3; the player must stop at x=2.7 even
        // though the requested displacement is many orders of magnitude
        // larger than one block.
        assert!((physics.position.x - 2.7).abs() < 1.0e-5);
        assert_eq!(physics.velocity.x, 0.0);
        assert!(!physics.get_aabb().intersects(&unit_block_aabb((3, 80, 8))));

        let mut open = PlayerPhysics::new(Vec3::new(0.5, 80.0, 8.5));
        open.set_flying(true);
        open.update(
            PLAYER_PHYSICS_TICK_DT,
            &empty_chunk_manager(),
            Vec3::new(1_000.0, 0.0, 0.0),
            false,
            true,
        );
        assert!(
            open.position.x - 0.5
                <= MAX_COLLISION_STEP
                    * PLAYER_MAX_COLLISION_ITERATIONS as f32
                    * PLAYER_PHYSICS_MAX_SUBSTEPS as f32
        );
    }

    #[test]
    fn a_50ms_tick_matches_four_shared_player_substeps() {
        let chunk_manager = empty_chunk_manager();
        let input = Vec3::new(0.2, 0.0, 0.1);
        let mut tick = PlayerPhysics::new(Vec3::new(8.5, 72.0, 8.5));
        tick.update(PLAYER_PHYSICS_TICK_DT, &chunk_manager, input, false, false);

        let mut split = PlayerPhysics::new(Vec3::new(8.5, 72.0, 8.5));
        for _ in 0..4 {
            split.update(
                PLAYER_PHYSICS_SUBSTEP_DT,
                &chunk_manager,
                input,
                false,
                false,
            );
        }

        assert_eq!(tick.position.to_array(), split.position.to_array());
        assert_eq!(tick.velocity.to_array(), split.velocity.to_array());
        assert_eq!(tick.on_ground, split.on_ground);
    }

    #[test]
    fn non_flying_gravity_and_fall_damage_are_unchanged() {
        let mut chunk_manager = empty_chunk_manager();
        chunk_manager
            .chunks
            .get_mut(&(0, 0))
            .unwrap()
            .set_block_local(8, 79, 8, BlockType::Stone);
        let mut physics = PlayerPhysics::new(Vec3::new(8.5, 85.0, 8.5));
        physics.highest_y = physics.position.y;

        let mut fall_damage = 0.0;
        for _ in 0..500 {
            fall_damage = physics.update(0.01, &chunk_manager, Vec3::ZERO, false, false);
            if physics.on_ground {
                break;
            }
        }

        assert!(!physics.is_flying());
        assert!(physics.on_ground);
        assert!((physics.position.y - 80.0).abs() < 1.0e-5);
        assert!((fall_damage - 2.0).abs() < 0.05);
    }
}

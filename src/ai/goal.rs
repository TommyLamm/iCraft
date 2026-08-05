use crate::chunk_manager::ChunkManager;
use crate::entity::{Entity, EntityType};
use glam::Vec3;

pub struct GoalContext<'a> {
    pub chunk_manager: &'a ChunkManager,
    pub player_position: Vec3,
    pub player_holding_item: Option<crate::inventory::Item>,
    pub delta_time: f32,
    pub target_entity_pos: Option<Vec3>,
}

pub trait Goal: Send + Sync {
    fn priority(&self) -> u8;
    fn can_start(&mut self, entity: &Entity, ctx: &GoalContext) -> bool;
    fn should_continue(&mut self, entity: &Entity, ctx: &GoalContext) -> bool;
    fn start(&mut self, _entity: &mut Entity, _ctx: &mut GoalContext) {}
    fn stop(&mut self, _entity: &mut Entity, _ctx: &mut GoalContext) {}
    fn tick(&mut self, entity: &mut Entity, ctx: &mut GoalContext);
}

// 1. Swim Goal (highest priority for floating in water)
pub struct SwimGoal;

impl Goal for SwimGoal {
    fn priority(&self) -> u8 {
        1
    }

    fn can_start(&mut self, entity: &Entity, ctx: &GoalContext) -> bool {
        let bx = entity.position.x.floor() as i32;
        let by = entity.position.y.floor() as i32;
        let bz = entity.position.z.floor() as i32;
        let block = ctx.chunk_manager.get_block(bx, by, bz);
        block == crate::world::BlockType::Water
    }

    fn should_continue(&mut self, entity: &Entity, ctx: &GoalContext) -> bool {
        self.can_start(entity, ctx)
    }

    fn tick(&mut self, entity: &mut Entity, _ctx: &mut GoalContext) {
        if entity.velocity.y < 2.0 {
            entity.velocity.y += 4.0 * _ctx.delta_time;
        }
    }
}

// 2. Sit Goal (for tamed pets)
pub struct SitGoal;

impl Goal for SitGoal {
    fn priority(&self) -> u8 {
        2
    }

    fn can_start(&mut self, entity: &Entity, _ctx: &GoalContext) -> bool {
        entity.is_sitting()
    }

    fn should_continue(&mut self, entity: &Entity, _ctx: &GoalContext) -> bool {
        entity.is_sitting()
    }

    fn tick(&mut self, entity: &mut Entity, _ctx: &mut GoalContext) {
        entity.velocity.x = 0.0;
        entity.velocity.z = 0.0;
    }
}

// 3. Follow Owner Goal (for tamed pets standing up)
pub struct FollowOwnerGoal;

impl Goal for FollowOwnerGoal {
    fn priority(&self) -> u8 {
        4
    }

    fn can_start(&mut self, entity: &Entity, ctx: &GoalContext) -> bool {
        if !entity.is_tamed() || entity.is_sitting() || entity.owner_id.is_none() {
            return false;
        }
        let dist_sq = entity.position.distance_squared(ctx.player_position);
        dist_sq > 4.0 * 4.0
    }

    fn should_continue(&mut self, entity: &Entity, ctx: &GoalContext) -> bool {
        if !entity.is_tamed() || entity.is_sitting() || entity.owner_id.is_none() {
            return false;
        }
        let dist_sq = entity.position.distance_squared(ctx.player_position);
        dist_sq > 2.0 * 2.0
    }

    fn tick(&mut self, entity: &mut Entity, ctx: &mut GoalContext) {
        let dist_sq = entity.position.distance_squared(ctx.player_position);

        // Teleport if too far (> 12 blocks)
        if dist_sq > 12.0 * 12.0 {
            // Find safe ground near player
            let px = ctx.player_position.x.floor() as i32;
            let py = ctx.player_position.y.floor() as i32;
            let pz = ctx.player_position.z.floor() as i32;

            for dx in -1..=1 {
                for dz in -1..=1 {
                    let ground = ctx.chunk_manager.get_block(px + dx, py - 1, pz + dz);
                    let spawn = ctx.chunk_manager.get_block(px + dx, py, pz + dz);
                    if ground != crate::world::BlockType::Air
                        && ground != crate::world::BlockType::Lava
                        && spawn == crate::world::BlockType::Air
                    {
                        entity.position =
                            Vec3::new((px + dx) as f32 + 0.5, py as f32, (pz + dz) as f32 + 0.5);
                        entity.velocity = Vec3::ZERO;
                        return;
                    }
                }
            }
        }

        // Walk towards player
        let dir = (ctx.player_position - entity.position).normalize_or_zero();
        let speed = match entity.entity_type {
            EntityType::Wolf => 4.5,
            EntityType::Cat => 4.0,
            _ => 3.5,
        };
        entity.velocity.x = dir.x * speed;
        entity.velocity.z = dir.z * speed;
        entity.yaw = f32::atan2(dir.x, dir.z);
    }
}

// 4. Melee Attack Goal
pub struct MeleeAttackGoal {
    pub attack_reach: f32,
    pub attack_cooldown_max: f32,
}

impl MeleeAttackGoal {
    pub fn new(reach: f32, cooldown: f32) -> Self {
        Self {
            attack_reach: reach,
            attack_cooldown_max: cooldown,
        }
    }
}

impl Goal for MeleeAttackGoal {
    fn priority(&self) -> u8 {
        5
    }

    fn can_start(&mut self, entity: &Entity, ctx: &GoalContext) -> bool {
        if let Some(target_pos) = ctx.target_entity_pos {
            let dist_sq = entity.position.distance_squared(target_pos);
            dist_sq <= 16.0 * 16.0
        } else if entity.target_player {
            let dist_sq = entity.position.distance_squared(ctx.player_position);
            dist_sq <= 16.0 * 16.0
        } else {
            false
        }
    }

    fn should_continue(&mut self, entity: &Entity, ctx: &GoalContext) -> bool {
        self.can_start(entity, ctx)
    }

    fn tick(&mut self, entity: &mut Entity, ctx: &mut GoalContext) {
        let target_pos = ctx.target_entity_pos.unwrap_or(ctx.player_position);

        let diff = target_pos - entity.position;
        let dist = diff.length();
        let dir = diff.normalize_or_zero();

        let speed = match entity.entity_type {
            EntityType::Spider => 4.5,
            EntityType::Slime | EntityType::MagmaCube => 3.0,
            EntityType::WitherSkeleton => 4.2,
            _ => 3.5,
        };

        if dist > self.attack_reach {
            entity.velocity.x = dir.x * speed;
            entity.velocity.z = dir.z * speed;
            entity.yaw = f32::atan2(dir.x, dir.z);
        } else {
            entity.velocity.x = 0.0;
            entity.velocity.z = 0.0;
        }

        // Leap attack for Spider
        if entity.entity_type == EntityType::Spider && dist < 4.0 && dist > 1.5 && entity.on_ground
        {
            entity.velocity.y = 5.0;
            entity.velocity.x = dir.x * 6.0;
            entity.velocity.z = dir.z * 6.0;
        }
    }
}

// 5. Wander Goal
pub struct WanderGoal {
    pub change_timer: f32,
    pub current_dir: Vec3,
}

impl Default for WanderGoal {
    fn default() -> Self {
        Self::new()
    }
}

impl WanderGoal {
    pub fn new() -> Self {
        Self {
            change_timer: 0.0,
            current_dir: Vec3::ZERO,
        }
    }
}

impl Goal for WanderGoal {
    fn priority(&self) -> u8 {
        10
    }

    fn can_start(&mut self, _entity: &Entity, _ctx: &GoalContext) -> bool {
        true
    }

    fn should_continue(&mut self, _entity: &Entity, _ctx: &GoalContext) -> bool {
        true
    }

    fn tick(&mut self, entity: &mut Entity, ctx: &mut GoalContext) {
        self.change_timer -= ctx.delta_time;
        if self.change_timer <= 0.0 {
            self.change_timer = 3.0 + ((entity.id % 5) as f32);
            let angle = ((entity.id.wrapping_mul(31) ^ (entity.life_time as u64)) % 360) as f32
                * std::f32::consts::PI
                / 180.0;
            self.current_dir = Vec3::new(angle.cos(), 0.0, angle.sin());
        }

        let speed = match entity.entity_type {
            EntityType::Bat => 2.0,
            EntityType::Squid => 1.5,
            _ => 1.2,
        };

        if entity.entity_type == EntityType::Bat {
            // Bats fly randomly in 3D
            entity.velocity = self.current_dir * speed
                + Vec3::new(0.0, (entity.life_time.sin() * 0.5) as f32, 0.0);
        } else if entity.entity_type == EntityType::Squid {
            // Squid swims in 3D
            entity.velocity = self.current_dir * speed;
        } else {
            entity.velocity.x = self.current_dir.x * speed;
            entity.velocity.z = self.current_dir.z * speed;
            if self.current_dir.length_squared() > 0.01 {
                entity.yaw = f32::atan2(self.current_dir.x, self.current_dir.z);
            }
        }
    }
}

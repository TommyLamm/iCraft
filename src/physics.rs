use crate::chunk_manager::ChunkManager;
use glam::Vec3;

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

pub struct PlayerPhysics {
    pub position: Vec3,
    pub velocity: Vec3,
    pub size: Vec3,
    pub on_ground: bool,
    pub highest_y: f32,
}

impl PlayerPhysics {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            size: Vec3::new(0.6, 1.8, 0.6), // Minecraft 玩家寬高
            on_ground: false,
            highest_y: position.y,
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
        // Hitbox size adjustment
        if is_sneaking {
            self.size.y = 1.5;
        } else {
            self.size.y = 1.8;
        }

        let was_on_ground = self.on_ground;

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
        let mut speed = 8.0;
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
        self.velocity.x = movement_input.x * speed;
        self.velocity.z = movement_input.z * speed;

        // 2. 套用重力與跳躍
        if is_in_water {
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
        self.position.x += self.velocity.x * dt;
        self.resolve_collisions(chunk_manager, 0);
        if is_sneaking && self.on_ground {
            if !self.is_block_below(chunk_manager) {
                self.position.x = old_x;
                self.velocity.x = 0.0;
            }
        }

        // 4. 沿 Z 軸位移並處理碰撞
        let old_z = self.position.z;
        self.position.z += self.velocity.z * dt;
        self.resolve_collisions(chunk_manager, 2);
        if is_sneaking && self.on_ground {
            if !self.is_block_below(chunk_manager) {
                self.position.z = old_z;
                self.velocity.z = 0.0;
            }
        }

        // 5. 沿 Y 軸位移並處理碰撞
        self.position.y += self.velocity.y * dt;
        self.on_ground = false;
        self.resolve_collisions(chunk_manager, 1);

        // Calculate fall damage on landing
        let mut fall_damage = 0.0;
        if !was_on_ground && self.on_ground {
            let fall_distance = self.highest_y - self.position.y;
            if fall_distance > 3.0 {
                fall_damage = fall_distance - 3.0;
            }
        }

        if self.on_ground || is_in_water || is_in_lava {
            self.highest_y = self.position.y;
        } else {
            self.highest_y = self.highest_y.max(self.position.y);
        }

        fall_damage
    }

    fn resolve_collisions(&mut self, chunk_manager: &ChunkManager, axis: usize) {
        let player_aabb = self.get_aabb();

        // 檢測玩家周圍可能相交的方塊
        let min_x = player_aabb.min.x.floor() as i32;
        let max_x = player_aabb.max.x.floor() as i32;
        let min_y =
            (player_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let max_y =
            (player_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let min_z = player_aabb.min.z.floor() as i32;
        let max_z = player_aabb.max.z.floor() as i32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let block = chunk_manager.get_block(x, y, z);
                    if block.properties().is_solid {
                        let block_aabb = AABB::new(
                            Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                            Vec3::ONE,
                        );

                        if self.get_aabb().intersects(&block_aabb) {
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

    pub fn is_block_below(&self, chunk_manager: &ChunkManager) -> bool {
        let mut check_aabb = self.get_aabb();
        check_aabb.min.y -= 0.05;
        check_aabb.max.y = self.position.y;

        let min_x = check_aabb.min.x.floor() as i32;
        let max_x = check_aabb.max.x.floor() as i32;
        let min_y =
            (check_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let max_y =
            (check_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let min_z = check_aabb.min.z.floor() as i32;
        let max_z = check_aabb.max.z.floor() as i32;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let block = chunk_manager.get_block(x, y, z);
                    if block.properties().is_solid {
                        let block_aabb = AABB::new(
                            Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                            Vec3::ONE,
                        );
                        if check_aabb.intersects(&block_aabb) {
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
    use crate::world::{BlockType, Chunk};

    #[test]
    fn test_aabb_intersection() {
        let box1 = AABB::new(Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        let box2 = AABB::new(Vec3::new(0.8, 0.0, 0.0), Vec3::ONE);
        let box3 = AABB::new(Vec3::new(1.5, 0.0, 0.0), Vec3::ONE);

        assert!(box1.intersects(&box2));
        assert!(!box1.intersects(&box3));
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
        let mut chunk_manager = ChunkManager::new(2);
        let mut chunk = Chunk::new(0, 0);
        // Clear all blocks to Air to prevent procedural terrain interference
        for x in 0..16 {
            for y in 0..256 {
                for z in 0..16 {
                    chunk.blocks[x][y][z] = BlockType::Air;
                }
            }
        }
        // Set one stone block at (8, 70, 8)
        chunk.blocks[8][70][8] = BlockType::Stone;
        chunk_manager.chunks.insert((0, 0), chunk);

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
}

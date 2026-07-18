use glam::Vec3;
use crate::world::Chunk;

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
}

impl PlayerPhysics {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            size: Vec3::new(0.6, 1.8, 0.6), // Minecraft 玩家寬高
            on_ground: false,
        }
    }

    pub fn get_aabb(&self) -> AABB {
        AABB::new(self.position + Vec3::new(0.0, self.size.y * 0.5, 0.0), self.size)
    }

    pub fn update(&mut self, dt: f32, chunk: &Chunk, movement_input: Vec3) {
        // 1. 套用玩家移動控制
        let speed = 8.0;
        self.velocity.x = movement_input.x * speed;
        self.velocity.z = movement_input.z * speed;

        // 2. 套用重力與跳躍
        if movement_input.y > 0.0 && self.on_ground {
            self.velocity.y = 10.0;
        }
        self.velocity.y -= 32.0 * dt;
        if self.velocity.y < -50.0 {
            self.velocity.y = -50.0; // 終端速度
        }

        // 3. 沿 X 軸位移並處理碰撞
        self.position.x += self.velocity.x * dt;
        self.resolve_collisions(chunk, 0);

        // 4. 沿 Z 軸位移並處理碰撞
        self.position.z += self.velocity.z * dt;
        self.resolve_collisions(chunk, 2);

        // 5. 沿 Y 軸位移並處理碰撞
        self.position.y += self.velocity.y * dt;
        self.on_ground = false;
        self.resolve_collisions(chunk, 1);
    }

    fn resolve_collisions(&mut self, chunk: &Chunk, axis: usize) {
        let player_aabb = self.get_aabb();

        // 檢測玩家周圍可能相交的方塊
        let min_x = (player_aabb.min.x.floor() as i32).max(0);
        let max_x = (player_aabb.max.x.floor() as i32).max(0);
        let min_y = (player_aabb.min.y.floor() as i32).max(0);
        let max_y = (player_aabb.max.y.floor() as i32).max(0);
        let min_z = (player_aabb.min.z.floor() as i32).max(0);
        let max_z = (player_aabb.max.z.floor() as i32).max(0);

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let block = chunk.get_block(x, y, z);
                    if block != crate::world::BlockType::Air {
                        let block_aabb = AABB::new(
                            Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                            Vec3::ONE,
                        );

                        if self.get_aabb().intersects(&block_aabb) {
                            if axis == 0 { // X 軸
                                if self.velocity.x > 0.0 {
                                    self.position.x = block_aabb.min.x - self.size.x * 0.5;
                                } else {
                                    self.position.x = block_aabb.max.x + self.size.x * 0.5;
                                }
                                self.velocity.x = 0.0;
                            } else if axis == 2 { // Z 軸
                                if self.velocity.z > 0.0 {
                                    self.position.z = block_aabb.min.z - self.size.z * 0.5;
                                } else {
                                    self.position.z = block_aabb.max.z + self.size.z * 0.5;
                                }
                                self.velocity.z = 0.0;
                            } else if axis == 1 { // Y 軸
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aabb_intersection() {
        let box1 = AABB::new(Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        let box2 = AABB::new(Vec3::new(0.8, 0.0, 0.0), Vec3::ONE);
        let box3 = AABB::new(Vec3::new(1.5, 0.0, 0.0), Vec3::ONE);

        assert!(box1.intersects(&box2));
        assert!(!box1.intersects(&box3));
    }
}

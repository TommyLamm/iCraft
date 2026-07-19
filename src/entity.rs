use glam::Vec3;
use crate::physics::AABB;
use crate::chunk_manager::ChunkManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    Zombie,
    Skeleton,
    Creeper,
    Arrow,
    Pig,
    Cow,
    Sheep,
    Chicken,
    HeartParticle,
}

pub struct Entity {
    pub id: u64,
    pub entity_type: EntityType,
    
    // Physics & movement
    pub position: Vec3,
    pub velocity: Vec3,
    pub size: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    
    // Mob properties
    pub health: f32,
    pub max_health: f32,
    pub target_player: bool,
    pub action_cooldown: f32,
    pub is_ignited: bool,
    pub burn_timer: f32,
    pub invulnerable_time: f32,

    // Passive mob fields
    pub age: f32,
    pub breeding_timer: f32,
    pub breed_cooldown: f32,
    pub has_wool: bool,
    pub wool_color: [f32; 3],
    pub grass_eat_timer: f32,
    pub egg_lay_timer: f32,
    pub life_time: f32,
}

impl Entity {
    pub fn new(id: u64, entity_type: EntityType, position: Vec3) -> Self {
        let size = match entity_type {
            EntityType::Zombie | EntityType::Skeleton => Vec3::new(0.6, 1.8, 0.6),
            EntityType::Creeper => Vec3::new(0.6, 1.7, 0.6),
            EntityType::Arrow => Vec3::new(0.15, 0.15, 0.15),
            EntityType::Pig => Vec3::new(0.9, 0.9, 0.9),
            EntityType::Cow => Vec3::new(0.9, 1.4, 0.9),
            EntityType::Sheep => Vec3::new(0.9, 1.3, 0.9),
            EntityType::Chicken => Vec3::new(0.4, 0.7, 0.4),
            EntityType::HeartParticle => Vec3::new(0.25, 0.25, 0.25),
        };
        let max_health = match entity_type {
            EntityType::Zombie | EntityType::Skeleton | EntityType::Creeper => 20.0,
            EntityType::Pig => 10.0,
            EntityType::Cow => 10.0,
            EntityType::Sheep => 8.0,
            EntityType::Chicken => 4.0,
            EntityType::Arrow | EntityType::HeartParticle => 0.0,
        };
        Self {
            id,
            entity_type,
            position,
            velocity: Vec3::ZERO,
            size,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            health: max_health,
            max_health,
            target_player: false,
            action_cooldown: 0.0,
            is_ignited: false,
            burn_timer: 0.0,
            invulnerable_time: 0.0,
            age: 0.0,
            breeding_timer: 0.0,
            breed_cooldown: 0.0,
            has_wool: true,
            wool_color: [1.0, 1.0, 1.0],
            grass_eat_timer: 0.0,
            egg_lay_timer: 300.0 + (id % 300) as f32,
            life_time: 1.5,
        }
    }

    pub fn get_aabb(&self) -> AABB {
        // Foot-based position
        AABB::new(self.position + Vec3::new(0.0, self.size.y * 0.5, 0.0), self.size)
    }

    pub fn update_physics(&mut self, dt: f32, chunk_manager: &ChunkManager) {
        if self.entity_type == EntityType::HeartParticle {
            self.position += self.velocity * dt;
            return;
        }
        if self.entity_type == EntityType::Arrow {
            // Arrow physics: gravity only, no horizontal deceleration
            self.velocity.y -= 12.0 * dt;
            self.position += self.velocity * dt;
            
            // Align orientation to velocity
            let dir = self.velocity.normalize_or_zero();
            self.yaw = f32::atan2(-dir.x, -dir.z);
            self.pitch = f32::asin(dir.y);
            return;
        }

        // Apply gravity
        let gravity = if self.entity_type == EntityType::Chicken && self.velocity.y < 0.0 {
            8.0 // slow glide
        } else {
            32.0
        };
        
        self.velocity.y -= gravity * dt;
        
        let terminal_vel = if self.entity_type == EntityType::Chicken {
            -2.0
        } else {
            -50.0
        };
        if self.velocity.y < terminal_vel {
            self.velocity.y = terminal_vel;
        }

        // Move X
        self.position.x += self.velocity.x * dt;
        self.resolve_collisions(chunk_manager, 0);

        // Move Z
        self.position.z += self.velocity.z * dt;
        self.resolve_collisions(chunk_manager, 2);

        // Move Y
        self.position.y += self.velocity.y * dt;
        self.on_ground = false;
        self.resolve_collisions(chunk_manager, 1);

        // Friction / Deceleration (simulate ground/air drag)
        let friction = if self.on_ground { 0.6 } else { 0.9 };
        self.velocity.x *= friction;
        self.velocity.z *= friction;
    }

    fn resolve_collisions(&mut self, chunk_manager: &ChunkManager, axis: usize) {
        let entity_aabb = self.get_aabb();
        let min_x = entity_aabb.min.x.floor() as i32;
        let max_x = entity_aabb.max.x.floor() as i32;
        let min_y = (entity_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let max_y = (entity_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let min_z = entity_aabb.min.z.floor() as i32;
        let max_z = entity_aabb.max.z.floor() as i32;

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
                                if self.velocity.x > 0.0 {
                                    self.position.x = block_aabb.min.x - self.size.x * 0.5;
                                } else {
                                    self.position.x = block_aabb.max.x + self.size.x * 0.5;
                                }
                                self.velocity.x = 0.0;
                            } else if axis == 2 {
                                if self.velocity.z > 0.0 {
                                    self.position.z = block_aabb.min.z - self.size.z * 0.5;
                                } else {
                                    self.position.z = block_aabb.max.z + self.size.z * 0.5;
                                }
                                self.velocity.z = 0.0;
                            } else if axis == 1 {
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

pub struct EntityManager {
    pub entities: Vec<Entity>,
    next_id: u64,
}

impl EntityManager {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            next_id: 1,
        }
    }

    pub fn spawn(&mut self, entity_type: EntityType, pos: Vec3) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entities.push(Entity::new(id, entity_type, pos));
        id
    }
}

pub fn ray_intersects_aabb(origin: Vec3, dir: Vec3, aabb: &AABB) -> Option<f32> {
    let mut tmin = (aabb.min.x - origin.x) / dir.x;
    let mut tmax = (aabb.max.x - origin.x) / dir.x;
    if tmin > tmax { std::mem::swap(&mut tmin, &mut tmax); }

    let mut tymin = (aabb.min.y - origin.y) / dir.y;
    let mut tymax = (aabb.max.y - origin.y) / dir.y;
    if tymin > tymax { std::mem::swap(&mut tymin, &mut tymax); }

    if tmin > tymax || tymin > tmax {
        return None;
    }
    if tymin > tmin { tmin = tymin; }
    if tymax < tmax { tmax = tymax; }

    let mut tzmin = (aabb.min.z - origin.z) / dir.z;
    let mut tzmax = (aabb.max.z - origin.z) / dir.z;
    if tzmin > tzmax { std::mem::swap(&mut tzmin, &mut tzmax); }

    if tmin > tzmax || tzmin > tmax {
        return None;
    }
    if tzmin > tmin { tmin = tzmin; }
    if tzmax < tmax { tmax = tzmax; }

    if tmax >= 0.0 {
        Some(tmin.max(0.0))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ray_aabb_intersection() {
        let box_pos = Vec3::new(0.0, 0.0, 0.0);
        let aabb = AABB::new(box_pos, Vec3::ONE);
        
        // Ray pointing straight at the center of the box from Z=-3
        let ray_origin = Vec3::new(0.0, 0.0, -3.0);
        let ray_dir = Vec3::new(0.0, 0.0, 1.0);
        let hit = ray_intersects_aabb(ray_origin, ray_dir, &aabb);
        assert!(hit.is_some());
        assert!((hit.unwrap() - 2.5).abs() < 1e-5); // intersection point should be Z=-0.5, so dist = 2.5

        // Ray pointing away
        let ray_dir_away = Vec3::new(0.0, 0.0, -1.0);
        assert!(ray_intersects_aabb(ray_origin, ray_dir_away, &aabb).is_none());
    }

    #[test]
    fn test_chicken_slow_fall() {
        let mut chicken = Entity::new(1, EntityType::Chicken, Vec3::new(0.0, 10.0, 0.0));
        chicken.velocity.y = -10.0;
        let chunk_manager = ChunkManager::new(4);
        chicken.update_physics(0.1, &chunk_manager);
        assert!(chicken.velocity.y >= -2.01 && chicken.velocity.y <= -1.99);
    }
}

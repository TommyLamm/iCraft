use crate::chunk_manager::ChunkManager;
use crate::physics::AABB;
use glam::Vec3;

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
    DroppedItem,
    SplashPotion,
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
    pub fire_aspect_timer: f32,
    pub burn_damage_timer: f32,
    pub invulnerable_time: f32,
    pub friendly_projectile: bool,
    pub projectile_damage: f32,
    pub potion: Option<crate::brewing::PotionData>,

    // Passive mob fields
    pub age: f32,
    pub breeding_timer: f32,
    pub breed_cooldown: f32,
    pub has_wool: bool,
    pub wool_color: [f32; 3],
    pub grass_eat_timer: f32,
    pub egg_lay_timer: f32,
    pub life_time: f32,

    // DroppedItem fields
    pub dropped_item: Option<crate::inventory::Item>,
    pub pickup_cooldown: f32,
}

impl Entity {
    pub fn new(id: u64, entity_type: EntityType, position: Vec3) -> Self {
        let size = match entity_type {
            EntityType::Zombie | EntityType::Skeleton => Vec3::new(0.6, 1.8, 0.6),
            EntityType::Creeper => Vec3::new(0.6, 1.7, 0.6),
            EntityType::Arrow | EntityType::SplashPotion => Vec3::new(0.15, 0.15, 0.15),
            EntityType::Pig => Vec3::new(0.9, 0.9, 0.9),
            EntityType::Cow => Vec3::new(0.9, 1.4, 0.9),
            EntityType::Sheep => Vec3::new(0.9, 1.3, 0.9),
            EntityType::Chicken => Vec3::new(0.4, 0.7, 0.4),
            EntityType::HeartParticle => Vec3::new(0.25, 0.25, 0.25),
            EntityType::DroppedItem => Vec3::new(0.25, 0.25, 0.25),
        };
        let max_health = match entity_type {
            EntityType::Zombie | EntityType::Skeleton | EntityType::Creeper => 20.0,
            EntityType::Pig => 10.0,
            EntityType::Cow => 10.0,
            EntityType::Sheep => 8.0,
            EntityType::Chicken => 4.0,
            EntityType::Arrow
            | EntityType::SplashPotion
            | EntityType::HeartParticle
            | EntityType::DroppedItem => 0.0,
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
            fire_aspect_timer: 0.0,
            burn_damage_timer: 0.0,
            invulnerable_time: 0.0,
            friendly_projectile: false,
            projectile_damage: 4.0,
            potion: None,
            age: 0.0,
            breeding_timer: 0.0,
            breed_cooldown: 0.0,
            has_wool: true,
            wool_color: [1.0, 1.0, 1.0],
            grass_eat_timer: 0.0,
            egg_lay_timer: 300.0 + (id % 300) as f32,
            life_time: 1.5,
            dropped_item: None,
            pickup_cooldown: 0.0,
        }
    }

    pub fn get_aabb(&self) -> AABB {
        // Foot-based position
        AABB::new(
            self.position + Vec3::new(0.0, self.size.y * 0.5, 0.0),
            self.size,
        )
    }

    pub fn update_physics(&mut self, dt: f32, chunk_manager: &ChunkManager) {
        if self.entity_type == EntityType::HeartParticle {
            self.position += self.velocity * dt;
            return;
        }
        if self.entity_type == EntityType::Arrow || self.entity_type == EntityType::SplashPotion {
            // Arrow physics: gravity only, no horizontal deceleration
            self.velocity.y -= 12.0 * dt;
            self.position += self.velocity * dt;

            // Align orientation to velocity
            let dir = self.velocity.normalize_or_zero();
            self.yaw = f32::atan2(-dir.x, -dir.z);
            self.pitch = f32::asin(dir.y);
            return;
        }

        // Dropped items count down their pickup-cooldown so freshly-dropped
        // stacks can't be instantly re-collected by the breaker.
        if self.entity_type == EntityType::DroppedItem && self.pickup_cooldown > 0.0 {
            self.pickup_cooldown = (self.pickup_cooldown - dt).max(0.0);
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
        let min_y =
            (entity_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let max_y =
            (entity_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
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
    if tmin > tmax {
        std::mem::swap(&mut tmin, &mut tmax);
    }

    let mut tymin = (aabb.min.y - origin.y) / dir.y;
    let mut tymax = (aabb.max.y - origin.y) / dir.y;
    if tymin > tymax {
        std::mem::swap(&mut tymin, &mut tymax);
    }

    if tmin > tymax || tymin > tmax {
        return None;
    }
    if tymin > tmin {
        tmin = tymin;
    }
    if tymax < tmax {
        tmax = tymax;
    }

    let mut tzmin = (aabb.min.z - origin.z) / dir.z;
    let mut tzmax = (aabb.max.z - origin.z) / dir.z;
    if tzmin > tzmax {
        std::mem::swap(&mut tzmin, &mut tzmax);
    }

    if tmin > tzmax || tzmin > tmax {
        return None;
    }
    if tzmin > tmin {
        tmin = tzmin;
    }
    if tzmax < tmax {
        tmax = tzmax;
    }

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

    #[test]
    fn dropped_item_falls_with_gravity() {
        let mut item = Entity::new(2, EntityType::DroppedItem, Vec3::new(0.5, 20.0, 0.5));
        item.dropped_item = Some(crate::inventory::Item::Stone);
        let chunk_manager = ChunkManager::new(4);
        // No solid block below within the chunk; gravity should pull it down.
        item.update_physics(0.5, &chunk_manager);
        assert!(
            item.velocity.y < 0.0,
            "dropped item should be falling under gravity"
        );
        assert!(
            item.position.y < 20.0,
            "dropped item should have moved downward"
        );
    }

    #[test]
    fn dropped_item_lands_on_solid_block() {
        // Build a chunk manager with a single solid stone block at world
        // (0, 10, 0). We start from a generated chunk but clear it so the only
        // solid block is our test floor.
        let mut chunk_manager = ChunkManager::new(4);
        let _ = chunk_manager.chunks.insert((0, 0), {
            let mut c = crate::world::Chunk::new(0, 0);
            for x in 0..crate::world::CHUNK_WIDTH {
                for y in 0..crate::world::CHUNK_HEIGHT {
                    for z in 0..crate::world::CHUNK_DEPTH {
                        c.blocks[x][y][z] = crate::world::BlockType::Air;
                    }
                }
            }
            // Place a 2x2 stone floor at y=10 covering the item's footprint.
            for fx in 0..2 {
                for fz in 0..2 {
                    c.blocks[fx][10][fz] = crate::world::BlockType::Stone;
                }
            }
            c
        });
        let mut item = Entity::new(3, EntityType::DroppedItem, Vec3::new(0.5, 12.0, 0.5));
        item.dropped_item = Some(crate::inventory::Item::Stone);
        // Simulate several physics steps so the item falls onto the floor.
        for _ in 0..400 {
            item.update_physics(0.05, &chunk_manager);
        }
        assert!(
            item.on_ground,
            "dropped item should come to rest on the solid block"
        );
        // Entity is foot-positioned; on landing it should sit at the top of the
        // block (y=11) since block AABB spans [10, 11].
        assert!(
            item.position.y >= 10.9 && item.position.y <= 11.1,
            "dropped item should rest on top of y=10 (got y={})",
            item.position.y
        );
    }

    #[test]
    fn dropped_item_pickup_cooldown_decreases() {
        let mut item = Entity::new(4, EntityType::DroppedItem, Vec3::new(0.5, 20.0, 0.5));
        item.pickup_cooldown = 0.5;
        let chunk_manager = ChunkManager::new(4);
        item.update_physics(0.3, &chunk_manager);
        assert!(
            (item.pickup_cooldown - 0.2).abs() < 1e-4,
            "pickup cooldown should decrement by dt"
        );
    }

    #[test]
    fn dropped_item_collection_adds_to_inventory() {
        // Standalone simulation of the collection logic: a player within 1.5m
        // of a DroppedItem should pick it up into their inventory.
        let mut em = EntityManager::new();
        let id = em.spawn(EntityType::DroppedItem, Vec3::new(0.0, 0.0, 0.0));
        {
            let e = em.entities.last_mut().unwrap();
            e.dropped_item = Some(crate::inventory::Item::Dirt);
            e.pickup_cooldown = 0.0;
        }
        let player_pos = Vec3::new(0.5, 0.0, 0.0); // within 1.5m
        assert_eq!(id, 1);

        // Manual collection (mirrors State::update logic).
        let mut inventory = crate::inventory::Inventory::new();
        let mut to_collect: Vec<usize> = Vec::new();
        for (i, e) in em.entities.iter().enumerate() {
            if e.entity_type != EntityType::DroppedItem {
                continue;
            }
            if e.pickup_cooldown > 0.0 {
                continue;
            }
            if e.dropped_item.is_none() {
                continue;
            }
            if e.position.distance(player_pos) < 1.5 {
                to_collect.push(i);
            }
        }
        for &i in to_collect.iter().rev() {
            let item = em.entities[i].dropped_item;
            if let Some(item) = item {
                let added = inventory.add_item(item);
                if added {
                    em.entities.remove(i);
                }
            }
        }

        assert!(
            em.entities.is_empty(),
            "DroppedItem entity should be despawned after collection"
        );
        assert!(
            inventory
                .hotbar
                .iter()
                .any(|s| s.map(|s| s.item).unwrap_or(crate::inventory::Item::Air)
                    == crate::inventory::Item::Dirt),
            "Dirt should have been added to the inventory"
        );
    }
}

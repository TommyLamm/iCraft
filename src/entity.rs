use crate::chunk_manager::ChunkManager;
use crate::physics::AABB;
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    Blaze,
    Piglin,
    Husk,
    Shulker,
    EnderDragon,
    Wither,
    EndCrystal,
    WitherSkull,
    DragonBreath,
    RemotePlayer,
    Enderman,
    ExperienceOrb,
}

impl EntityType {
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::Zombie => 0,
            Self::Skeleton => 1,
            Self::Creeper => 2,
            Self::Arrow => 3,
            Self::Pig => 4,
            Self::Cow => 5,
            Self::Sheep => 6,
            Self::Chicken => 7,
            Self::HeartParticle => 8,
            Self::DroppedItem => 9,
            Self::SplashPotion => 10,
            Self::Blaze => 11,
            Self::Piglin => 12,
            Self::Husk => 13,
            Self::Shulker => 14,
            Self::EnderDragon => 15,
            Self::Wither => 16,
            Self::EndCrystal => 17,
            Self::WitherSkull => 18,
            Self::DragonBreath => 19,
            Self::RemotePlayer => 20,
            Self::Enderman => 21,
            Self::ExperienceOrb => 22,
        }
    }

    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Zombie),
            1 => Some(Self::Skeleton),
            2 => Some(Self::Creeper),
            3 => Some(Self::Arrow),
            4 => Some(Self::Pig),
            5 => Some(Self::Cow),
            6 => Some(Self::Sheep),
            7 => Some(Self::Chicken),
            8 => Some(Self::HeartParticle),
            9 => Some(Self::DroppedItem),
            10 => Some(Self::SplashPotion),
            11 => Some(Self::Blaze),
            12 => Some(Self::Piglin),
            13 => Some(Self::Husk),
            14 => Some(Self::Shulker),
            15 => Some(Self::EnderDragon),
            16 => Some(Self::Wither),
            17 => Some(Self::EndCrystal),
            18 => Some(Self::WitherSkull),
            19 => Some(Self::DragonBreath),
            20 => Some(Self::RemotePlayer),
            21 => Some(Self::Enderman),
            22 => Some(Self::ExperienceOrb),
            _ => None,
        }
    }

    pub fn is_passive(self) -> bool {
        matches!(self, Self::Pig | Self::Cow | Self::Sheep | Self::Chicken)
    }

    pub fn is_hostile(self) -> bool {
        matches!(
            self,
            Self::Zombie
                | Self::Skeleton
                | Self::Creeper
                | Self::Blaze
                | Self::Piglin
                | Self::Husk
                | Self::Shulker
                | Self::Enderman
                | Self::EnderDragon
                | Self::Wither
        )
    }

    pub fn is_living(self) -> bool {
        self.is_passive() || self.is_hostile()
    }

    pub fn is_projectile(self) -> bool {
        matches!(
            self,
            Self::Arrow | Self::SplashPotion | Self::WitherSkull | Self::DragonBreath
        )
    }

    pub fn uses_standard_player_kill_rewards(self) -> bool {
        matches!(
            self,
            Self::Zombie
                | Self::Skeleton
                | Self::Creeper
                | Self::Pig
                | Self::Cow
                | Self::Sheep
                | Self::Chicken
        )
    }

    pub fn is_boss(self) -> bool {
        matches!(self, Self::EnderDragon | Self::Wither)
    }

    pub fn is_persistent(self) -> bool {
        self.is_boss() || matches!(self, Self::EndCrystal | Self::Shulker)
    }

    pub fn uses_flying_physics(self) -> bool {
        matches!(
            self,
            Self::Blaze | Self::EnderDragon | Self::Wither | Self::WitherSkull | Self::DragonBreath
        )
    }

    pub fn is_anchored(self) -> bool {
        matches!(self, Self::Shulker | Self::EndCrystal)
    }

    pub fn boss_name(self) -> Option<&'static str> {
        match self {
            Self::EnderDragon => Some("ENDER DRAGON"),
            Self::Wither => Some("WITHER"),
            _ => None,
        }
    }
}

pub struct Entity {
    pub id: u64,
    pub entity_type: EntityType,
    /// For RemotePlayer entities, the network player id; zero for local/mob entities.
    pub player_id: u64,
    /// Username shown for RemotePlayer entities. String allocation is intentional because
    /// remote roster names are network-owned and Entity values are relatively few.
    pub username: String,

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
    pub player_kill_rewarded: bool,
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
    /// How many items of `dropped_item` this entity carries. Block-break and
    /// mob drops stay at 1; player throws of a whole stack use a single
    /// entity with a larger count.
    pub dropped_count: u32,
    pub dropped_stack: Option<crate::inventory::ItemStack>,
    /// Cumulative loaded time (seconds) since spawned. Used for 5 min (300s) despawn.
    pub item_age: f32,
    /// Experience value for ExperienceOrb entities.
    pub xp_value: u32,
    pub pickup_cooldown: f32,
    pub ai_phase: u8,
    pub ai_timer: f32,
    /// Continuous time the local player has kept this Enderman's head in view.
    pub enderman_gaze_timer: f32,
}

impl Entity {
    pub fn new(id: u64, entity_type: EntityType, position: Vec3) -> Self {
        let size = match entity_type {
            EntityType::Zombie | EntityType::Skeleton | EntityType::Blaze => {
                Vec3::new(0.6, 1.8, 0.6)
            }
            EntityType::Piglin | EntityType::Husk => Vec3::new(0.6, 1.95, 0.6),
            EntityType::Creeper => Vec3::new(0.6, 1.7, 0.6),
            EntityType::Arrow
            | EntityType::SplashPotion
            | EntityType::WitherSkull
            | EntityType::DragonBreath => Vec3::new(0.25, 0.25, 0.25),
            EntityType::Pig => Vec3::new(0.9, 0.9, 0.9),
            EntityType::Cow => Vec3::new(0.9, 1.4, 0.9),
            EntityType::Sheep => Vec3::new(0.9, 1.3, 0.9),
            EntityType::Chicken => Vec3::new(0.4, 0.7, 0.4),
            EntityType::HeartParticle => Vec3::new(0.25, 0.25, 0.25),
            EntityType::DroppedItem | EntityType::ExperienceOrb => Vec3::new(0.25, 0.25, 0.25),
            EntityType::Shulker => Vec3::ONE,
            EntityType::Enderman => Vec3::new(0.6, 2.9, 0.6),
            EntityType::EnderDragon => Vec3::new(8.0, 4.0, 8.0),
            EntityType::Wither => Vec3::new(1.0, 3.5, 1.0),
            EntityType::EndCrystal => Vec3::new(1.5, 2.0, 1.5),
            EntityType::RemotePlayer => Vec3::new(0.6, 1.8, 0.6),
        };
        let max_health = match entity_type {
            EntityType::Zombie | EntityType::Skeleton | EntityType::Creeper => 20.0,
            EntityType::Blaze => 20.0,
            EntityType::Piglin => 16.0,
            EntityType::Husk => 20.0,
            EntityType::Shulker => 30.0,
            EntityType::Enderman => 40.0,
            EntityType::EnderDragon => 200.0,
            EntityType::Wither => 300.0,
            EntityType::EndCrystal => 5.0,
            EntityType::Pig => 10.0,
            EntityType::Cow => 10.0,
            EntityType::Sheep => 8.0,
            EntityType::Chicken => 4.0,
            EntityType::Arrow
            | EntityType::SplashPotion
            | EntityType::WitherSkull
            | EntityType::DragonBreath
            | EntityType::HeartParticle
            | EntityType::DroppedItem
            | EntityType::ExperienceOrb => 0.0,
            EntityType::RemotePlayer => 20.0,
        };
        Self {
            id,
            entity_type,
            player_id: 0,
            username: String::new(),
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
            player_kill_rewarded: false,
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
            dropped_count: 1,
            dropped_stack: None,
            item_age: 0.0,
            xp_value: 0,
            pickup_cooldown: 0.0,
            ai_phase: 0,
            ai_timer: 0.0,
            enderman_gaze_timer: 0.0,
        }
    }

    pub fn get_aabb(&self) -> AABB {
        // Foot-based position
        AABB::new(
            self.position + Vec3::new(0.0, self.size.y * 0.5, 0.0),
            self.size,
        )
    }

    pub fn is_local_living_target(&self) -> bool {
        self.entity_type.is_living() && self.health > 0.0
    }

    pub fn is_player_melee_target(&self) -> bool {
        self.health > 0.0
            && (self.entity_type.is_living() || self.entity_type == EntityType::EndCrystal)
    }

    /// Player arrows can destroy End Crystals, but status potions only target living entities.
    pub fn is_player_projectile_target(&self) -> bool {
        self.health > 0.0
            && (self.entity_type.is_living() || self.entity_type == EntityType::EndCrystal)
    }

    pub fn update_physics(&mut self, dt: f32, chunk_manager: &ChunkManager) {
        if self.entity_type == EntityType::HeartParticle {
            self.position += self.velocity * dt;
            return;
        }
        if self.entity_type.is_anchored() {
            self.velocity = Vec3::ZERO;
            return;
        }
        if self.entity_type.uses_flying_physics() {
            self.position += self.velocity * dt;
            if self.velocity.length_squared() > 0.0001 {
                let dir = self.velocity.normalize_or_zero();
                self.yaw = f32::atan2(dir.x, dir.z);
                self.pitch = f32::asin(dir.y.clamp(-1.0, 1.0));
            }
            return;
        }
        if self.entity_type == EntityType::Arrow || self.entity_type == EntityType::SplashPotion {
            // Arrow physics: gravity only, no horizontal deceleration
            self.velocity.y -= 12.0 * dt;
            self.position += self.velocity * dt;

            // Align orientation to velocity
            let dir = self.velocity.normalize_or_zero();
            self.yaw = f32::atan2(dir.x, dir.z);
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

use std::collections::HashMap;

#[derive(Default)]
#[allow(dead_code)]
pub struct EntityScratch {
    pub arrows_to_spawn: Vec<(Vec3, Vec3)>,
    pub explosions: Vec<Vec3>,
    pub blocks_removed: Vec<(i32, i32, i32)>,
    pub items_to_drop: Vec<(crate::inventory::Item, Vec3)>,
    pub death_sounds: Vec<Vec3>,
    pub hearts_to_spawn: Vec<Vec3>,
    pub baby_mobs_to_spawn: Vec<(EntityType, Vec3)>,
    pub id_list: Vec<u64>,
    pub usize_list: Vec<usize>,
}

/// Classification used by the R5.9 query audit.  Global simulation and
/// cleanup passes intentionally visit every entity; candidate selection must
/// instead go through the type/spatial indexes below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityIterationKind {
    GlobalSimulation,
    GlobalCleanup,
    CandidateQuery,
}

pub const fn is_global_entity_maintenance(kind: EntityIterationKind) -> bool {
    matches!(
        kind,
        EntityIterationKind::GlobalSimulation | EntityIterationKind::GlobalCleanup
    )
}

impl EntityScratch {
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.arrows_to_spawn.clear();
        self.explosions.clear();
        self.blocks_removed.clear();
        self.items_to_drop.clear();
        self.death_sounds.clear();
        self.hearts_to_spawn.clear();
        self.baby_mobs_to_spawn.clear();
        self.id_list.clear();
        self.usize_list.clear();
    }
}

pub struct EntityManager {
    pub entities: Vec<Entity>,
    pub id_to_index: HashMap<u64, usize>,
    pub type_buckets: HashMap<EntityType, Vec<u64>>,
    pub spatial_buckets: HashMap<(i32, i32), Vec<u64>>,
    /// Last bucket recorded for each entity. This lets position changes move a
    /// single id between buckets without rebuilding the whole index.
    entity_chunks: HashMap<u64, (i32, i32)>,
    #[allow(dead_code)]
    pub scratch: EntityScratch,
    next_id: u64,
    #[cfg(test)]
    position_sync_visits: u64,
}

impl EntityManager {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            id_to_index: HashMap::new(),
            type_buckets: HashMap::new(),
            spatial_buckets: HashMap::new(),
            entity_chunks: HashMap::new(),
            scratch: EntityScratch::default(),
            next_id: 1,
            #[cfg(test)]
            position_sync_visits: 0,
        }
    }

    pub fn rebuild_indexes(&mut self) {
        self.id_to_index.clear();
        self.entity_chunks.clear();
        self.type_buckets.clear();
        self.spatial_buckets.clear();

        for (idx, entity) in self.entities.iter().enumerate() {
            self.id_to_index.insert(entity.id, idx);
            self.type_buckets
                .entry(entity.entity_type)
                .or_default()
                .push(entity.id);
            let chunk_pos = (
                (entity.position.x / 16.0).floor() as i32,
                (entity.position.z / 16.0).floor() as i32,
            );
            self.spatial_buckets
                .entry(chunk_pos)
                .or_default()
                .push(entity.id);
            self.entity_chunks.insert(entity.id, chunk_pos);
        }
    }

    fn chunk_for(position: Vec3) -> (i32, i32) {
        (
            (position.x / 16.0).floor() as i32,
            (position.z / 16.0).floor() as i32,
        )
    }

    /// Synchronize one entity after its position has been changed in place.
    pub fn sync_entity_position(&mut self, id: u64) {
        #[cfg(test)]
        {
            self.position_sync_visits += 1;
        }
        let Some(&idx) = self.id_to_index.get(&id) else {
            return;
        };
        let Some(entity) = self.entities.get(idx) else {
            return;
        };
        let new_chunk = Self::chunk_for(entity.position);
        let old_chunk = self.entity_chunks.get(&id).copied();
        if old_chunk == Some(new_chunk) {
            return;
        }
        if let Some(old) = old_chunk {
            if let Some(bucket) = self.spatial_buckets.get_mut(&old) {
                bucket.retain(|&candidate| candidate != id);
            }
            if self.spatial_buckets.get(&old).is_some_and(Vec::is_empty) {
                self.spatial_buckets.remove(&old);
            }
        }
        self.spatial_buckets.entry(new_chunk).or_default().push(id);
        self.entity_chunks.insert(id, new_chunk);
    }

    /// Synchronize only entities whose positions may have changed.
    pub fn sync_entity_positions(&mut self, moved_ids: &[u64]) {
        for &id in moved_ids {
            self.sync_entity_position(id);
        }
    }

    /// Synchronize all positions using the tracked bucket map. Prefer
    /// `sync_entity_positions` when the caller already knows which ids moved.
    pub fn sync_positions(&mut self) {
        let mut ids = std::mem::take(&mut self.scratch.id_list);
        ids.clear();
        ids.extend(self.entities.iter().map(|entity| entity.id));
        for &id in &ids {
            self.sync_entity_position(id);
        }
        self.scratch.id_list = ids;
    }

    /// Compatibility shim for callers outside the entity runtime. New code
    /// should use `sync_entity_positions` when moved ids are available.
    #[deprecated(note = "use sync_entity_positions when moved ids are available")]
    pub fn update_spatial_indexes(&mut self) {
        self.sync_positions();
    }

    pub fn get_by_id(&self, id: u64) -> Option<&Entity> {
        self.id_to_index
            .get(&id)
            .and_then(|&idx| self.entities.get(idx))
    }

    pub fn get_by_id_mut(&mut self, id: u64) -> Option<&mut Entity> {
        if let Some(&idx) = self.id_to_index.get(&id) {
            self.entities.get_mut(idx)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn get_index_by_id(&self, id: u64) -> Option<usize> {
        self.id_to_index.get(&id).copied()
    }

    pub fn spawn(&mut self, entity_type: EntityType, pos: Vec3) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let idx = self.entities.len();
        let entity = Entity::new(id, entity_type, pos);
        self.entities.push(entity);
        self.id_to_index.insert(id, idx);
        self.type_buckets.entry(entity_type).or_default().push(id);
        let chunk_pos = Self::chunk_for(pos);
        self.spatial_buckets.entry(chunk_pos).or_default().push(id);
        self.entity_chunks.insert(id, chunk_pos);
        id
    }

    pub fn add_restored_entity(&mut self, data: &crate::save::EntitySaveData) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let idx = self.entities.len();
        let entity = data.to_entity(id);
        let entity_type = entity.entity_type;
        let pos = entity.position;
        self.entities.push(entity);
        self.id_to_index.insert(id, idx);
        self.type_buckets.entry(entity_type).or_default().push(id);
        let chunk_pos = Self::chunk_for(pos);
        self.spatial_buckets.entry(chunk_pos).or_default().push(id);
        self.entity_chunks.insert(id, chunk_pos);
        id
    }

    pub fn swap_remove(&mut self, idx: usize) -> Entity {
        let removed = self.entities.swap_remove(idx);
        self.id_to_index.remove(&removed.id);
        if idx < self.entities.len() {
            let swapped_id = self.entities[idx].id;
            self.id_to_index.insert(swapped_id, idx);
        }
        // Incremental bucket update: remove the entity from type and spatial buckets.
        if let Some(bucket) = self.type_buckets.get_mut(&removed.entity_type) {
            bucket.retain(|&id| id != removed.id);
        }
        if self
            .type_buckets
            .get(&removed.entity_type)
            .is_some_and(Vec::is_empty)
        {
            self.type_buckets.remove(&removed.entity_type);
        }
        let removed_chunk_pos = self
            .entity_chunks
            .remove(&removed.id)
            .unwrap_or_else(|| Self::chunk_for(removed.position));
        if let Some(bucket) = self.spatial_buckets.get_mut(&removed_chunk_pos) {
            bucket.retain(|&id| id != removed.id);
        }
        if self
            .spatial_buckets
            .get(&removed_chunk_pos)
            .is_some_and(Vec::is_empty)
        {
            self.spatial_buckets.remove(&removed_chunk_pos);
        }
        removed
    }

    pub fn remove_by_id(&mut self, id: u64) -> Option<Entity> {
        if let Some(&idx) = self.id_to_index.get(&id) {
            Some(self.swap_remove(idx))
        } else {
            None
        }
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&Entity) -> bool,
    {
        let mut index = 0;
        while index < self.entities.len() {
            if f(&self.entities[index]) {
                index += 1;
            } else {
                self.swap_remove(index);
            }
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.entities.clear();
        self.id_to_index.clear();
        self.type_buckets.clear();
        self.spatial_buckets.clear();
        self.entity_chunks.clear();
    }

    pub fn count_passive(&self) -> usize {
        self.type_buckets
            .iter()
            .filter(|(t, _)| t.is_passive())
            .map(|(_, v)| v.len())
            .sum()
    }

    pub fn count_hostile(&self) -> usize {
        self.type_buckets
            .iter()
            .filter(|(t, _)| t.is_hostile())
            .map(|(_, v)| v.len())
            .sum()
    }

    #[allow(dead_code)]
    pub fn get_entities_by_type(&self, entity_type: EntityType) -> impl Iterator<Item = &Entity> {
        self.type_buckets
            .get(&entity_type)
            .into_iter()
            .flatten()
            .filter_map(|id| self.get_by_id(*id))
    }

    #[allow(dead_code)]
    pub fn get_entities_in_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> impl Iterator<Item = &Entity> {
        self.spatial_buckets
            .get(&(chunk_x, chunk_z))
            .into_iter()
            .flatten()
            .filter_map(|id| self.get_by_id(*id))
    }

    /// Return entities in a horizontal radius using the spatial buckets. The
    /// squared-distance test keeps callers from doing a full entity scan.
    pub fn query_radius(&self, center: Vec3, radius: f32) -> impl Iterator<Item = &Entity> {
        debug_assert!(!is_global_entity_maintenance(
            EntityIterationKind::CandidateQuery
        ));
        let radius = radius.max(0.0);
        let radius_sq = radius * radius;
        let min_x = ((center.x - radius) / 16.0).floor() as i32;
        let max_x = ((center.x + radius) / 16.0).floor() as i32;
        let min_z = ((center.z - radius) / 16.0).floor() as i32;
        let max_z = ((center.z + radius) / 16.0).floor() as i32;
        (min_x..=max_x)
            .flat_map(move |x| {
                (min_z..=max_z)
                    .filter_map(move |z| self.spatial_buckets.get(&(x, z)))
                    .flat_map(|ids| ids.iter())
            })
            .filter_map(|id| self.get_by_id(*id))
            .filter(move |entity| entity.position.distance_squared(center) <= radius_sq)
    }

    /// Radius query restricted to one or more entity types.
    pub fn query_radius_types<'a>(
        &'a self,
        center: Vec3,
        radius: f32,
        types: &'a [EntityType],
    ) -> impl Iterator<Item = &'a Entity> + 'a {
        self.query_radius(center, radius)
            .filter(move |entity| types.contains(&entity.entity_type))
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
    fn r59_iteration_classification_keeps_candidate_queries_distinct() {
        assert!(is_global_entity_maintenance(
            EntityIterationKind::GlobalSimulation
        ));
        assert!(is_global_entity_maintenance(
            EntityIterationKind::GlobalCleanup
        ));
        assert!(!is_global_entity_maintenance(
            EntityIterationKind::CandidateQuery
        ));
    }

    #[test]
    fn test_entity_manager_counts_passive_and_hostile() {
        let mut em = EntityManager::new();
        em.spawn(EntityType::Pig, Vec3::ZERO);
        em.spawn(EntityType::Cow, Vec3::ZERO);
        em.spawn(EntityType::Zombie, Vec3::ZERO);
        em.spawn(EntityType::Skeleton, Vec3::ZERO);
        em.spawn(EntityType::Creeper, Vec3::ZERO);
        em.spawn(EntityType::DroppedItem, Vec3::ZERO);

        assert_eq!(em.count_passive(), 2);
        assert_eq!(em.count_hostile(), 3);
    }

    #[test]
    fn moved_position_sync_visits_only_supplied_ids_and_preserves_buckets() {
        let mut em = EntityManager::new();
        let moved = em.spawn(EntityType::Pig, Vec3::new(1.0, 64.0, 1.0));
        for offset in 0..31 {
            em.spawn(
                EntityType::Cow,
                Vec3::new(2.0 + offset as f32 * 0.1, 64.0, 2.0),
            );
        }

        em.get_by_id_mut(moved).unwrap().position = Vec3::new(17.0, 64.0, 1.0);
        let visits = em.position_sync_visits;
        em.sync_entity_positions(&[moved]);

        assert_eq!(em.position_sync_visits - visits, 1);
        assert!(!em
            .get_entities_in_chunk(0, 0)
            .any(|entity| entity.id == moved));
        assert!(em
            .get_entities_in_chunk(1, 0)
            .any(|entity| entity.id == moved));
        assert_eq!(em.entities.len(), 32);
    }

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
    fn remote_player_representation_has_human_aabb_and_defaults() {
        let entity = Entity::new(9, EntityType::RemotePlayer, Vec3::ZERO);
        assert_eq!(entity.size, Vec3::new(0.6, 1.8, 0.6));
        assert_eq!(entity.player_id, 0);
        assert!(entity.username.is_empty());
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
                        c.set_block_local(x, y as i32, z, crate::world::BlockType::Air);
                    }
                }
            }
            // Place a 2x2 stone floor at y=10 covering the item's footprint.
            for fx in 0..2 {
                for fz in 0..2 {
                    c.set_block_local(fx, 10, fz, crate::world::BlockType::Stone);
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

    #[test]
    fn incremental_indexes_match_rebuild_oracle_after_lifecycle() {
        let mut em = EntityManager::new();
        let a = em.spawn(EntityType::Zombie, Vec3::new(1.0, 0.0, 1.0));
        let b = em.spawn(EntityType::Cow, Vec3::new(20.0, 0.0, 1.0));
        let c = em.spawn(EntityType::Cow, Vec3::new(1.0, 0.0, 20.0));
        em.get_by_id_mut(a).unwrap().position = Vec3::new(33.0, 0.0, 1.0);
        em.sync_entity_position(a);
        em.remove_by_id(b);
        em.retain(|entity| entity.id != c);

        let mut oracle = EntityManager::new();
        oracle.entities = em
            .entities
            .iter()
            .map(|entity| Entity {
                id: entity.id,
                ..Entity::new(entity.id, entity.entity_type, entity.position)
            })
            .collect();
        oracle.rebuild_indexes();
        assert_eq!(em.id_to_index, oracle.id_to_index);
        assert_eq!(em.type_buckets, oracle.type_buckets);
        assert_eq!(em.spatial_buckets, oracle.spatial_buckets);
        assert!(em.spatial_buckets.values().all(|bucket| {
            let mut ids = bucket.clone();
            ids.sort_unstable();
            ids.windows(2).all(|pair| pair[0] != pair[1])
        }));
    }

    #[test]
    fn entity_index_radius_query_handles_sparse_and_boundary_buckets() {
        let mut em = EntityManager::new();
        let center = em.spawn(EntityType::Cow, Vec3::new(15.5, 4.0, 0.0));
        let across_boundary = em.spawn(EntityType::Pig, Vec3::new(16.5, 4.0, 0.0));
        em.spawn(EntityType::Sheep, Vec3::new(1_000_000.0, 4.0, 1_000_000.0));

        let mut ids: Vec<_> = em
            .query_radius(Vec3::new(16.0, 4.0, 0.0), 1.0)
            .map(|entity| entity.id)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![center, across_boundary]);

        assert_eq!(
            em.query_radius(Vec3::new(15.5, 4.0, 0.0), -1.0)
                .map(|entity| entity.id)
                .collect::<Vec<_>>(),
            vec![center]
        );
    }

    #[test]
    fn entity_index_rebuild_removes_stale_empty_buckets() {
        let mut em = EntityManager::new();
        let cow = em.spawn(EntityType::Cow, Vec3::new(32.0, 0.0, 32.0));
        em.entities.clear();
        em.rebuild_indexes();

        assert!(em.id_to_index.is_empty());
        assert!(em.type_buckets.is_empty());
        assert!(em.spatial_buckets.is_empty());
        assert!(em.get_by_id(cow).is_none());
    }

    #[test]
    fn test_dropped_item_and_xp_orb_entity_properties() {
        let mut em = EntityManager::new();
        let item_id = em.spawn(EntityType::DroppedItem, Vec3::new(1.0, 2.0, 3.0));
        let xp_id = em.spawn(EntityType::ExperienceOrb, Vec3::new(1.0, 2.0, 3.0));

        let item_ent = em.get_by_id_mut(item_id).unwrap();
        item_ent.item_age = 299.9;
        assert!(item_ent.item_age < 300.0);
        item_ent.item_age = 300.0;
        assert!(item_ent.item_age >= 300.0);

        let xp_ent = em.get_by_id_mut(xp_id).unwrap();
        xp_ent.xp_value = 55;
        assert_eq!(xp_ent.xp_value, 55);
        assert_eq!(xp_ent.entity_type, EntityType::ExperienceOrb);
    }
}

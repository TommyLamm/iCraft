//! Dimension-specific hostile mobs, boss AI, and boss side effects.
//!
//! This module deliberately does not mutate chunks.  Callers apply the returned
//! [`BossEvents`] after the entity update, which keeps mesh/light invalidation in
//! the normal block-placement and explosion paths.

use crate::chunk_manager::ChunkManager;
use crate::dimension::Dimension;
use crate::entity::{EntityIterationKind, EntityManager, EntityType};
use crate::inventory::{GameMode, Item};
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use glam::Vec3;

pub type BlockPos = (i32, i32, i32);

const NETHER_MOB_CAP: usize = 10;
const SHULKER_CAP: usize = 6;
const ENDERMAN_CAP: usize = 12;
const PROJECTILE_LIFETIME: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Melee,
    BlazeFireball,
    ShulkerBullet,
    DragonCharge,
    DragonBreath,
    WitherSkull,
    WitherCharge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerDamageEvent {
    pub amount: f32,
    pub source_entity: Option<u64>,
    pub kind: DamageKind,
    pub knockback: Vec3,
}

impl PlayerDamageEvent {
    fn new(amount: f32, source_entity: Option<u64>, kind: DamageKind) -> Self {
        Self {
            amount,
            source_entity,
            kind,
            knockback: Vec3::ZERO,
        }
    }

    fn with_knockback(mut self, knockback: Vec3) -> Self {
        self.knockback = knockback;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WitherEffectEvent {
    pub duration: f32,
    pub amplifier: u8,
    pub source_entity: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExplosionEvent {
    pub position: Vec3,
    pub radius: f32,
    pub break_blocks: bool,
    pub source_entity: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropEvent {
    pub position: Vec3,
    pub item: Item,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPlacementEvent {
    pub position: BlockPos,
    pub block: BlockType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragonCompletionEvent {
    pub dragon_id: u64,
    pub portal_center: BlockPos,
    pub dragon_egg_position: BlockPos,
}

#[derive(Debug, Default)]
pub struct BossEvents {
    pub player_damage: Vec<PlayerDamageEvent>,
    pub apply_wither: Vec<WitherEffectEvent>,
    pub explosions: Vec<ExplosionEvent>,
    pub drops: Vec<DropEvent>,
    pub block_placements: Vec<BlockPlacementEvent>,
    pub dragon_completion: Option<DragonCompletionEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BossHud {
    pub entity_id: u64,
    pub boss_type: EntityType,
    pub title: &'static str,
    pub progress: f32,
}

/// Adds the dimension's persistent encounters and a bounded number of ambient
/// hostiles. `time` is mixed into a tiny deterministic RNG so tests and replays
/// remain reproducible without adding a random-number dependency.
pub fn ensure_dimension_entities(
    dimension: Dimension,
    entities: &mut EntityManager,
    chunks: &ChunkManager,
    player_pos: Vec3,
    time: f32,
) {
    match dimension {
        Dimension::Overworld => {}
        Dimension::Nether => ensure_nether_mob(entities, chunks, player_pos, time),
        Dimension::End => ensure_end_encounters(entities, chunks, player_pos, time),
    }
}

fn ensure_nether_mob(
    entities: &mut EntityManager,
    chunks: &ChunkManager,
    player_pos: Vec3,
    time: f32,
) {
    // Global cap maintenance: count only indexed hostile buckets, not a
    // candidate radius/target query.
    debug_assert!(crate::entity::is_global_entity_maintenance(
        EntityIterationKind::GlobalCleanup
    ));
    let nether_count = [EntityType::Blaze, EntityType::Piglin, EntityType::Husk]
        .into_iter()
        .map(|kind| {
            entities
                .get_entities_by_type(kind)
                .filter(|entity| entity.health > 0.0)
                .count()
        })
        .sum::<usize>();
    if nether_count >= NETHER_MOB_CAP || chunks.chunks.is_empty() {
        return;
    }

    let mut seed = mix64(
        time.to_bits() as u64
            ^ (player_pos.x.floor() as i64 as u64).rotate_left(17)
            ^ (player_pos.z.floor() as i64 as u64).rotate_left(39)
            ^ nether_count as u64,
    );
    let loaded: Vec<(i32, i32)> = chunks.chunks.keys().copied().collect();
    let (cx, cz) = loaded[(next_u64(&mut seed) as usize) % loaded.len()];
    let wx = cx * CHUNK_WIDTH as i32 + (next_u64(&mut seed) % CHUNK_WIDTH as u64) as i32;
    let wz = cz * CHUNK_DEPTH as i32 + (next_u64(&mut seed) % CHUNK_DEPTH as u64) as i32;
    let Some(y) = open_surface_y(chunks, wx, wz) else {
        return;
    };

    // Do not materialize an enemy directly on top of the player.
    let pos = Vec3::new(wx as f32 + 0.5, y as f32, wz as f32 + 0.5);
    if pos.distance_squared(player_pos) < 8.0 * 8.0 {
        return;
    }
    let kind = match next_u64(&mut seed) % 6 {
        0 | 1 => EntityType::Blaze,
        2 | 3 | 4 => EntityType::Piglin,
        _ => EntityType::Husk,
    };
    entities.spawn(kind, pos);
}

fn ensure_end_encounters(
    entities: &mut EntityManager,
    chunks: &ChunkManager,
    player_pos: Vec3,
    time: f32,
) {
    let dragon_exists = entities
        .get_entities_by_type(EntityType::EnderDragon)
        .next()
        .is_some();
    let dragon_completed = chunks.chunks.values().any(|chunk| {
        chunk
            .sections
            .iter()
            .flatten()
            .any(|section| section.contains_block(BlockType::DragonEgg))
    });

    // The dragon egg is the persistent world marker that prevents a defeated
    // dragon from being recreated after its entity has been removed.
    if !dragon_exists && !dragon_completed {
        entities.spawn(EntityType::EnderDragon, Vec3::new(0.5, 92.0, 0.5));
        for (x, y, z) in crate::dimension::END_CRYSTAL_TOWERS {
            entities.spawn(
                EntityType::EndCrystal,
                Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5),
            );
        }
    }

    ensure_enderman(entities, chunks, player_pos, time);

    let shulker_count = entities
        .get_entities_by_type(EntityType::Shulker)
        .filter(|entity| entity.health > 0.0)
        .count();
    if shulker_count >= SHULKER_CAP {
        return;
    }

    // Purpur is an unambiguous loaded End City marker. Pick one roof position
    // per call so entering a large city never creates an unbounded burst.
    let mut candidates = Vec::new();
    for (&(cx, cz), chunk) in &chunks.chunks {
        for lx in 0..CHUNK_WIDTH {
            for lz in 0..CHUNK_DEPTH {
                for y in (1..CHUNK_HEIGHT - 1).rev() {
                    let wy = y as i32 + (chunk.min_section_y as i32 * 16);
                    if chunk.get_block_local(lx, wy, lz) == BlockType::Purpur
                        && chunk.get_block_local(lx, wy + 1, lz) == BlockType::Air
                    {
                        candidates.push((
                            cx * CHUNK_WIDTH as i32 + lx as i32,
                            wy + 1,
                            cz * CHUNK_DEPTH as i32 + lz as i32,
                        ));
                        break;
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        return;
    }
    let index = mix64(time.to_bits() as u64 ^ shulker_count as u64) as usize % candidates.len();
    let (x, y, z) = candidates[index];
    let pos = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
    if !entities
        .query_radius_types(pos, 2.0, &[EntityType::Shulker])
        .any(|entity| entity.position.distance_squared(pos) < 4.0)
    {
        entities.spawn(EntityType::Shulker, pos);
    }
}

/// Advances dimension mobs and bosses, returning every world/player side
/// effect for the caller to apply. Entities with non-positive health are
/// consumed exactly once because they are removed before this function returns.
pub fn update_dimension_entities(
    dimension: Dimension,
    entities: &mut EntityManager,
    chunks: &ChunkManager,
    player_pos: Vec3,
    player_look: Vec3,
    dt: f32,
    game_mode: GameMode,
) -> BossEvents {
    let dt = dt.max(0.0).min(0.25);
    let mut events = BossEvents::default();
    collect_deaths(entities, &mut events);

    let crystal_positions: Vec<Vec3> = entities
        .get_entities_by_type(EntityType::EndCrystal)
        .filter(|entity| entity.health > 0.0)
        .map(|entity| entity.position)
        .collect();
    if dimension == Dimension::End {
        repair_legacy_end_crystal_towers(chunks, &crystal_positions, &mut events);
    }
    let mut pending_spawns = Vec::new();
    let mut removed_projectiles = Vec::new();
    let mut moved_ids = Vec::new();
    let is_creative = game_mode == GameMode::Creative;

    // Global simulation maintenance: every live entity receives its timer
    // tick/physics update; this is intentionally not a candidate query.
    debug_assert!(crate::entity::is_global_entity_maintenance(
        EntityIterationKind::GlobalSimulation
    ));
    for entity in &mut entities.entities {
        let position_before_update = entity.position;
        entity.action_cooldown = (entity.action_cooldown - dt).max(0.0);
        entity.ai_timer += dt;

        match entity.entity_type {
            EntityType::Blaze => {
                if !is_creative {
                    let delta = player_pos - entity.position;
                    let horizontal = Vec3::new(delta.x, 0.0, delta.z);
                    let desired_y = player_pos.y + 4.0;
                    entity.velocity = horizontal.normalize_or_zero()
                        * if delta.length() > 12.0 { 2.5 } else { -1.2 };
                    entity.velocity.y = (desired_y - entity.position.y).clamp(-2.0, 2.0);
                    if delta.length_squared() <= 28.0 * 28.0 && entity.action_cooldown <= 0.0 {
                        events.player_damage.push(PlayerDamageEvent::new(
                            5.0,
                            Some(entity.id),
                            DamageKind::BlazeFireball,
                        ));
                        entity.action_cooldown = 2.5;
                    }
                } else {
                    entity.velocity = Vec3::ZERO;
                }
                entity.update_physics(dt, chunks);
            }
            EntityType::Piglin | EntityType::Husk => {
                if !is_creative {
                    let delta = player_pos - entity.position;
                    let horizontal = Vec3::new(delta.x, 0.0, delta.z);
                    entity.velocity.x = horizontal.normalize_or_zero().x * 3.0;
                    entity.velocity.z = horizontal.normalize_or_zero().z * 3.0;
                    if delta.length_squared() <= 2.2 * 2.2 && entity.action_cooldown <= 0.0 {
                        events.player_damage.push(PlayerDamageEvent::new(
                            if entity.entity_type == EntityType::Piglin {
                                5.0
                            } else {
                                4.0
                            },
                            Some(entity.id),
                            DamageKind::Melee,
                        ));
                        entity.action_cooldown = 1.0;
                    }
                } else {
                    entity.velocity = Vec3::ZERO;
                }
                entity.update_physics(dt, chunks);
            }
            EntityType::Shulker => {
                if !is_creative
                    && entity.position.distance_squared(player_pos) <= 24.0 * 24.0
                    && entity.action_cooldown <= 0.0
                {
                    events.player_damage.push(PlayerDamageEvent::new(
                        4.0,
                        Some(entity.id),
                        DamageKind::ShulkerBullet,
                    ));
                    entity.action_cooldown = 3.0;
                }
            }
            EntityType::Enderman => {
                if !is_creative {
                    let delta = player_pos - entity.position;
                    let horizontal = Vec3::new(delta.x, 0.0, delta.z);
                    let player_eye = player_pos + Vec3::Y * 1.62;
                    let head = entity.position + Vec3::Y * 2.62;
                    let to_head = (head - player_eye).normalize_or_zero();
                    let looking_at_head = horizontal.length_squared() <= 32.0 * 32.0
                        && player_look.normalize_or_zero().dot(to_head) >= 0.995;
                    if entity.ai_phase == 0 {
                        if looking_at_head {
                            entity.enderman_gaze_timer += dt;
                            if entity.enderman_gaze_timer >= 5.0 {
                                entity.ai_phase = 1;
                                entity.target_player = true;
                            }
                        } else {
                            entity.enderman_gaze_timer = 0.0;
                        }
                    }

                    if entity.ai_phase == 1 {
                        let direction = horizontal.normalize_or_zero();
                        entity.target_player = true;
                        entity.yaw = f32::atan2(direction.x, direction.z);
                        entity.velocity.x = direction.x * 4.5;
                        entity.velocity.z = direction.z * 4.5;
                        if delta.length_squared() <= 2.2 * 2.2 && entity.action_cooldown <= 0.0 {
                            events.player_damage.push(PlayerDamageEvent::new(
                                7.0,
                                Some(entity.id),
                                DamageKind::Melee,
                            ));
                            entity.action_cooldown = 1.0;
                        }
                    } else {
                        entity.target_player = false;
                        // Calm Endermen wander instead of freezing in place.
                        // Each entity gets a stable, changing heading and a
                        // short pause within a six-second idle cycle.
                        let cycle = (entity.ai_timer / 6.0).floor() as u64;
                        let seed = entity
                            .id
                            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                            .wrapping_add(cycle.wrapping_mul(0xBF58_476D_1CE4_E5B9));
                        let angle = (seed as u32) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                        entity.yaw = angle;
                        let idle_speed = if entity.ai_timer % 6.0 < 4.5 {
                            1.25
                        } else {
                            0.0
                        };
                        entity.velocity.x = angle.sin() * idle_speed;
                        entity.velocity.z = angle.cos() * idle_speed;
                    }
                } else {
                    entity.target_player = false;
                    entity.velocity.x = 0.0;
                    entity.velocity.z = 0.0;
                }
                entity.update_physics(dt, chunks);
            }
            EntityType::EnderDragon => update_dragon(
                entity,
                &crystal_positions,
                player_pos,
                dt,
                game_mode,
                &mut pending_spawns,
                &mut events,
            ),
            EntityType::Wither => update_wither(
                entity,
                player_pos,
                dt,
                game_mode,
                &mut pending_spawns,
                &mut events,
            ),
            EntityType::WitherSkull | EntityType::DragonBreath => {
                entity.update_physics(dt, chunks);
                if projectile_hit(entity, chunks, player_pos, game_mode, &mut events) {
                    removed_projectiles.push(entity.id);
                }
            }
            _ => {}
        }
        if entity.position != position_before_update {
            moved_ids.push(entity.id);
        }
    }

    entities.retain(|entity| !removed_projectiles.contains(&entity.id));
    for (kind, position, velocity, damage) in pending_spawns {
        let id = entities.spawn(kind, position);
        if let Some(projectile) = entities.get_by_id_mut(id) {
            projectile.velocity = velocity;
            projectile.projectile_damage = damage;
            projectile.ai_timer = 0.0;
        }
    }
    entities.sync_entity_positions(&moved_ids);
    events
}

/// Old End saves may already contain healing-crystal entities from before
/// obsidian towers were part of terrain generation. Repair only a completely
/// absent support column beneath a live crystal; existing or damaged towers
/// are left untouched.
fn repair_legacy_end_crystal_towers(
    chunks: &ChunkManager,
    live_crystals: &[Vec3],
    events: &mut BossEvents,
) {
    for (center_x, crystal_y, center_z) in crate::dimension::END_CRYSTAL_TOWERS {
        let crystal_position = Vec3::new(
            center_x as f32 + 0.5,
            crystal_y as f32,
            center_z as f32 + 0.5,
        );
        if !live_crystals
            .iter()
            .any(|position| position.distance_squared(crystal_position) < 1.0)
        {
            continue;
        }

        let top_y = crystal_y - 1;
        let Some(((center_chunk_x, center_chunk_z), _)) =
            chunks.world_to_local(center_x, top_y, center_z)
        else {
            continue;
        };
        if !chunks
            .chunks
            .contains_key(&(center_chunk_x, center_chunk_z))
        {
            continue;
        }
        if (1..=top_y)
            .rev()
            .any(|y| chunks.get_block(center_x, y, center_z) == BlockType::Obsidian)
        {
            continue;
        }

        for dx in -2..=2 {
            for dz in -2..=2 {
                if dx * dx + dz * dz > 4 {
                    continue;
                }
                let x = center_x + dx;
                let z = center_z + dz;
                let Some(((chunk_x, chunk_z), _)) = chunks.world_to_local(x, top_y, z) else {
                    continue;
                };
                if !chunks.chunks.contains_key(&(chunk_x, chunk_z)) {
                    continue;
                }
                let ground_y = (1..top_y)
                    .rev()
                    .find(|y| chunks.get_block(x, *y, z) != BlockType::Air)
                    .unwrap_or(0);
                for y in (ground_y + 1)..=top_y {
                    if chunks.get_block(x, y, z) == BlockType::Air {
                        events.block_placements.push(BlockPlacementEvent {
                            position: (x, y, z),
                            block: BlockType::Obsidian,
                        });
                    }
                }
            }
        }
    }
}

fn update_dragon(
    dragon: &mut crate::entity::Entity,
    crystals: &[Vec3],
    player_pos: Vec3,
    dt: f32,
    game_mode: GameMode,
    pending_spawns: &mut Vec<(EntityType, Vec3, Vec3, f32)>,
    events: &mut BossEvents,
) {
    let health_ratio = if dragon.max_health > 0.0 {
        dragon.health / dragon.max_health
    } else {
        0.0
    };
    let attack_cycle = dragon.ai_timer % 18.0;
    dragon.ai_phase = if game_mode == GameMode::Creative {
        0
    } else if (10.0..14.5).contains(&attack_cycle) {
        1 // periodic charge, including while the dragon is at full health
    } else if health_ratio > 0.60 {
        0 // high orbit
    } else if health_ratio > 0.30 {
        1 // dive
    } else {
        2 // low orbit and breath
    };

    if !crystals.is_empty() && dragon.health < dragon.max_health {
        dragon.health = (dragon.health + 1.0 * dt).min(dragon.max_health);
    }

    match dragon.ai_phase {
        0 => {
            let angle = dragon.ai_timer * 0.35 + dragon.id as f32 * 0.13;
            let target = Vec3::new(angle.cos() * 42.0, 86.0, angle.sin() * 42.0);
            dragon.velocity = (target - dragon.position).normalize_or_zero() * 12.0;
        }
        1 => {
            let target = player_pos + Vec3::new(0.0, 1.5, 0.0);
            dragon.velocity = (target - dragon.position).normalize_or_zero() * 18.0;
            if game_mode != GameMode::Creative
                && dragon.position.distance_squared(player_pos) < 5.5 * 5.5
                && dragon.action_cooldown <= 0.0
            {
                let impact = dragon.velocity.normalize_or_zero() * 14.0 + Vec3::Y * 4.0;
                events.player_damage.push(
                    PlayerDamageEvent::new(10.0, Some(dragon.id), DamageKind::DragonCharge)
                        .with_knockback(impact),
                );
                dragon.action_cooldown = 1.25;
            }
        }
        _ => {
            let angle = dragon.ai_timer * 0.5;
            let target = player_pos + Vec3::new(angle.cos() * 18.0, 12.0, angle.sin() * 18.0);
            dragon.velocity = (target - dragon.position).normalize_or_zero() * 10.0;
            if game_mode != GameMode::Creative && dragon.action_cooldown <= 0.0 {
                let origin = dragon.position + Vec3::new(0.0, 1.0, 0.0);
                let velocity = (player_pos + Vec3::Y - origin).normalize_or_zero() * 11.0;
                pending_spawns.push((EntityType::DragonBreath, origin, velocity, 6.0));
                dragon.action_cooldown = 2.0;
            }
        }
    }

    // The dragon is moved directly by its AI instead of the generic flying
    // physics path, so keep its model orientation in sync explicitly. This
    // gives the body and head the same forward and vertical flight direction.
    if dragon.velocity.length_squared() > 0.0001 {
        let direction = dragon.velocity.normalize_or_zero();
        dragon.yaw = f32::atan2(direction.x, direction.z);
        // The mob vertex transform rotates +Z toward -Y for positive pitch.
        // Negate the geometric angle so the dragon's +Z head direction
        // follows an upward velocity with a matching upward tilt.
        dragon.pitch = -f32::asin(direction.y.clamp(-1.0, 1.0));
    }
    dragon.position += dragon.velocity * dt;
}

fn ensure_enderman(
    entities: &mut EntityManager,
    chunks: &ChunkManager,
    player_pos: Vec3,
    time: f32,
) {
    let count = entities
        .get_entities_by_type(EntityType::Enderman)
        .filter(|entity| entity.health > 0.0)
        .count();
    if count >= ENDERMAN_CAP || chunks.chunks.is_empty() {
        return;
    }

    let loaded: Vec<(i32, i32)> = chunks.chunks.keys().copied().collect();
    let mut seed = mix64(
        time.to_bits() as u64
            ^ (player_pos.x.floor() as i64 as u64).rotate_left(13)
            ^ (player_pos.z.floor() as i64 as u64).rotate_left(37)
            ^ count as u64,
    );
    for _ in 0..8 {
        let (cx, cz) = loaded[(next_u64(&mut seed) as usize) % loaded.len()];
        let wx = cx * CHUNK_WIDTH as i32 + (next_u64(&mut seed) % CHUNK_WIDTH as u64) as i32;
        let wz = cz * CHUNK_DEPTH as i32 + (next_u64(&mut seed) % CHUNK_DEPTH as u64) as i32;
        let Some(y) = open_surface_y(chunks, wx, wz) else {
            continue;
        };
        if chunks.get_block(wx, y - 1, wz) != BlockType::EndStone
            || chunks.get_block(wx, y + 2, wz) != BlockType::Air
        {
            continue;
        }
        let position = Vec3::new(wx as f32 + 0.5, y as f32, wz as f32 + 0.5);
        if position.distance_squared(player_pos) < 8.0 * 8.0
            || entities
                .query_radius_types(position, 3.0, &[EntityType::Enderman])
                .next()
                .is_some()
        {
            continue;
        }
        entities.spawn(EntityType::Enderman, position);
        break;
    }
}

fn update_wither(
    wither: &mut crate::entity::Entity,
    player_pos: Vec3,
    dt: f32,
    game_mode: GameMode,
    pending_spawns: &mut Vec<(EntityType, Vec3, Vec3, f32)>,
    events: &mut BossEvents,
) {
    let low_health = wither.health <= wither.max_health * 0.5;
    wither.ai_phase = u8::from(low_health);
    let is_creative = game_mode == GameMode::Creative;

    if !is_creative {
        let target_height = if low_health { 2.5 } else { 8.0 };
        let target = player_pos + Vec3::new(0.0, target_height, 0.0);
        let speed = if low_health { 13.0 } else { 7.0 };
        wither.velocity = (target - wither.position).normalize_or_zero() * speed;
    } else {
        wither.velocity = Vec3::ZERO;
    }
    wither.position += wither.velocity * dt;

    if !is_creative {
        if low_health
            && wither.position.distance_squared(player_pos) < 3.5 * 3.5
            && wither.action_cooldown <= 0.0
        {
            events.player_damage.push(PlayerDamageEvent::new(
                12.0,
                Some(wither.id),
                DamageKind::WitherCharge,
            ));
            events.apply_wither.push(WitherEffectEvent {
                duration: 10.0,
                amplifier: 1,
                source_entity: Some(wither.id),
            });
            events.explosions.push(ExplosionEvent {
                position: wither.position,
                radius: 3.0,
                break_blocks: true,
                source_entity: Some(wither.id),
            });
            wither.action_cooldown = 1.5;
        } else if wither.action_cooldown <= 0.0 {
            let origin = wither.position + Vec3::new(0.0, 2.2, 0.0);
            let velocity = (player_pos + Vec3::Y - origin).normalize_or_zero() * 14.0;
            pending_spawns.push((
                EntityType::WitherSkull,
                origin,
                velocity,
                if low_health { 10.0 } else { 8.0 },
            ));
            wither.action_cooldown = if low_health { 1.0 } else { 1.8 };
        }
    }
}

fn projectile_hit(
    projectile: &crate::entity::Entity,
    chunks: &ChunkManager,
    player_pos: Vec3,
    game_mode: GameMode,
    events: &mut BossEvents,
) -> bool {
    let expired = projectile.ai_timer >= PROJECTILE_LIFETIME;
    let position = (
        projectile.position.x.floor() as i32,
        projectile.position.y.floor() as i32,
        projectile.position.z.floor() as i32,
    );
    let block_hit = chunks
        .get_block(position.0, position.1, position.2)
        .properties()
        .is_solid;
    let player_hit = game_mode != GameMode::Creative
        && projectile.position.distance_squared(player_pos) <= 1.35 * 1.35;
    if !expired && !block_hit && !player_hit {
        return false;
    }

    if player_hit {
        let kind = if projectile.entity_type == EntityType::WitherSkull {
            DamageKind::WitherSkull
        } else {
            DamageKind::DragonBreath
        };
        events.player_damage.push(PlayerDamageEvent::new(
            projectile.projectile_damage.max(1.0),
            Some(projectile.id),
            kind,
        ));
        if projectile.entity_type == EntityType::WitherSkull {
            events.apply_wither.push(WitherEffectEvent {
                duration: 8.0,
                amplifier: 1,
                source_entity: Some(projectile.id),
            });
        }
    }
    if block_hit && projectile.entity_type == EntityType::WitherSkull {
        events.explosions.push(ExplosionEvent {
            position: projectile.position,
            radius: 1.75,
            break_blocks: true,
            source_entity: Some(projectile.id),
        });
    }
    true
}

fn collect_deaths(entities: &mut EntityManager, events: &mut BossEvents) {
    let mut dead_ids = Vec::new();
    // Global cleanup maintenance: consume dead boss-owned entities exactly
    // once. This pass is deliberately exhaustive and does not select targets.
    debug_assert!(crate::entity::is_global_entity_maintenance(
        EntityIterationKind::GlobalCleanup
    ));
    for entity in &entities.entities {
        // Projectile/particle/item entities intentionally have max_health == 0;
        // they expire through their own lifetime rules rather than the mob-death
        // path. Only entities owned by this module are consumed here.
        if entity.health > 0.0
            || !matches!(
                entity.entity_type,
                EntityType::Blaze
                    | EntityType::Piglin
                    | EntityType::Husk
                    | EntityType::Shulker
                    | EntityType::Enderman
                    | EntityType::EndCrystal
                    | EntityType::EnderDragon
                    | EntityType::Wither
            )
        {
            continue;
        }
        dead_ids.push(entity.id);
        match entity.entity_type {
            EntityType::Blaze => events.drops.push(DropEvent {
                position: entity.position,
                item: Item::BlazeRod,
                count: 1,
            }),
            EntityType::Shulker => events.drops.push(DropEvent {
                position: entity.position,
                item: Item::ShulkerShell,
                count: 1,
            }),
            EntityType::EndCrystal => events.explosions.push(ExplosionEvent {
                position: entity.position,
                radius: 6.0,
                break_blocks: true,
                source_entity: Some(entity.id),
            }),
            EntityType::EnderDragon => complete_dragon(entity.id, entity.position, events),
            EntityType::Wither => events.drops.push(DropEvent {
                position: entity.position,
                item: Item::NetherStar,
                count: 1,
            }),
            _ => {}
        }
    }
    entities.retain(|entity| !dead_ids.contains(&entity.id));
}

fn complete_dragon(dragon_id: u64, death_position: Vec3, events: &mut BossEvents) {
    const EXIT_Y: i32 = 73;
    let portal_center = (0, EXIT_Y, 0);
    for x in -1..=1 {
        for z in -1..=1 {
            events.block_placements.push(BlockPlacementEvent {
                position: (x, EXIT_Y, z),
                block: BlockType::EndPortal,
            });
        }
    }
    let egg = (0, EXIT_Y + 5, 0);
    events.block_placements.push(BlockPlacementEvent {
        position: egg,
        block: BlockType::DragonEgg,
    });
    events.drops.push(DropEvent {
        position: death_position,
        item: Item::EndCrystal,
        count: 4,
    });
    events.dragon_completion = Some(DragonCompletionEvent {
        dragon_id,
        portal_center,
        dragon_egg_position: egg,
    });
}

/// Recognizes either horizontal orientation of the seven-block Wither summon.
/// The returned positions contain all three skulls followed by all four soul
/// sand blocks and are suitable for atomic validation/consumption by the caller.
pub fn detect_wither_pattern<F>(changed: BlockPos, getter: F) -> Option<Vec<BlockPos>>
where
    F: Fn(BlockPos) -> BlockType,
{
    for skull_y in (changed.1 - 2)..=(changed.1 + 2) {
        for center_x in (changed.0 - 1)..=(changed.0 + 1) {
            for center_z in (changed.2 - 1)..=(changed.2 + 1) {
                for axis in [(1, 0), (0, 1)] {
                    let center = (center_x, skull_y, center_z);
                    let skulls = [
                        (center.0 - axis.0, center.1, center.2 - axis.1),
                        center,
                        (center.0 + axis.0, center.1, center.2 + axis.1),
                    ];
                    let soul_sand = [
                        (skulls[0].0, skull_y - 1, skulls[0].2),
                        (center.0, skull_y - 1, center.2),
                        (skulls[2].0, skull_y - 1, skulls[2].2),
                        (center.0, skull_y - 2, center.2),
                    ];
                    if skulls
                        .iter()
                        .all(|&pos| getter(pos) == BlockType::WitherSkeletonSkull)
                        && soul_sand
                            .iter()
                            .all(|&pos| getter(pos) == BlockType::SoulSand)
                    {
                        let mut consumed = Vec::with_capacity(7);
                        consumed.extend(skulls);
                        consumed.extend(soul_sand);
                        return Some(consumed);
                    }
                }
            }
        }
    }
    None
}

pub fn active_boss_hud(entities: &EntityManager) -> Option<BossHud> {
    [EntityType::EnderDragon, EntityType::Wither]
        .into_iter()
        .flat_map(|kind| entities.get_entities_by_type(kind))
        .find_map(|entity| {
            let title = entity.entity_type.boss_name()?;
            let progress = if entity.max_health > 0.0 {
                (entity.health / entity.max_health).clamp(0.0, 1.0)
            } else {
                0.0
            };
            Some(BossHud {
                entity_id: entity.id,
                boss_type: entity.entity_type,
                title,
                progress,
            })
        })
}

fn open_surface_y(chunks: &ChunkManager, wx: i32, wz: i32) -> Option<i32> {
    (1..CHUNK_HEIGHT as i32 - 2).rev().find_map(|y| {
        let floor = chunks.get_block(wx, y, wz);
        let feet = chunks.get_block(wx, y + 1, wz);
        let head = chunks.get_block(wx, y + 2, wz);
        if floor.properties().is_solid && feet == BlockType::Air && head == BlockType::Air {
            Some(y + 1)
        } else {
            None
        }
    })
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    mix64(*state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;
    use std::collections::HashMap;

    #[test]
    fn legacy_live_crystal_without_tower_gets_obsidian_support_events() {
        let mut chunks = ChunkManager::new(1);
        let mut chunk = Chunk::new(2, 0);
        let local_x = 42usize.rem_euclid(CHUNK_WIDTH);
        for y in 1..78 {
            chunk.set_block_local(local_x, y, 0, BlockType::Air);
        }
        chunk.set_block_local(local_x, 65, 0, BlockType::EndStone);
        chunks.chunks.insert((2, 0), chunk);

        let mut events = BossEvents::default();
        repair_legacy_end_crystal_towers(&chunks, &[Vec3::new(42.5, 78.0, 0.5)], &mut events);

        assert!(events.block_placements.iter().any(|placement| {
            placement.position == (42, 77, 0) && placement.block == BlockType::Obsidian
        }));
    }

    fn pattern_blocks(axis: (i32, i32)) -> HashMap<BlockPos, BlockType> {
        let mut blocks = HashMap::new();
        for offset in -1..=1 {
            blocks.insert(
                (axis.0 * offset, 12, axis.1 * offset),
                BlockType::WitherSkeletonSkull,
            );
            blocks.insert((axis.0 * offset, 11, axis.1 * offset), BlockType::SoulSand);
        }
        blocks.insert((0, 10, 0), BlockType::SoulSand);
        blocks
    }

    #[test]
    fn detects_x_oriented_wither_pattern() {
        let blocks = pattern_blocks((1, 0));
        let found = detect_wither_pattern((1, 12, 0), |pos| {
            blocks.get(&pos).copied().unwrap_or(BlockType::Air)
        });
        assert_eq!(found.as_ref().map(Vec::len), Some(7));
        assert!(found.unwrap().contains(&(0, 10, 0)));
    }

    #[test]
    fn detects_z_oriented_wither_pattern() {
        let blocks = pattern_blocks((0, 1));
        let found = detect_wither_pattern((0, 11, -1), |pos| {
            blocks.get(&pos).copied().unwrap_or(BlockType::Air)
        });
        assert_eq!(found.as_ref().map(Vec::len), Some(7));
    }

    #[test]
    fn rejects_near_miss_wither_pattern() {
        let mut blocks = pattern_blocks((1, 0));
        blocks.remove(&(0, 10, 0));
        assert!(detect_wither_pattern((0, 12, 0), |pos| {
            blocks.get(&pos).copied().unwrap_or(BlockType::Air)
        })
        .is_none());
    }

    #[test]
    fn boss_bar_progress_is_clamped() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::Wither, Vec3::ZERO);
        entities.entities[0].health = entities.entities[0].max_health * 2.0;
        assert_eq!(active_boss_hud(&entities).unwrap().progress, 1.0);
        entities.entities[0].health = -10.0;
        assert_eq!(active_boss_hud(&entities).unwrap().progress, 0.0);
    }

    #[test]
    fn dragon_changes_phase_and_crystal_heals_it() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::EnderDragon, Vec3::new(0.0, 80.0, 0.0));
        entities.spawn(EntityType::EndCrystal, Vec3::new(8.0, 80.0, 0.0));
        entities.entities[0].health = 100.0;
        let chunks = ChunkManager::new(1);

        update_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            Vec3::ZERO,
            Vec3::ZERO,
            0.2,
            GameMode::Survival,
        );

        let dragon = entities
            .get_entities_by_type(EntityType::EnderDragon)
            .next()
            .unwrap();
        assert_eq!(dragon.ai_phase, 1);
        assert!(dragon.health > 100.0);
    }

    #[test]
    fn dragon_orientation_follows_flight_velocity() {
        let mut dragon =
            crate::entity::Entity::new(1, EntityType::EnderDragon, Vec3::new(0.0, 80.0, 0.0));
        let mut pending_spawns = Vec::new();
        let mut events = BossEvents::default();

        update_dragon(
            &mut dragon,
            &[],
            Vec3::new(0.0, 80.0, 0.0),
            0.0,
            GameMode::Survival,
            &mut pending_spawns,
            &mut events,
        );

        let direction = dragon.velocity.normalize();
        assert!((dragon.yaw - f32::atan2(direction.x, direction.z)).abs() < 1e-6);
        assert!((dragon.pitch + f32::asin(direction.y)).abs() < 1e-6);
    }

    #[test]
    fn full_health_dragon_periodically_charges_and_knocks_player_back() {
        let mut entities = EntityManager::new();
        let dragon_id = entities.spawn(EntityType::EnderDragon, Vec3::new(0.0, 80.0, 4.0));
        entities.get_by_id_mut(dragon_id).unwrap().ai_timer = 9.9;
        let chunks = ChunkManager::new(1);

        let events = update_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            Vec3::new(0.0, 80.0, 0.0),
            Vec3::ZERO,
            0.2,
            GameMode::Survival,
        );

        assert_eq!(entities.get_by_id(dragon_id).unwrap().ai_phase, 1);
        let hit = events
            .player_damage
            .iter()
            .find(|hit| hit.kind == DamageKind::DragonCharge)
            .expect("charging dragon should collide with a nearby player");
        assert!(hit.knockback.length() > 10.0);
        assert!(hit.knockback.y > 0.0);
    }

    #[test]
    fn end_dimension_spawns_enderman_on_end_stone() {
        let mut chunks = ChunkManager::new(1);
        let mut chunk = Chunk::new(0, 0);
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 1..CHUNK_HEIGHT {
                    chunk.set_block_local(x, y as i32, z, BlockType::Air);
                }
                chunk.set_block_local(x, 64, z, BlockType::EndStone);
            }
        }
        chunks.chunks.insert((0, 0), chunk);
        let mut entities = EntityManager::new();

        ensure_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            Vec3::new(100.0, 65.0, 100.0),
            3.0,
        );

        let enderman = entities
            .get_entities_by_type(EntityType::Enderman)
            .next()
            .expect("End terrain should spawn an Enderman");
        assert_eq!(
            chunks.get_block(
                enderman.position.x.floor() as i32,
                enderman.position.y.floor() as i32 - 1,
                enderman.position.z.floor() as i32,
            ),
            BlockType::EndStone
        );
    }

    #[test]
    fn enderman_only_attacks_after_five_seconds_of_head_gaze() {
        let mut entities = EntityManager::new();
        let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 0.0, 10.0));
        entities.get_by_id_mut(id).unwrap().enderman_gaze_timer = 4.7;
        let chunks = ChunkManager::new(1);
        let player = Vec3::ZERO;
        let look = (Vec3::new(0.0, 2.62, 10.0) - Vec3::Y * 1.62).normalize();

        update_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            player,
            look,
            0.2,
            GameMode::Survival,
        );
        assert_eq!(entities.get_by_id(id).unwrap().ai_phase, 0);
        assert!(!entities.get_by_id(id).unwrap().target_player);

        let moved_head = entities.get_by_id(id).unwrap().position + Vec3::Y * 2.62;
        let look = (moved_head - Vec3::Y * 1.62).normalize();
        update_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            player,
            look,
            0.2,
            GameMode::Survival,
        );
        assert_eq!(entities.get_by_id(id).unwrap().ai_phase, 1);
        assert!(entities.get_by_id(id).unwrap().target_player);
    }

    #[test]
    fn enderman_gaze_timer_resets_when_player_looks_away() {
        let mut entities = EntityManager::new();
        let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 0.0, 10.0));
        entities.get_by_id_mut(id).unwrap().enderman_gaze_timer = 4.9;
        let chunks = ChunkManager::new(1);

        update_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            Vec3::ZERO,
            -Vec3::Z,
            0.2,
            GameMode::Survival,
        );

        let enderman = entities.get_by_id(id).unwrap();
        assert_eq!(enderman.ai_phase, 0);
        assert_eq!(enderman.enderman_gaze_timer, 0.0);
        assert!(!enderman.target_player);
    }

    #[test]
    fn calm_enderman_wanders_without_targeting_player() {
        let mut entities = EntityManager::new();
        let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 64.0, 10.0));
        let chunks = ChunkManager::new(1);

        update_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            Vec3::ZERO,
            -Vec3::Z,
            0.1,
            GameMode::Survival,
        );

        let enderman = entities.get_by_id(id).unwrap();
        assert_eq!(enderman.ai_phase, 0);
        assert!(!enderman.target_player);
        assert!(Vec3::new(enderman.velocity.x, 0.0, enderman.velocity.z).length() > 1.0);
    }

    #[test]
    fn wither_enters_low_health_charge_phase() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::Wither, Vec3::new(0.0, 8.0, 0.0));
        entities.entities[0].health = 140.0;
        let chunks = ChunkManager::new(1);

        update_dimension_entities(
            Dimension::Overworld,
            &mut entities,
            &chunks,
            Vec3::ZERO,
            Vec3::ZERO,
            0.1,
            GameMode::Survival,
        );

        assert_eq!(entities.entities[0].ai_phase, 1);
    }

    #[test]
    fn creative_mode_bosses_do_not_attack_player() {
        let mut entities = EntityManager::new();
        entities.spawn(EntityType::Blaze, Vec3::new(5.0, 0.0, 0.0));
        entities.spawn(EntityType::Piglin, Vec3::new(1.0, 0.0, 0.0));
        entities.spawn(EntityType::EnderDragon, Vec3::new(0.0, 80.0, 0.0));
        let chunks = ChunkManager::new(1);

        let events = update_dimension_entities(
            Dimension::Nether,
            &mut entities,
            &chunks,
            Vec3::ZERO,
            Vec3::ZERO,
            0.1,
            GameMode::Creative,
        );

        assert!(
            events.player_damage.is_empty(),
            "Bosses emitted player damage events in Creative mode!"
        );
        let dragon = entities
            .get_entities_by_type(EntityType::EnderDragon)
            .next()
            .unwrap();
        assert_eq!(dragon.ai_phase, 0, "Dragon left phase 0 in Creative mode!");
    }

    #[test]
    fn nether_spawning_is_bounded_and_inside_loaded_chunks() {
        let mut chunks = ChunkManager::new(1);
        let mut chunk = Chunk::new(0, 0);
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 1..CHUNK_HEIGHT {
                    chunk.set_block_local(x, y as i32, z, BlockType::Air);
                }
                chunk.set_block_local(x, 40, z, BlockType::Netherrack);
            }
        }
        chunks.chunks.insert((0, 0), chunk);
        let mut entities = EntityManager::new();
        for tick in 0..128 {
            ensure_dimension_entities(
                Dimension::Nether,
                &mut entities,
                &chunks,
                Vec3::new(128.0, 41.0, 128.0),
                tick as f32,
            );
        }
        assert!(entities.entities.len() <= NETHER_MOB_CAP);
        assert!(!entities.entities.is_empty());
        assert!(entities.entities.iter().all(|entity| {
            entity.position.x >= 0.0
                && entity.position.x < CHUNK_WIDTH as f32
                && entity.position.z >= 0.0
                && entity.position.z < CHUNK_DEPTH as f32
        }));
    }
}

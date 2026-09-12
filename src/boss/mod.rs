//! Dimension-specific hostile mobs, boss AI, and boss side effects.
//!
//! This module deliberately does not mutate chunks.  Callers apply the returned
//! [`BossEvents`] after the entity update, which keeps mesh/light invalidation in
//! the normal block-placement and explosion paths.

use crate::chunk_manager::WorldColumns;
use crate::dimension::Dimension;
use crate::entity::{EntityIterationKind, EntityManager, EntityType};
use crate::inventory::{GameMode, Item};
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_WIDTH, SECTION_SIZE};
use glam::Vec3;

mod dragon;
mod nether;
mod wither;

use dragon::{
    complete_dragon, ensure_end_encounters, ensure_enderman, repair_legacy_end_crystal_towers,
    update_dragon,
};
use nether::ensure_nether_mob;
use wither::{collect_deaths, detect_wither_pattern, projectile_hit, update_wither};


pub type BlockPos = (i32, i32, i32);

const NETHER_MOB_CAP: usize = 10;
const SHULKER_CAP: usize = 6;
const ENDERMAN_CAP: usize = 12;
const ENDERMAN_GAZE_RANGE: f32 = 32.0;
const ENDERMAN_GAZE_DURATION: f32 = 3.0;
// A forgiving cone keeps normal camera jitter and the Enderman's idle walk
// from breaking the gaze. It is centered on the head and therefore
// works from every side of the model, independent of the Enderman's yaw.
const ENDERMAN_GAZE_DOT: f32 = 0.95;
const PROJECTILE_LIFETIME: f32 = 12.0;
const LEGACY_TOWER_REPAIR_INTERVAL: f32 = 1.0;
const DRAGON_EGG_POSITION: BlockPos = (0, 78, 0);

pub(super) fn player_is_gazing_at_enderman_head(player_eye: Vec3, player_look: Vec3, head: Vec3) -> bool {
    let to_head = head - player_eye;
    let distance_squared = to_head.length_squared();
    if distance_squared <= f32::EPSILON
        || distance_squared > ENDERMAN_GAZE_RANGE * ENDERMAN_GAZE_RANGE
    {
        return false;
    }

    let look = player_look.normalize_or_zero();
    look.length_squared() > 0.0 && look.dot(to_head.normalize()) >= ENDERMAN_GAZE_DOT
}

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
    chunks: &WorldColumns,
    player_pos: Vec3,
    time: f32,
) {
    match dimension {
        Dimension::Overworld => {}
        Dimension::Nether => ensure_nether_mob(entities, chunks, player_pos, time),
        Dimension::End => ensure_end_encounters(entities, chunks, player_pos, time),
    }
}

pub fn update_dimension_entities(
    dimension: Dimension,
    entities: &mut EntityManager,
    chunks: &WorldColumns,
    players: &[(Vec3, Vec3)],
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
    let repair_due = dimension == Dimension::End
        && entities
            .get_entities_by_type(EntityType::EnderDragon)
            .any(|dragon| periodic_work_due(dragon.ai_timer, dt, LEGACY_TOWER_REPAIR_INTERVAL));
    if repair_due {
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

        let nearest = players.iter().min_by(|(left, _), (right, _)| {
            entity
                .position
                .distance_squared(*left)
                .total_cmp(&entity.position.distance_squared(*right))
        });
        let (player_pos, player_look) = nearest
            .copied()
            .unwrap_or((Vec3::ZERO, Vec3::NEG_Z));

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
                entity.update_physics_in(dt, chunks);
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
                entity.update_physics_in(dt, chunks);
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
                // Creative players cannot provoke an Enderman, but changing
                // game mode must not freeze its normal idle movement.
                if is_creative {
                    entity.ai_phase = 0;
                    entity.enderman_gaze_timer = 0.0;
                }

                if !is_creative {
                    let delta = player_pos - entity.position;
                    let horizontal = Vec3::new(delta.x, 0.0, delta.z);
                    let player_eye = player_pos + Vec3::Y * 1.62;
                    let head = entity.position + Vec3::Y * 2.62;
                    let looking_at_head =
                        player_is_gazing_at_enderman_head(player_eye, player_look, head);
                    if entity.ai_phase == 0 {
                        if looking_at_head {
                            entity.enderman_gaze_timer += dt;
                            if entity.enderman_gaze_timer >= ENDERMAN_GAZE_DURATION {
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
                    }
                }

                if is_creative || entity.ai_phase == 0 {
                    entity.target_player = false;
                    // Calm Endermen wander in every game mode. Each entity
                    // gets a stable, changing heading and a short pause within
                    // a six-second idle cycle.
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
                entity.update_physics_in(dt, chunks);
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
                entity.update_physics_in(dt, chunks);
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

pub(super) fn open_surface_y(chunks: &WorldColumns, wx: i32, wz: i32) -> Option<i32> {
    let height = chunks.dimension.height();
    let min_y = height.min_y() + 1;
    let max_y = height.max_y_exclusive() - 2;
    (min_y..max_y).rev().find_map(|y| {
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

pub(super) fn periodic_work_due(timer: f32, dt: f32, interval: f32) -> bool {
    timer <= f32::EPSILON || (timer / interval).floor() != ((timer + dt) / interval).floor()
}

pub(super) fn mix64(value: u64) -> u64 {
    crate::world_tick::deterministic_rng(value, 0)
}

pub(super) fn next_u64(state: &mut u64) -> u64 {
    crate::world_tick::next_splitmix64(state)
}

#[cfg(test)]
mod tests;

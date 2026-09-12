use super::*;
use super::dragon::complete_dragon;

pub(super) fn update_wither(
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

pub(super) fn projectile_hit(
    projectile: &crate::entity::Entity,
    chunks: &WorldColumns,
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

pub(super) fn collect_deaths(entities: &mut EntityManager, events: &mut BossEvents) {
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
            EntityType::Enderman => events.drops.push(DropEvent {
                position: entity.position,
                item: Item::EyeOfEnder,
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

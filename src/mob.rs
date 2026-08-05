use crate::chunk_manager::{mark_block_mesh_dependencies, ChunkManager};
use crate::entity::{Entity, EntityIterationKind, EntityManager, EntityType};
use crate::inventory::GameMode;
use crate::physics::PlayerPhysics;
use crate::player::PlayerState;
use glam::Vec3;

fn should_retain_after_health_cleanup(entity: &Entity) -> bool {
    entity.health > 0.0
        || (entity.max_health <= 0.0 && entity.health >= 0.0)
        || matches!(
            entity.entity_type,
            EntityType::Blaze
                | EntityType::Piglin
                | EntityType::Husk
                | EntityType::Shulker
                | EntityType::Enderman
                | EntityType::EnderDragon
                | EntityType::Wither
                | EntityType::EndCrystal
                | EntityType::RemotePlayer
        )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlayerHitSource {
    AttackerPosition(Vec3),
    ProjectileVelocity(Vec3),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PlayerHitEvent {
    amount: f32,
    source: PlayerHitSource,
}

impl PlayerHitEvent {
    fn from_attacker(amount: f32, position: Vec3) -> Self {
        Self {
            amount,
            source: PlayerHitSource::AttackerPosition(position),
        }
    }

    fn from_projectile(amount: f32, velocity: Vec3) -> Self {
        Self {
            amount,
            source: PlayerHitSource::ProjectileVelocity(velocity),
        }
    }

    fn horizontal_knockback_direction(self, player_position: Vec3) -> Vec3 {
        let direction = match self.source {
            PlayerHitSource::AttackerPosition(position) => player_position - position,
            PlayerHitSource::ProjectileVelocity(velocity) => velocity,
        };
        Vec3::new(direction.x, 0.0, direction.z).normalize_or_zero()
    }
}

pub fn calculate_explosion_damage(center: Vec3, player_pos: Vec3) -> f32 {
    let dist = center.distance(player_pos);
    if dist >= 5.0 {
        0.0
    } else {
        (5.0 - dist) * 5.0
    }
}

pub fn explode(
    center: Vec3,
    radius: f32,
    chunk_manager: &mut ChunkManager,
    dirty_meshes: &mut std::collections::HashSet<(i32, i32)>,
    player_physics: &mut PlayerPhysics,
    player_state: &mut PlayerState,
    break_blocks: bool,
    game_mode: GameMode,
    damage_multiplier: f32,
) -> Vec<(i32, i32, i32)> {
    let cx = center.x.floor() as i32;
    let cy = center.y.floor() as i32;
    let cz = center.z.floor() as i32;
    let r_ceil = radius.ceil() as i32;

    let mut dirty_chunks = std::collections::HashSet::new();
    let mut blocks_removed = Vec::new();

    if break_blocks {
        // 1. Break blocks in radius
        for x in (cx - r_ceil)..=(cx + r_ceil) {
            for y in (cy - r_ceil)..=(cy + r_ceil) {
                for z in (cz - r_ceil)..=(cz + r_ceil) {
                    let dx = x as f32 + 0.5 - center.x;
                    let dy = y as f32 + 0.5 - center.y;
                    let dz = z as f32 + 0.5 - center.z;
                    if dx * dx + dy * dy + dz * dz <= radius * radius {
                        let block = chunk_manager.get_block(x, y, z);
                        if block != crate::world::BlockType::Air
                            && block != crate::world::BlockType::Bedrock
                        {
                            chunk_manager.set_block(x, y, z, crate::world::BlockType::Air);
                            blocks_removed.push((x, y, z, block));
                        }
                    }
                }
            }
        }

        // 2. Recalculate lighting for affected spots. Unsupported plants and
        // snow broken above the blast are part of the returned authoritative
        // mutation list too.
        let mut unsupported_removed = Vec::new();
        for &(x, y, z, old_block) in &blocks_removed {
            crate::lighting::update_sky_light_after_removed(
                chunk_manager,
                x,
                y,
                z,
                &mut dirty_chunks,
            );
            crate::lighting::update_block_light_after_removed(
                chunk_manager,
                x,
                y,
                z,
                old_block.properties().light_emission,
                &mut dirty_chunks,
            );

            mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
            chunk_manager.check_and_break_unsupported_above(
                x,
                y,
                z,
                &mut dirty_chunks,
                |pos, block| unsupported_removed.push((pos.0, pos.1, pos.2, block)),
            );
        }
        blocks_removed.extend(unsupported_removed);

        dirty_meshes.extend(dirty_chunks);
    }

    // 3. Player damage and knockback
    if game_mode != GameMode::Creative {
        let dist = center.distance(player_physics.position);
        if dist < 5.0 {
            let dmg = calculate_explosion_damage(center, player_physics.position);
            if dmg > 0.0 {
                // Inflict damage using player's existing interface
                player_state.take_damage(
                    dmg * damage_multiplier,
                    crate::player::DamageSource::Explosion,
                );
                let dir = (player_physics.position - center).normalize_or_zero();
                player_physics.velocity += dir * 12.0 + Vec3::new(0.0, 5.0, 0.0);
            }
        }
    }

    blocks_removed
        .into_iter()
        .map(|(x, y, z, _)| (x, y, z))
        .collect()
}

fn is_under_sun(
    chunk_manager: &ChunkManager,
    pos: Vec3,
    sky_light_level: u8,
    is_raining: bool,
) -> bool {
    if is_raining || sky_light_level <= 10 {
        return false;
    }
    let mx = pos.x.floor() as i32;
    let my = pos.y.floor() as i32;
    let mz = pos.z.floor() as i32;

    // Check if foot or eye block is water
    let feet_block = chunk_manager.get_block(mx, my, mz);
    let head_block = chunk_manager.get_block(mx, my + 1, mz);
    if feet_block == crate::world::BlockType::Water || head_block == crate::world::BlockType::Water
    {
        return false;
    }

    if chunk_manager.get_sky_light(mx, my, mz) < 12 {
        return false;
    }

    // Check if there is any solid block above
    for y in (my + 1)..320 {
        if chunk_manager.get_block(mx, y, mz).properties().is_solid {
            return false;
        }
    }
    true
}

fn get_highest_solid_y(chunk_manager: &ChunkManager, x: i32, z: i32) -> Option<i32> {
    let height = chunk_manager.dimension.height();
    for y in (height.min_y..height.max_y_exclusive()).rev() {
        if chunk_manager.get_block(x, y, z).properties().is_solid {
            return Some(y);
        }
    }
    None
}

pub fn spawn_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &ChunkManager,
    player_pos: Vec3,
    sky_light_level: u8,
    time: f32,
) {
    // Limit total hostile mobs to prevent lag
    if entity_manager.count_hostile() >= 20 {
        return;
    }

    // Use time-varying seed so RNG produces different results each frame
    let time_bits = (time * 1000.0) as u32;
    let mut rng_seed = (player_pos.x.to_bits())
        .wrapping_mul(31)
        .wrapping_add(player_pos.z.to_bits())
        .wrapping_add(entity_manager.entities.len() as u32)
        .wrapping_add(time_bits.wrapping_mul(2654435761));

    let mut next_rand = || {
        rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
        (rng_seed / 65536) % 32768
    };

    // ~1% chance per frame to attempt a spawn
    if next_rand() % 100 != 0 {
        return;
    }

    let angle = (next_rand() % 360) as f32 * std::f32::consts::PI / 180.0;
    let dist = (24 + (next_rand() % 56)) as f32;
    let spawn_x = (player_pos.x + angle.cos() * dist) as i32;
    let spawn_z = (player_pos.z + angle.sin() * dist) as i32;

    if let Some(solid_y) = get_highest_solid_y(chunk_manager, spawn_x, spawn_z) {
        let spawn_y = solid_y + 1;
        let height = chunk_manager.dimension.height();
        if spawn_y >= height.min_y && spawn_y < height.max_y_exclusive() - 1 {
            if chunk_manager.get_block(spawn_x, spawn_y, spawn_z) == crate::world::BlockType::Air
                && chunk_manager.get_block(spawn_x, spawn_y + 1, spawn_z)
                    == crate::world::BlockType::Air
            {
                let block_light = chunk_manager.get_block_light(spawn_x, spawn_y, spawn_z);
                let effective_sky = if sky_light_level > 10 {
                    sky_light_level
                } else {
                    4
                };
                let total_light = effective_sky.max(block_light);

                if total_light <= 7 {
                    let r = next_rand() % 3;
                    let et = match r {
                        0 => EntityType::Zombie,
                        1 => EntityType::Skeleton,
                        _ => EntityType::Creeper,
                    };
                    entity_manager.spawn(
                        et,
                        Vec3::new(spawn_x as f32 + 0.5, spawn_y as f32, spawn_z as f32 + 0.5),
                    );
                    println!(
                        "[Debug] Spawned {:?} at ({}, {}, {})",
                        et, spawn_x, spawn_y, spawn_z
                    );
                }
            }
        }
    }
}

pub fn update_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &mut ChunkManager,
    dirty_meshes: &mut std::collections::HashSet<(i32, i32)>,
    player_physics: &mut PlayerPhysics,
    player_state: &mut PlayerState,
    game_mode: GameMode,
    sky_light_level: u8,
    is_raining: bool,
    dt: f32,
    audio_manager: &mut crate::audio::AudioManager,
    listener_right: Vec3,
    player_invisible: bool,
    damage_multiplier: f32,
    is_host: bool,
) -> Vec<(i32, i32, i32)> {
    if !is_host {
        return Vec::new();
    }
    let player_pos = player_physics.position;

    // Global cleanup maintenance: retain/despawn is exhaustive by design;
    // it is not a radius/type candidate query.
    debug_assert!(crate::entity::is_global_entity_maintenance(
        EntityIterationKind::GlobalCleanup
    ));
    // Despawn out-of-bounds mobs
    entity_manager.retain(|entity| {
        if entity.entity_type == EntityType::RemotePlayer
            || entity.entity_type.is_projectile()
            || entity.entity_type.is_persistent()
        {
            true
        } else {
            entity.position.distance_squared(player_pos) <= 128.0 * 128.0
        }
    });

    // We collect arrows to spawn, creeper explosions to trigger, and sound effects to print.
    let mut arrows_to_spawn = Vec::new();
    let mut explosions = Vec::new();
    let mut player_hit = None;
    let mut moved_ids = Vec::new();

    // Global simulation maintenance: each locally simulated entity receives
    // one physics/AI tick. Candidate targeting is distance-gated in this pass
    // and does not scan a second entity collection.
    debug_assert!(crate::entity::is_global_entity_maintenance(
        EntityIterationKind::GlobalSimulation
    ));
    for entity in &mut entity_manager.entities {
        if entity.entity_type == EntityType::RemotePlayer {
            continue;
        }
        // Invulnerable frame countdown
        if entity.invulnerable_time > 0.0 {
            entity.invulnerable_time = (entity.invulnerable_time - dt).max(0.0);
        }

        if matches!(
            entity.entity_type,
            EntityType::SplashPotion
                | EntityType::Blaze
                | EntityType::Piglin
                | EntityType::Husk
                | EntityType::Shulker
                | EntityType::Enderman
                | EntityType::EnderDragon
                | EntityType::Wither
                | EntityType::EndCrystal
                | EntityType::WitherSkull
                | EntityType::DragonBreath
        ) {
            continue;
        }

        // Apply physical update
        let position_before_physics = entity.position;
        entity.update_physics(dt, chunk_manager);
        if entity.position != position_before_physics {
            moved_ids.push(entity.id);
        }

        let is_in_water = {
            let mx = entity.position.x.floor() as i32;
            let my = entity.position.y.floor() as i32;
            let mz = entity.position.z.floor() as i32;
            let feet_block = chunk_manager.get_block(mx, my, mz);
            let head_block = chunk_manager.get_block(mx, my + 1, mz);
            feet_block == crate::world::BlockType::Water
                || head_block == crate::world::BlockType::Water
        };

        if is_in_water || is_raining {
            entity.fire_aspect_timer = 0.0;
            entity.burn_timer = 0.0;
            entity.burn_damage_timer = 0.0;
        } else if (entity.entity_type == EntityType::Zombie
            || entity.entity_type == EntityType::Skeleton)
            && is_under_sun(chunk_manager, entity.position, sky_light_level, is_raining)
        {
            entity.fire_aspect_timer = entity.fire_aspect_timer.max(8.0);
        }

        if entity.fire_aspect_timer > 0.0 {
            entity.fire_aspect_timer = (entity.fire_aspect_timer - dt).max(0.0);
            entity.burn_damage_timer += dt;
            if entity.burn_damage_timer >= 1.0 {
                entity.burn_damage_timer -= 1.0;
                entity.health -= 1.0;
            }
        } else {
            entity.burn_damage_timer = 0.0;
        }

        if entity.entity_type == EntityType::Arrow {
            // Check collision with solid blocks
            let ax = entity.position.x.floor() as i32;
            let ay = entity.position.y.floor() as i32;
            let az = entity.position.z.floor() as i32;
            if chunk_manager.get_block(ax, ay, az).properties().is_solid {
                // Stuck in wall, mark arrow dead
                entity.health = -1.0;
                continue;
            }

            // Check collision with player AABB
            let player_aabb = player_physics.get_aabb();
            if !entity.friendly_projectile && entity.get_aabb().intersects(&player_aabb) {
                player_hit = Some(PlayerHitEvent::from_projectile(4.0, entity.velocity));
                entity.health = -1.0; // Destroy arrow
            }
            continue;
        }

        // Dropped items only need physics; skip all hostile AI.
        if entity.entity_type == EntityType::DroppedItem {
            continue;
        }

        // AI decision logic
        let is_hostile = entity.entity_type == EntityType::Zombie
            || entity.entity_type == EntityType::Skeleton
            || entity.entity_type == EntityType::Creeper;

        let dist_sq = entity.position.distance_squared(player_pos);
        if is_hostile && dist_sq <= 256.0 && !player_invisible && game_mode != GameMode::Creative {
            entity.target_player = true;

            // Turn towards player
            let dir = player_pos - entity.position;
            entity.yaw = f32::atan2(dir.x, dir.z);

            let walk_dir = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();

            match entity.entity_type {
                EntityType::Zombie => {
                    // Chase player
                    let speed = 2.5;
                    entity.velocity.x = walk_dir.x * speed;
                    entity.velocity.z = walk_dir.z * speed;

                    // Obstacle jump check
                    let next_x = entity.position.x + walk_dir.x * 0.4;
                    let next_z = entity.position.z + walk_dir.z * 0.4;
                    let bx = next_x.floor() as i32;
                    let bz = next_z.floor() as i32;
                    let by = entity.position.y.floor() as i32;
                    if entity.on_ground
                        && chunk_manager.get_block(bx, by, bz).properties().is_solid
                        && !chunk_manager
                            .get_block(bx, by + 2, bz)
                            .properties()
                            .is_solid
                    {
                        entity.velocity.y = 8.0;
                    }

                    // Melee attack
                    if dist_sq <= 1.44 && entity.action_cooldown <= 0.0 {
                        player_hit = Some(PlayerHitEvent::from_attacker(3.0, entity.position));
                        entity.action_cooldown = 1.0; // Cooldown
                    }
                }
                EntityType::Skeleton => {
                    // Keep distance AI
                    let speed = 2.5;
                    if dist_sq < 64.0 {
                        // Back away
                        entity.velocity.x = -walk_dir.x * speed;
                        entity.velocity.z = -walk_dir.z * speed;
                    } else if dist_sq > 144.0 {
                        // Move closer
                        entity.velocity.x = walk_dir.x * speed;
                        entity.velocity.z = walk_dir.z * speed;
                    } else {
                        // Stop and shoot
                        entity.velocity.x = 0.0;
                        entity.velocity.z = 0.0;
                    }

                    // Obstacle jump check
                    if entity.velocity.length_squared() > 0.0 {
                        let walk_vel = entity.velocity.normalize_or_zero();
                        let next_x = entity.position.x + walk_vel.x * 0.4;
                        let next_z = entity.position.z + walk_vel.z * 0.4;
                        let bx = next_x.floor() as i32;
                        let bz = next_z.floor() as i32;
                        let by = entity.position.y.floor() as i32;
                        if entity.on_ground
                            && chunk_manager.get_block(bx, by, bz).properties().is_solid
                            && !chunk_manager
                                .get_block(bx, by + 2, bz)
                                .properties()
                                .is_solid
                        {
                            entity.velocity.y = 8.0;
                        }
                    }

                    let spawn_pos = entity.position + Vec3::new(0.0, 1.4, 0.0);
                    let mut shoot_dir =
                        (player_pos + Vec3::new(0.0, 1.0, 0.0) - spawn_pos).normalize_or_zero();
                    // Add slight gravity correction
                    shoot_dir.y += 0.08;
                    entity.pitch = shoot_dir.y.clamp(-1.0, 1.0).asin();

                    // Shooting arrows
                    if entity.action_cooldown <= 0.0 {
                        let arrow_vel = shoot_dir.normalize() * 18.0;

                        arrows_to_spawn.push((spawn_pos, arrow_vel));
                        entity.action_cooldown = 2.0; // Shooting cooldown

                        let listener_pos = player_physics.position + Vec3::new(0.0, 1.6, 0.0);
                        audio_manager.play_sound_3d(
                            crate::audio::SoundId::ArrowShoot,
                            spawn_pos,
                            listener_pos,
                            listener_right,
                        );
                    }
                }
                EntityType::Creeper => {
                    // Chase player
                    let speed = if entity.is_ignited { 0.0 } else { 3.0 };
                    entity.velocity.x = walk_dir.x * speed;
                    entity.velocity.z = walk_dir.z * speed;

                    // Obstacle jump check
                    if !entity.is_ignited {
                        let next_x = entity.position.x + walk_dir.x * 0.4;
                        let next_z = entity.position.z + walk_dir.z * 0.4;
                        let bx = next_x.floor() as i32;
                        let bz = next_z.floor() as i32;
                        let by = entity.position.y.floor() as i32;
                        if entity.on_ground
                            && chunk_manager.get_block(bx, by, bz).properties().is_solid
                            && !chunk_manager
                                .get_block(bx, by + 2, bz)
                                .properties()
                                .is_solid
                        {
                            entity.velocity.y = 8.0;
                        }
                    }

                    // Ignite countdown
                    if dist_sq <= 4.0 {
                        if !entity.is_ignited {
                            println!("[Debug] Creeper: ssssssssss...");
                            entity.is_ignited = true;
                            entity.action_cooldown = 1.5; // Fuse duration

                            let listener_pos = player_physics.position + Vec3::new(0.0, 1.6, 0.0);
                            audio_manager.play_sound_3d(
                                crate::audio::SoundId::CreeperIgnition,
                                entity.position,
                                listener_pos,
                                listener_right,
                            );
                        }
                    } else if dist_sq > 12.25 {
                        if entity.is_ignited {
                            println!("[Debug] Creeper: fuse defused.");
                            entity.is_ignited = false;
                            entity.action_cooldown = 0.0;
                        }
                    }

                    if entity.is_ignited {
                        entity.action_cooldown -= dt;
                        if entity.action_cooldown <= 0.0 {
                            // Trigger explosion!
                            explosions.push(entity.position);
                            entity.health = -1.0; // Destroy creeper
                        }
                    }
                }
                _ => {}
            }
        } else {
            entity.target_player = false;
            if entity.entity_type == EntityType::Creeper && entity.is_ignited {
                println!("[Debug] Creeper: fuse defused.");
                entity.is_ignited = false;
                entity.action_cooldown = 0.0;
            }
        }

        // Tick down cooldowns
        if entity.action_cooldown > 0.0 && !entity.is_ignited {
            entity.action_cooldown = (entity.action_cooldown - dt).max(0.0);
        }
    }

    // Spawn created arrows
    for (pos, vel) in arrows_to_spawn {
        let mut arrow = Entity::new(0, EntityType::Arrow, pos);
        arrow.velocity = vel;
        entity_manager.spawn(EntityType::Arrow, pos);
        // Fix the newly spawned arrow's velocity in the manager
        if let Some(new_arrow) = entity_manager.entities.last_mut() {
            new_arrow.velocity = vel;
        }
    }

    // Trigger explosions
    let mut blocks_removed = Vec::new();
    for exp_pos in explosions {
        blocks_removed.extend(explode(
            exp_pos,
            3.0, // radius
            chunk_manager,
            dirty_meshes,
            player_physics,
            player_state,
            is_host,
            game_mode,
            damage_multiplier,
        ));

        let listener_pos = player_physics.position + Vec3::new(0.0, 1.6, 0.0);
        audio_manager.play_sound_3d(
            crate::audio::SoundId::Explosion,
            exp_pos,
            listener_pos,
            listener_right,
        );
    }

    // Handle player taking damage
    if let Some(hit) = player_hit.filter(|_| game_mode != GameMode::Creative) {
        // Player is hit, apply damage and small knockback
        let died = player_state.take_damage(
            hit.amount * damage_multiplier,
            crate::player::DamageSource::Mob,
        );
        if died {
            println!("[Debug] Player died from mob attack!");
            player_state.is_dead = true;
            player_state.death_reason = Some(crate::player::DamageSource::Mob);
        } else {
            // Apply knockback
            let flat_dir = hit.horizontal_knockback_direction(player_pos);
            player_physics.velocity += flat_dir * 8.0 + Vec3::new(0.0, 3.0, 0.0);
        }
    }

    // Handle entity death drops and sound effects for dying living entities
    let mut items_to_drop = Vec::new();
    let mut death_sounds = Vec::new();

    // Indexed death-candidate query: only living type buckets participate;
    // projectiles/items/remote players are excluded without a full scan.
    let death_candidates = [
        EntityType::Zombie,
        EntityType::Skeleton,
        EntityType::Creeper,
        EntityType::Pig,
        EntityType::Cow,
        EntityType::Sheep,
        EntityType::Chicken,
        EntityType::Blaze,
        EntityType::Piglin,
        EntityType::Husk,
        EntityType::Shulker,
        EntityType::EnderDragon,
        EntityType::Wither,
        EntityType::Spider,
        EntityType::Slime,
        EntityType::Witch,
        EntityType::Drowned,
        EntityType::Ghast,
        EntityType::MagmaCube,
        EntityType::WitherSkeleton,
        EntityType::Wolf,
        EntityType::Cat,
        EntityType::Horse,
        EntityType::Bat,
        EntityType::Squid,
    ];
    let mut slimes_to_split = Vec::new();
    for entity in death_candidates
        .into_iter()
        .flat_map(|kind| entity_manager.get_entities_by_type(kind))
    {
        if entity.health <= 0.0 {
            death_sounds.push(entity.position);
            match entity.entity_type {
                EntityType::Zombie | EntityType::Drowned => {
                    items_to_drop.push((crate::inventory::Item::RottenFlesh, entity.position));
                }
                EntityType::Skeleton => {
                    items_to_drop.push((crate::inventory::Item::Bone, entity.position));
                    items_to_drop.push((crate::inventory::Item::Arrow, entity.position));
                }
                EntityType::Spider => {
                    items_to_drop.push((crate::inventory::Item::String, entity.position));
                    items_to_drop.push((crate::inventory::Item::SpiderEye, entity.position));
                }
                EntityType::Slime => {
                    items_to_drop.push((crate::inventory::Item::Slimeball, entity.position));
                    if entity.slime_size > 1 {
                        slimes_to_split.push((
                            EntityType::Slime,
                            entity.slime_size / 2,
                            entity.position,
                        ));
                    }
                }
                EntityType::Witch => {
                    items_to_drop.push((crate::inventory::Item::GlassBottle, entity.position));
                }
                EntityType::Ghast => {
                    items_to_drop.push((crate::inventory::Item::GhastTear, entity.position));
                }
                EntityType::MagmaCube => {
                    items_to_drop.push((crate::inventory::Item::MagmaCream, entity.position));
                    if entity.slime_size > 1 {
                        slimes_to_split.push((
                            EntityType::MagmaCube,
                            entity.slime_size / 2,
                            entity.position,
                        ));
                    }
                }
                EntityType::WitherSkeleton => {
                    items_to_drop.push((crate::inventory::Item::Bone, entity.position));
                    items_to_drop.push((crate::inventory::Item::Coal, entity.position));
                }
                EntityType::Wolf => {
                    items_to_drop.push((crate::inventory::Item::Bone, entity.position));
                }
                EntityType::Cat => {
                    items_to_drop.push((crate::inventory::Item::String, entity.position));
                }
                EntityType::Horse => {
                    items_to_drop.push((crate::inventory::Item::Leather, entity.position));
                }
                EntityType::Squid => {
                    items_to_drop.push((crate::inventory::Item::InkSac, entity.position));
                }
                _ => {}
            }
        }
    }

    let listener_pos = player_physics.position + Vec3::new(0.0, 1.6, 0.0);
    for death_pos in death_sounds {
        audio_manager.play_sound_3d(
            crate::audio::SoundId::PlayerDeath,
            death_pos,
            listener_pos,
            listener_right,
        );
    }

    // Clean up dead entities (health < 0 or health == 0)
    entity_manager.retain(should_retain_after_health_cleanup);

    for (kind, new_size, pos) in slimes_to_split {
        for i in 0..2 {
            let offset = Vec3::new((i as f32 - 0.5) * 0.5, 0.2, 0.0);
            let id = entity_manager.spawn(kind, pos + offset);
            if let Some(child) = entity_manager.get_by_id_mut(id) {
                child.slime_size = new_size;
                child.size = Vec3::new(0.5, 0.5, 0.5) * new_size as f32;
                child.max_health = (new_size as f32) * 4.0;
                child.health = child.max_health;
            }
        }
    }

    // Spawn dropped items for dead entities
    for (item, pos) in items_to_drop {
        let id = entity_manager.spawn(EntityType::DroppedItem, pos);
        if let Some(drop) = entity_manager.get_by_id_mut(id) {
            drop.dropped_item = Some(item);
            drop.velocity = Vec3::new(0.0, 2.0, 0.0);
            drop.pickup_cooldown = 0.5;
        }
    }

    entity_manager.sync_entity_positions(&moved_ids);
    blocks_removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_cleanup_removes_zero_hp_living_entities_but_preserves_nonliving_lifecycles() {
        let mut entity_manager = EntityManager::new();
        let zombie_id = entity_manager.spawn(EntityType::Zombie, Vec3::ZERO);
        let dropped_item_id = entity_manager.spawn(EntityType::DroppedItem, Vec3::ZERO);
        let active_arrow_id = entity_manager.spawn(EntityType::Arrow, Vec3::ZERO);
        let expired_arrow_id = entity_manager.spawn(EntityType::Arrow, Vec3::ZERO);
        let boss_owned_id = entity_manager.spawn(EntityType::Blaze, Vec3::ZERO);

        entity_manager.get_by_id_mut(zombie_id).unwrap().health = 0.0;
        entity_manager
            .get_by_id_mut(expired_arrow_id)
            .unwrap()
            .health = -1.0;
        entity_manager.get_by_id_mut(boss_owned_id).unwrap().health = 0.0;

        entity_manager.retain(should_retain_after_health_cleanup);

        assert!(!entity_manager
            .entities
            .iter()
            .any(|entity| entity.id == zombie_id));
        assert!(entity_manager
            .entities
            .iter()
            .any(|entity| entity.id == dropped_item_id));
        assert!(entity_manager
            .entities
            .iter()
            .any(|entity| entity.id == active_arrow_id));
        assert!(!entity_manager
            .entities
            .iter()
            .any(|entity| entity.id == expired_arrow_id));
        assert!(entity_manager
            .entities
            .iter()
            .any(|entity| entity.id == boss_owned_id));
    }

    #[test]
    fn test_explosion_damage() {
        let center = Vec3::new(0.0, 0.0, 0.0);

        // Exact center: maximum damage
        let d1 = calculate_explosion_damage(center, center);
        assert_eq!(d1, 25.0);

        // Distance = 2.0: damage = (5.0 - 2.0) * 5.0 = 15.0
        let d2 = calculate_explosion_damage(center, Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(d2, 15.0);

        // Distance = 5.5: 0 damage
        let d3 = calculate_explosion_damage(center, Vec3::new(5.5, 0.0, 0.0));
        assert_eq!(d3, 0.0);
    }

    #[test]
    fn explosion_reports_authoritative_block_removals_and_can_be_visual_only() {
        let mut manager = ChunkManager::new(1);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(2, 10, 2, crate::world::BlockType::Stone);
        let mut meshes = std::collections::HashSet::new();
        let mut physics = PlayerPhysics::new(Vec3::new(100.0, 100.0, 100.0));
        let mut player = PlayerState::new();
        let center = Vec3::new(2.5, 10.5, 2.5);

        let visual_only = explode(
            center,
            1.0,
            &mut manager,
            &mut meshes,
            &mut physics,
            &mut player,
            false,
            GameMode::Creative,
            0.0,
        );
        assert!(visual_only.is_empty());
        assert_eq!(manager.get_block(2, 10, 2), crate::world::BlockType::Stone);

        let authoritative = explode(
            center,
            1.0,
            &mut manager,
            &mut meshes,
            &mut physics,
            &mut player,
            true,
            GameMode::Creative,
            0.0,
        );
        assert!(authoritative.contains(&(2, 10, 2)));
        assert_eq!(manager.get_block(2, 10, 2), crate::world::BlockType::Air);
    }

    #[test]
    fn test_mob_yaw_faces_player() {
        let mob_pos = Vec3::new(0.0, 0.0, 0.0);
        let player_pos = Vec3::new(0.0, 0.0, 5.0); // Player is at +Z relative to mob
        let dir = player_pos - mob_pos;
        let yaw = f32::atan2(dir.x, dir.z);

        // Front face (+Z) in local coordinates transforms to (sin(yaw), 0, cos(yaw)) in world coordinates.
        let facing_dir = Vec3::new(yaw.sin(), 0.0, yaw.cos()).normalize_or_zero();
        let expected_dir = dir.normalize_or_zero();

        assert!((facing_dir.x - expected_dir.x).abs() < 1e-5);
        assert!((facing_dir.z - expected_dir.z).abs() < 1e-5);
    }

    #[test]
    fn player_knockback_uses_actual_attacker_regardless_of_entity_order() {
        fn run_update(decoy_first: bool) -> Vec3 {
            let mut entity_manager = EntityManager::new();
            if decoy_first {
                entity_manager.spawn(EntityType::DroppedItem, Vec3::new(10.0, 1.0, 0.0));
            }
            entity_manager.spawn(EntityType::Zombie, Vec3::new(-1.0, 1.0, 0.0));
            if !decoy_first {
                entity_manager.spawn(EntityType::DroppedItem, Vec3::new(10.0, 1.0, 0.0));
            }

            let mut chunk_manager = ChunkManager::new(1);
            let mut chunk_meshes = std::collections::HashSet::new();
            let mut player_physics = PlayerPhysics::new(Vec3::new(0.0, 1.0, 0.0));
            let mut player_state = PlayerState::new();
            let mut audio_manager = crate::audio::AudioManager::new();

            update_mobs(
                &mut entity_manager,
                &mut chunk_manager,
                &mut chunk_meshes,
                &mut player_physics,
                &mut player_state,
                GameMode::Survival,
                0,
                false,
                0.0,
                &mut audio_manager,
                Vec3::X,
                false,
                1.0,
                true,
            );
            player_physics.velocity
        }

        let decoy_first = run_update(true);
        let attacker_first = run_update(false);
        assert!(
            decoy_first.x > 0.0,
            "knockback must move away from attacker"
        );
        assert_eq!(decoy_first, attacker_first);
    }

    #[test]
    fn creative_mode_mobs_do_not_target_player() {
        let mut entity_manager = EntityManager::new();
        entity_manager.spawn(EntityType::Zombie, Vec3::new(2.0, 1.0, 0.0));
        entity_manager.spawn(EntityType::Creeper, Vec3::new(2.0, 1.0, 0.0));

        let mut chunk_manager = ChunkManager::new(1);
        let mut chunk_meshes = std::collections::HashSet::new();
        let mut player_physics = PlayerPhysics::new(Vec3::new(0.0, 1.0, 0.0));
        let mut player_state = PlayerState::new();
        let mut audio_manager = crate::audio::AudioManager::new();

        update_mobs(
            &mut entity_manager,
            &mut chunk_manager,
            &mut chunk_meshes,
            &mut player_physics,
            &mut player_state,
            GameMode::Creative,
            15,
            false,
            0.1,
            &mut audio_manager,
            Vec3::X,
            false,
            1.0,
            true,
        );

        for entity in &entity_manager.entities {
            assert!(
                !entity.target_player,
                "Mob of type {:?} targeted player in Creative mode!",
                entity.entity_type
            );
            assert!(!entity.is_ignited, "Creeper ignited in Creative mode!");
        }
    }

    #[test]
    fn remote_player_velocity_survives_mob_update() {
        let mut entity_manager = EntityManager::new();
        let remote_id = entity_manager.spawn(EntityType::RemotePlayer, Vec3::new(2.0, 1.0, 0.0));
        let expected_velocity = Vec3::new(4.0, 1.0, -2.0);
        entity_manager.get_by_id_mut(remote_id).unwrap().velocity = expected_velocity;

        let mut chunk_manager = ChunkManager::new(1);
        let mut chunk_meshes = std::collections::HashSet::new();
        let mut player_physics = PlayerPhysics::new(Vec3::ZERO);
        let mut player_state = PlayerState::new();
        let mut audio_manager = crate::audio::AudioManager::new();
        update_mobs(
            &mut entity_manager,
            &mut chunk_manager,
            &mut chunk_meshes,
            &mut player_physics,
            &mut player_state,
            GameMode::Creative,
            15,
            false,
            0.1,
            &mut audio_manager,
            Vec3::X,
            false,
            1.0,
            true,
        );

        let remote = entity_manager.get_by_id(remote_id).unwrap();
        assert_eq!(remote.velocity, expected_velocity);
    }

    #[test]
    fn test_daylight_exposure_and_water_extinguish() {
        let mut chunk_manager = ChunkManager::new(1);
        let chunk = crate::world::Chunk::new(0, 0);
        chunk_manager.chunks.insert((0, 0), chunk);

        let highest_y = (-64..320)
            .rev()
            .find(|&y| chunk_manager.get_block(8, y, 8).properties().is_solid)
            .unwrap_or(64);

        let zombie_pos = Vec3::new(8.0, (highest_y + 1) as f32, 8.0);
        for y in (highest_y + 1)..320 {
            chunk_manager.set_block(8, y, 8, crate::world::BlockType::Air);
            chunk_manager.set_sky_light(8, y, 8, 15);
        }
        // Exposed to sky (15), sky_light_level = 15, not raining, not in water
        assert!(is_under_sun(&chunk_manager, zombie_pos, 15, false));
        // Raining -> should not be exposed to sun
        assert!(!is_under_sun(&chunk_manager, zombie_pos, 15, true));
    }

    #[test]
    fn test_mob_burn_death_drops() {
        let mut entity_manager = EntityManager::new();
        let zombie_id = entity_manager.spawn(EntityType::Zombie, Vec3::new(0.0, 64.0, 0.0));
        if let Some(zombie) = entity_manager.get_by_id_mut(zombie_id) {
            zombie.health = 0.0;
            zombie.fire_aspect_timer = 1.0;
        }

        let mut chunk_manager = ChunkManager::new(1);
        let mut chunk_meshes = std::collections::HashSet::new();
        let mut player_physics = PlayerPhysics::new(Vec3::ZERO);
        let mut player_state = PlayerState::new();
        let mut audio_manager = crate::audio::AudioManager::new();

        update_mobs(
            &mut entity_manager,
            &mut chunk_manager,
            &mut chunk_meshes,
            &mut player_physics,
            &mut player_state,
            GameMode::Survival,
            15,
            false,
            0.1,
            &mut audio_manager,
            Vec3::X,
            false,
            1.0,
            true,
        );

        // Zombie should be dead and cleaned up
        assert!(!entity_manager.entities.iter().any(|e| e.id == zombie_id));

        // Dropped item RottenFlesh should exist
        let dropped_flesh = entity_manager.entities.iter().find(|e| {
            e.entity_type == EntityType::DroppedItem
                && e.dropped_item == Some(crate::inventory::Item::RottenFlesh)
        });
        assert!(dropped_flesh.is_some());
    }
}

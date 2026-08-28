//! Leftover renderer-owned world tick for launches without an embedded runtime.
//!
//! Live Singleplayer / Host never enter these methods: they tick
//! `ServerRuntime` via `tick_authority_boundary` and only keep presentation
//! (keys, sprint latch, footsteps, `update_chunks`). This module is a child of
//! `state` (`#[path]`) so it can see private `State` fields without making
//! leftover simulation authoritative. Compiles only under `cfg(test)` or
//! feature `legacy_owner`.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MeleeImpact {
    Invulnerable,
    Damaged { killed: bool },
}

pub(super) fn apply_melee_impact(
    entity: &mut crate::entity::Entity,
    direction: Vec3,
    damage: f32,
    knockback: f32,
    fire_level: u8,
) -> MeleeImpact {
    if entity.invulnerable_time > 0.0 {
        return MeleeImpact::Invulnerable;
    }

    if entity.entity_type == crate::entity::EntityType::EndCrystal {
        entity.health = 0.0;
    } else {
        entity.health -= damage;
    }
    entity.invulnerable_time = 0.4;
    entity.velocity += direction.normalize_or_zero() * knockback + Vec3::new(0.0, 3.0, 0.0);
    if fire_level > 0 {
        entity.fire_aspect_timer = entity.fire_aspect_timer.max(fire_level as f32 * 4.0);
    }

    MeleeImpact::Damaged {
        killed: entity.health <= 0.0,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PlayerKill {
    pub(super) entity_type: crate::entity::EntityType,
    pub(super) position: Vec3,
    pub(super) burning: bool,
    pub(super) has_wool: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PlayerKillRewards {
    pub(super) items: Vec<Item>,
    pub(super) experience: u32,
}

pub(super) fn claim_standard_player_kill(entity: &mut crate::entity::Entity) -> Option<PlayerKill> {
    if entity.health > 0.0
        || entity.player_kill_rewarded
        || !entity.entity_type.uses_standard_player_kill_rewards()
    {
        return None;
    }

    entity.player_kill_rewarded = true;
    Some(PlayerKill {
        entity_type: entity.entity_type,
        position: entity.position,
        burning: entity.burn_timer > 0.0 || entity.fire_aspect_timer > 0.0,
        has_wool: entity.has_wool,
    })
}

pub(super) fn apply_player_projectile_damage(
    entity: &mut crate::entity::Entity,
    damage: f32,
) -> Option<PlayerKill> {
    if !entity.is_player_projectile_target() || damage <= 0.0 {
        return None;
    }

    if entity.entity_type == crate::entity::EntityType::EndCrystal {
        entity.health = 0.0;
    } else {
        entity.health -= damage;
    }
    claim_standard_player_kill(entity)
}

pub(super) fn apply_player_splash_effect(
    entity: &mut crate::entity::Entity,
    potion: crate::brewing::PotionData,
) -> Option<PlayerKill> {
    if !entity.is_local_living_target() {
        return None;
    }

    match potion.kind {
        crate::brewing::PotionKind::Healing | crate::brewing::PotionKind::Regeneration => {
            entity.health = (entity.health + 4.0 * potion.level as f32).min(entity.max_health);
        }
        crate::brewing::PotionKind::Poison => {
            entity.health -= 2.0 * potion.level as f32;
        }
        crate::brewing::PotionKind::Slowness => entity.velocity *= 0.4,
        _ => {}
    }

    claim_standard_player_kill(entity)
}

pub(super) fn standard_player_kill_rewards(kill: PlayerKill, looting: u8) -> PlayerKillRewards {
    let mut items = Vec::new();
    for _ in 0..=(looting / 2) {
        match kill.entity_type {
            crate::entity::EntityType::Zombie => items.push(Item::RottenFlesh),
            crate::entity::EntityType::Skeleton => {
                items.push(Item::Bone);
                items.push(Item::Arrow);
                let mut rng_seed = (kill.position.x as u32)
                    .wrapping_mul(31)
                    .wrapping_add(kill.position.z as u32);
                rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                if ((rng_seed / 65536) % 32768) % 10 == 0 {
                    items.push(Item::Bow);
                }
            }
            crate::entity::EntityType::Creeper => items.push(Item::Gunpowder),
            crate::entity::EntityType::Pig => items.push(if kill.burning {
                Item::CookedPorkchop
            } else {
                Item::RawPorkchop
            }),
            crate::entity::EntityType::Cow => {
                items.push(Item::RawBeef);
                if (kill.position.x as u32).wrapping_mul(31) % 2 == 0 {
                    items.push(Item::Leather);
                }
            }
            crate::entity::EntityType::Sheep => {
                items.push(Item::RawMutton);
                if kill.has_wool {
                    items.push(Item::Wool);
                }
            }
            crate::entity::EntityType::Chicken => {
                items.push(Item::RawChicken);
                items.push(Item::Feather);
            }
            _ => {}
        }
    }

    let experience = match kill.entity_type {
        crate::entity::EntityType::Zombie
        | crate::entity::EntityType::Skeleton
        | crate::entity::EntityType::Creeper => 5,
        _ => 2,
    };
    PlayerKillRewards { items, experience }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GeneratedItemDestination {
    Inventory,
    Dropped,
    IgnoredAir,
}

pub(super) fn spawn_dropped_item_entity(
    entity_manager: &mut crate::entity::EntityManager,
    item: Item,
    position: Vec3,
    random_seed: u32,
) -> bool {
    if item == Item::Air {
        return false;
    }

    let id = entity_manager.spawn(crate::entity::EntityType::DroppedItem, position);
    let Some(entity) = entity_manager.entities.last_mut() else {
        return false;
    };
    entity.dropped_item = Some(item);

    let mut rng = random_seed.wrapping_add((id.wrapping_mul(2_654_435_761)) as u32);
    rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let vx = ((rng / 65_536) as f32 / 32_768.0 - 0.5) * 1.5;
    rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let vz = ((rng / 65_536) as f32 / 32_768.0 - 0.5) * 1.5;
    rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let vy = 2.0 + ((rng / 65_536) as f32 / 32_768.0);
    entity.velocity = Vec3::new(vx, vy, vz);
    entity.pickup_cooldown = 0.5;
    true
}

pub(super) fn store_or_drop_generated_item(
    inventory: &mut Inventory,
    entity_manager: &mut crate::entity::EntityManager,
    item: Item,
    position: Vec3,
    random_seed: u32,
) -> GeneratedItemDestination {
    if item == Item::Air {
        return GeneratedItemDestination::IgnoredAir;
    }
    if inventory.add_item(item) {
        return GeneratedItemDestination::Inventory;
    }

    let spawned = spawn_dropped_item_entity(entity_manager, item, position, random_seed);
    debug_assert!(spawned);
    GeneratedItemDestination::Dropped
}

pub(super) fn parse_command_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl State {
    /// Item-use completion (eat / drink / consume) for leftover world owners.
    pub(super) fn legacy_tick_item_use(&mut self) {
        if self.inventory.is_open
            || self.is_paused
            || self.is_chat_open
            || self.player_state.is_dead
        {
            self.player_state.using_item = None;
            return;
        }
        let Some(ref mut using) = self.player_state.using_item else {
            return;
        };
        using.ticks_held += 1;

        if using.action == crate::player::ItemUseAction::Eat
            || using.action == crate::player::ItemUseAction::Drink
        {
            if let Some(max_ticks) = using.max_ticks {
                if using.ticks_held >= max_ticks {
                    let item = using.item;
                    let slot = using.slot;
                    if using.action == crate::player::ItemUseAction::Eat {
                        if let Some(food_props) = item.food_properties() {
                            self.player_state.hunger =
                                (self.player_state.hunger + food_props.hunger).min(20.0);
                            self.player_state.saturation = (self.player_state.saturation
                                + food_props.saturation)
                                .min(self.player_state.hunger);
                            self.trigger_advancement(
                                crate::advancements::AdvancementTrigger::EatFood(item),
                            );
                            if let Some(ret) = food_props.return_item {
                                let _ = self.inventory.add_stack(ItemStack::new(ret, 1));
                            }
                        }
                    } else if using.action == crate::player::ItemUseAction::Drink {
                        if item == Item::MilkBucket {
                            self.potion_effects.active.clear();
                            if self.game_mode_policy().hunger_enabled {
                                let _ = self.inventory.add_stack(ItemStack::new(Item::Bucket, 1));
                            }
                        }
                    }

                    match slot {
                        crate::player::HandSlot::MainHand(_i) => {
                            self.inventory
                                .use_selected_item(self.game_mode == GameMode::Creative);
                        }
                        crate::player::HandSlot::OffHand => {
                            if self.game_mode_policy().hunger_enabled {
                                if let Some(ref mut offhand) = self.inventory.offhand {
                                    if offhand.count > 1 {
                                        offhand.count -= 1;
                                    } else {
                                        self.inventory.offhand = None;
                                    }
                                }
                            }
                        }
                    }

                    self.audio_manager
                        .play_sound(crate::audio::SoundId::UiClick);
                    self.player_state.using_item = None;
                }
            }
        }
    }

    /// Autosave, fluids, redstone/hoppers for leftover world owners.
    pub(super) fn legacy_tick_world_systems(&mut self, dt: f32) {
        let authoritative = true;
        self.autosave_timer += dt;
        if authoritative && self.autosave_timer >= 300.0 {
            self.autosave_timer = 0.0;
            if let Err(error) = self.trigger_background_save() {
                eprintln!("[Save] Could not enqueue autosave: {error}");
            }
        }

        self.water_tick_timer += dt;
        if authoritative && self.water_tick_timer >= 0.25 {
            self.water_tick_timer = 0.0;
            let lighting_started = Instant::now();
            let (mut dirty, mutations) =
                crate::fluid::tick_all_loaded_fluids(&mut self.chunk_manager, false, 2048);
            for mutation in mutations {
                let (x, y, z) = mutation.position;
                self.broadcast_block_change_with_raw(x, y, z, mutation.block, mutation.raw_fluid);
                self.check_and_break_unsupported_above(x, y, z, &mut dirty);
            }
            self.invalidate_chunk_meshes(dirty, DependencyReason::Fluid);
            let lighting_elapsed = lighting_started.elapsed();
            self.lighting_time_frame += lighting_elapsed;
            self.lighting_scopes_frame.record(
                crate::perf::LightingSource::Fluid as usize,
                lighting_elapsed,
            );
        }

        self.lava_tick_timer += dt;
        if authoritative && self.lava_tick_timer >= 1.5 {
            self.lava_tick_timer = 0.0;
            let lighting_started = Instant::now();
            let (mut dirty, mutations) =
                crate::fluid::tick_all_loaded_fluids(&mut self.chunk_manager, true, 512);
            for mutation in mutations {
                let (x, y, z) = mutation.position;
                self.broadcast_block_change_with_raw(x, y, z, mutation.block, mutation.raw_fluid);
                self.check_and_break_unsupported_above(x, y, z, &mut dirty);
            }
            self.invalidate_chunk_meshes(dirty, DependencyReason::Fluid);
            let lighting_elapsed = lighting_started.elapsed();
            self.lighting_time_frame += lighting_elapsed;
            self.lighting_scopes_frame.record(
                crate::perf::LightingSource::Fluid as usize,
                lighting_elapsed,
            );
        }

        if authoritative {
            self.redstone_tick_timer += dt;
        }
        let redstone_started = Instant::now();
        let mut redstone_steps = 0;
        while authoritative && self.redstone_tick_timer >= 0.05 && redstone_steps < 4 {
            self.redstone_tick_timer -= 0.05;
            redstone_steps += 1;
            let mut occupants = Vec::with_capacity(self.entity_manager.entities.len() + 1);
            occupants.push((
                self.player_physics.position.x.floor() as i32,
                self.player_physics.position.y.floor() as i32,
                self.player_physics.position.z.floor() as i32,
            ));
            occupants.extend(self.entity_manager.entities.iter().map(|entity| {
                (
                    entity.position.x.floor() as i32,
                    entity.position.y.floor() as i32,
                    entity.position.z.floor() as i32,
                )
            }));
            let update = self.redstone.tick(&mut self.chunk_manager, &occupants);
            let observer_pulses = update.observer_pulses as u64;
            self.apply_redstone_update(update);
            self.perf_counters.observer_pulses = self
                .perf_counters
                .observer_pulses
                .saturating_add(observer_pulses);
            self.update_hopper_power_states();
            let hopper_result = crate::world_tick::tick_all_loaded_hoppers_with_entities(
                &mut self.chunk_manager,
                Some(&mut self.entity_manager),
                64,
            );
            self.perf_counters.hopper_transfers = self
                .perf_counters
                .hopper_transfers
                .saturating_add(hopper_result.transfers as u64);
            self.perf_counters.hopper_container_checks = self
                .perf_counters
                .hopper_container_checks
                .saturating_add(hopper_result.container_checks as u64);
            if hopper_result.budget_exhausted {
                self.perf_counters.hopper_budget_exhausted =
                    self.perf_counters.hopper_budget_exhausted.saturating_add(1);
            }
            for (x, y, z) in hopper_result.changed_positions {
                self.redstone
                    .mark_container_changed(&self.chunk_manager, (x, y, z));
                let entity = self.chunk_manager.get_block_entity(x, y, z).cloned();
                self.broadcast_block_entity_delta(x, y, z, entity);
            }
        }
        self.perf_counters.redstone_scheduled_backlog = self.redstone.scheduled_len() as u64;
        self.perf_counters.observer_pending_pulses = self
            .chunk_manager
            .chunks
            .values()
            .flat_map(|chunk| chunk.iter_block_entities())
            .filter(|(_, entity)| {
                matches!(
                    entity,
                    crate::block_entity::BlockEntity::Observer(observer)
                        if observer.pending_pulse > 0
                )
            })
            .count() as u64;
        if redstone_steps == 4 {
            self.redstone_tick_timer = self.redstone_tick_timer.min(0.05);
        }
        let redstone_elapsed = redstone_started.elapsed();
        self.perf_recorder
            .record(crate::perf::ScopeId::Redstone, redstone_elapsed);
        self.lighting_time_frame += redstone_elapsed;
        self.lighting_scopes_frame.record(
            crate::perf::LightingSource::Redstone as usize,
            redstone_elapsed,
        );
    }

    /// Night skip for leftover world owners. Called after the sleep timer and
    /// before pickup, matching the pre-split `tick_simulation` order.
    pub(super) fn legacy_tick_night_skip(&mut self) {
        let mut total_overworld_players = 0;
        let mut sleeping_overworld_players = 0;

        if self.current_dimension == crate::dimension::Dimension::Overworld
            && !self.player_state.is_dead
        {
            total_overworld_players += 1;
            if self.player_state.is_sleeping {
                sleeping_overworld_players += 1;
            }
        }

        for (_id, remote) in &self.remote_players {
            if remote.dimension == crate::dimension::Dimension::Overworld && !remote.is_dead {
                total_overworld_players += 1;
                if remote.is_sleeping {
                    sleeping_overworld_players += 1;
                }
            }
        }

        let required_sleepers =
            ((total_overworld_players * self.world_rules.sleeping_percentage as usize + 99) / 100)
                .max(1);
        if self.world_rules.do_daylight_cycle
            && total_overworld_players > 0
            && sleeping_overworld_players >= required_sleepers
        {
            let ready_to_skip = if self.player_state.is_sleeping {
                self.player_state.sleep_timer >= 5.0
            } else {
                true
            };
            if ready_to_skip {
                let current_day = self.world_time.ticks / 24000;
                self.world_time.ticks = (current_day + 1) * 24000 + 1000;
                self.weather.clear_weather();

                if self.player_state.is_sleeping {
                    let bed_pos = self.player_state.bed_pos.unwrap_or([
                        self.player_physics.position.x as i32,
                        self.player_physics.position.y as i32,
                        self.player_physics.position.z as i32,
                    ]);
                    let (safe_p, _) = crate::world::find_safe_spawn_position(
                        &self.chunk_manager,
                        (bed_pos[0], bed_pos[1], bed_pos[2]),
                    );
                    self.player_physics.position = safe_p;
                    self.player_state.is_sleeping = false;
                    self.player_state.sleep_timer = 0.0;
                    self.player_state.bed_pos = None;
                }

                let remote_ids: Vec<u64> = self.remote_players.keys().copied().collect();
                for id in remote_ids {
                    if let Some(remote) = self.remote_players.get_mut(&id) {
                        if remote.is_sleeping {
                            remote.is_sleeping = false;
                            remote.bed_pos = None;
                            self.network.broadcast_sleep_state_sync(id, false);
                        }
                    }
                }

                self.broadcast_time_sync();
                println!("[Game] Night skipped! Woke up. Good morning!");
            }
        }
    }

    /// Leaf decay random ticks. Called after lava damage and before cactus,
    /// while `total_time` is still the previous-frame clock.
    pub(super) fn legacy_tick_leaf_decay(&mut self) {
        // Leaf Decay Random Ticks (30 random ticks per 20 Hz sim tick)
        let chunk_keys: Vec<(i32, i32)> = self.chunk_manager.chunks.keys().cloned().collect();
        if !chunk_keys.is_empty() {
            let mut rng_seed = (self.total_time * 1000.0) as u32;
            let mut next_rand = |max: u32| -> u32 {
                rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                ((rng_seed / 65536) % 32768) % max
            };

            for _ in 0..30 {
                let chunk_idx = next_rand(chunk_keys.len() as u32) as usize;
                let (cx, cz) = chunk_keys[chunk_idx];

                let rx = next_rand(16) as i32;
                let rz = next_rand(16) as i32;
                let ry = next_rand(120) as i32 + 40;

                let wx = cx * 16 + rx;
                let wz = cz * 16 + rz;

                let block = self.chunk_manager.get_block(wx, ry, wz);
                if block == BlockType::OakLeaves
                    || block == BlockType::BirchLeaves
                    || block == BlockType::SpruceLeaves
                {
                    let mut queue = std::collections::VecDeque::new();
                    let mut visited = std::collections::HashSet::new();
                    queue.push_back((wx, ry, wz, 0));
                    visited.insert((wx, ry, wz));

                    let mut found_log = false;
                    while let Some((bx, by, bz, dist)) = queue.pop_front() {
                        let b = self.chunk_manager.get_block(bx, by, bz);
                        if b == BlockType::OakLog
                            || b == BlockType::BirchLog
                            || b == BlockType::SpruceLog
                        {
                            found_log = true;
                            break;
                        }
                        if dist < 4 {
                            for (dx, dy, dz) in &[
                                (1, 0, 0),
                                (-1, 0, 0),
                                (0, 1, 0),
                                (0, -1, 0),
                                (0, 0, 1),
                                (0, 0, -1),
                            ] {
                                let nx = bx + dx;
                                let ny = by + dy;
                                let nz = bz + dz;
                                let neighbor_b = self.chunk_manager.get_block(nx, ny, nz);
                                let is_leaf = neighbor_b == BlockType::OakLeaves
                                    || neighbor_b == BlockType::BirchLeaves
                                    || neighbor_b == BlockType::SpruceLeaves;
                                if (is_leaf
                                    || neighbor_b == BlockType::OakLog
                                    || neighbor_b == BlockType::BirchLog
                                    || neighbor_b == BlockType::SpruceLog)
                                    && visited.insert((nx, ny, nz))
                                {
                                    queue.push_back((nx, ny, nz, dist + 1));
                                }
                            }
                        }
                    }

                    if !found_log {
                        self.chunk_manager.set_block(wx, ry, wz, BlockType::Air);
                        let mut dirty_chunks = std::collections::HashSet::new();
                        crate::lighting::update_sky_light_after_removed(
                            &mut self.chunk_manager,
                            wx,
                            ry,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
                        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Mob);
                        self.broadcast_block_change(wx, ry, wz, BlockType::Air);
                    }
                }
            }
        }
    }

    /// Leftover oxygen / starvation. Called after cactus and before
    /// `total_time` advances.
    pub(super) fn legacy_tick_oxygen(&mut self, dt: f32) {
        // Update player state timers & starvation
        let px = self.player_physics.position.x.floor() as i32;
        let pz = self.player_physics.position.z.floor() as i32;
        let block_at_eyes = self.chunk_manager.get_block(
            px,
            (self.player_physics.position.y + 1.62).floor() as i32,
            pz,
        );
        let is_underwater = block_at_eyes == BlockType::Water;
        let respiration_level: u8 = self
            .inventory
            .armor
            .iter()
            .flatten()
            .map(|stack| {
                stack
                    .enchantments
                    .level_of(crate::enchantment::Enchantment::Respiration(1))
            })
            .sum();
        let water_breathing = self.potion_effects.has_water_breathing();
        let oxygen_rate = 1.0 / (1.0 + respiration_level as f32);
        if self.game_mode_policy().can_take_damage {
            if let Some((dmg, src)) = self.player_state.update_with_oxygen_rate(
                dt,
                is_underwater && !water_breathing,
                oxygen_rate,
            ) {
                self.take_damage(dmg, src);
            }
        }
    }

    /// Leftover hostile/passive mobs and random ticks. Called after `total_time`
    /// advances so leftover spawn seeds stay on the same clock as before.
    pub(super) fn legacy_tick_owned_world(&mut self, dt: f32) {
        let authoritative = true;
        if authoritative {
            let hostile_mobs_started = Instant::now();
            if self.difficulty == Difficulty::Peaceful {
                self.entity_manager
                    .entities
                    .retain(|entity| !entity.entity_type.is_hostile());
            } else if self.world_rules.do_mob_spawning
                && self.current_dimension == crate::dimension::Dimension::Overworld
                // Phantom entities are not part of the current entity registry;
                // until they are added, the insomnia rule gates the night-time
                // ambient hostile spawn budget (daylight spawns remain intact).
                && (self.world_rules.do_insomnia || self.world_time.sky_light_level() > 7)
            {
                crate::mob::spawn_mobs(
                    &mut self.entity_manager,
                    &self.chunk_manager,
                    self.player_physics.position,
                    self.world_time.sky_light_level(),
                    self.total_time,
                );
            }

            if self.difficulty != Difficulty::Peaceful
                && self.world_rules.do_mob_spawning
                && self.game_mode_policy().can_target_mobs
            {
                self.boss_maintenance_timer -= dt;
                if self.boss_maintenance_timer <= 0.0 {
                    crate::boss::ensure_dimension_entities(
                        self.current_dimension,
                        &mut self.entity_manager,
                        &self.chunk_manager,
                        self.player_physics.position,
                        self.total_time,
                    );
                    self.boss_maintenance_timer = 1.0;
                }
                let boss_events = crate::boss::update_dimension_entities(
                    self.current_dimension,
                    &mut self.entity_manager,
                    &self.chunk_manager,
                    self.player_physics.position,
                    Vec3::new(
                        self.camera.yaw.cos() * self.camera.pitch.cos(),
                        self.camera.pitch.sin(),
                        self.camera.yaw.sin() * self.camera.pitch.cos(),
                    ),
                    dt,
                    self.game_mode,
                );
                self.apply_boss_events(boss_events);
            }

            // Update mobs
            self.update_player_projectiles(dt);
            self.perf_recorder.record(
                crate::perf::ScopeId::HostileMobs,
                hostile_mobs_started.elapsed(),
            );

            // Passive mobs (daytime spawn)
            let passive_mobs_started = Instant::now();
            if self.world_rules.do_mob_spawning
                && self.current_dimension == crate::dimension::Dimension::Overworld
            {
                crate::passive_mob::spawn_passive_mobs(
                    &mut self.entity_manager,
                    &self.chunk_manager,
                    self.player_physics.position,
                    self.world_time.sky_light_level(),
                    self.total_time,
                );
            }
            self.perf_recorder.record(
                crate::perf::ScopeId::PassiveMobs,
                passive_mobs_started.elapsed(),
            );
            let (mut mutations, _stats) = crate::world_tick::sample_all_loaded_random_ticks(
                &self.chunk_manager,
                self.world_seed as u64,
                self.world_time.ticks,
                self.current_dimension as u8,
                512,
            );
            if !self.world_rules.do_fire_tick {
                mutations.retain(|mutation| {
                    self.chunk_manager
                        .get_block(mutation.pos.0, mutation.pos.1, mutation.pos.2)
                        != BlockType::Fire
                });
            }
            if !mutations.is_empty() {
                if let Ok(outcome) =
                    crate::world_mutation::apply_batch(&mut self.chunk_manager, mutations)
                {
                    for res in &outcome.mutations {
                        self.broadcast_block_change(res.pos.0, res.pos.1, res.pos.2, res.new_block);
                        self.chunk_manager.set_block_state(
                            res.pos.0,
                            res.pos.1,
                            res.pos.2,
                            res.new_state,
                        );
                    }
                }
            }
        }
    }

    pub(super) fn store_or_drop_generated_item(&mut self, item: Item, position: Vec3) {
        let _ = store_or_drop_generated_item(
            &mut self.inventory,
            &mut self.entity_manager,
            item,
            position,
            self.total_time.to_bits(),
        );
    }

    pub(super) fn settle_standard_player_kill(&mut self, kill: PlayerKill, looting: u8) {
        if matches!(self.game_mode, GameMode::Creative | GameMode::Spectator) {
            return;
        }

        let rewards = standard_player_kill_rewards(kill, looting);
        for item in rewards.items {
            self.store_or_drop_generated_item(item, kill.position);
        }
        self.player_state.add_experience(rewards.experience);
    }

    pub(super) fn update_player_projectiles(&mut self, dt: f32) {
        let mut player_kills = Vec::new();
        let mut splashes = Vec::new();
        for projectile in &mut self.entity_manager.entities {
            if projectile.entity_type != crate::entity::EntityType::SplashPotion {
                continue;
            }
            projectile.update_physics(dt, &self.chunk_manager);
            projectile.life_time -= dt;
            let pos = projectile.position;
            let hit_block = self
                .chunk_manager
                .get_block(
                    pos.x.floor() as i32,
                    pos.y.floor() as i32,
                    pos.z.floor() as i32,
                )
                .properties()
                .is_solid;
            if hit_block || projectile.life_time <= 0.0 {
                if let Some(potion) = projectile.potion {
                    splashes.push((pos, potion));
                }
                projectile.health = -1.0;
            }
        }

        for (position, potion) in splashes {
            if position.distance(self.player_physics.position) <= 4.0 {
                let healing = self.potion_effects.apply(potion);
                self.player_state.health =
                    (self.player_state.health + healing).min(self.player_state.max_health);
            }
            let ids: Vec<u64> = self
                .entity_manager
                .query_radius(position, 4.0)
                .map(|e| e.id)
                .collect();
            for id in ids {
                let Some(entity) = self.entity_manager.get_by_id_mut(id) else {
                    continue;
                };
                if let Some(kill) = apply_player_splash_effect(entity, potion) {
                    player_kills.push(kill);
                }
            }
        }

        let mut hits = Vec::new();
        let projectile_ids: Vec<u64> = self
            .entity_manager
            .get_entities_by_type(crate::entity::EntityType::Arrow)
            .filter(|p| p.friendly_projectile)
            .map(|p| p.id)
            .collect();
        for projectile_id in projectile_ids {
            let Some(projectile) = self.entity_manager.get_by_id(projectile_id) else {
                continue;
            };
            let target_ids: Vec<u64> = self
                .entity_manager
                .query_radius(projectile.position, 2.0)
                .map(|t| t.id)
                .collect();
            for target_id in target_ids {
                let Some(target) = self.entity_manager.get_by_id(target_id) else {
                    continue;
                };
                if target.id != projectile.id
                    && target.is_player_projectile_target()
                    && projectile.get_aabb().intersects(&target.get_aabb())
                {
                    hits.push((projectile.id, target.id, projectile.projectile_damage));
                    break;
                }
            }
        }
        for (projectile_id, target_id, damage) in hits {
            if let Some(target) = self.entity_manager.get_by_id_mut(target_id) {
                if let Some(kill) = apply_player_projectile_damage(target, damage) {
                    player_kills.push(kill);
                }
            }
            if let Some(projectile) = self.entity_manager.get_by_id_mut(projectile_id) {
                projectile.health = -1.0;
            }
        }
        for kill in player_kills {
            self.settle_standard_player_kill(kill, 0);
        }
        self.entity_manager.retain(|entity| {
            entity.health >= 0.0
                || matches!(
                    entity.entity_type,
                    crate::entity::EntityType::Blaze
                        | crate::entity::EntityType::Piglin
                        | crate::entity::EntityType::Husk
                        | crate::entity::EntityType::Shulker
                        | crate::entity::EntityType::EnderDragon
                        | crate::entity::EntityType::Wither
                        | crate::entity::EntityType::EndCrystal
                        | crate::entity::EntityType::RemotePlayer
                )
        });
    }

    pub(super) fn apply_boss_events(&mut self, events: crate::boss::BossEvents) {
        let is_legacy_owner = self.presentation_topology().is_legacy_owner();
        for hit in events.player_damage {
            let can_receive_impact = is_legacy_owner
                && self.game_mode != GameMode::Creative
                && !self.player_state.is_dead
                && self.player_state.invulnerable_time <= 0.0;
            self.take_damage(hit.amount, DamageSource::Mob);
            if can_receive_impact && hit.knockback.length_squared() > 0.0 {
                self.player_physics.velocity += hit.knockback;
            }
        }
        for effect in events.apply_wither {
            self.wither_effect_timer = self.wither_effect_timer.max(effect.duration);
        }
        for explosion in events.explosions {
            if is_legacy_owner {
                let remove_entity_ids: Vec<u64> = self
                    .entity_manager
                    .entities
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.entity_type,
                            crate::entity::EntityType::DroppedItem
                                | crate::entity::EntityType::ExperienceOrb
                        ) && e.position.distance(explosion.position) <= explosion.radius
                    })
                    .map(|e| e.id)
                    .collect();
                for id in remove_entity_ids {
                    self.entity_manager.remove_by_id(id);
                }
            }
            if explosion.break_blocks && is_legacy_owner {
                let mut dirty_meshes = std::collections::HashSet::new();
                let removed = crate::mob::explode(
                    explosion.position,
                    explosion.radius,
                    &mut self.chunk_manager,
                    &mut dirty_meshes,
                    &mut self.player_physics,
                    &mut self.player_state,
                    true,
                    GameMode::Creative,
                    0.0,
                );
                self.invalidate_chunk_meshes(dirty_meshes, DependencyReason::Mob);
                for (x, y, z) in removed {
                    self.broadcast_block_change(x, y, z, BlockType::Air);
                }
            }
            self.audio_manager
                .play_sound(crate::audio::SoundId::Explosion);
        }
        for drop in events.drops {
            for _ in 0..drop.count {
                self.spawn_dropped_item(drop.item, drop.position);
            }
        }
        let changes: Vec<_> = events
            .block_placements
            .into_iter()
            .map(|placement| (placement.position, placement.block))
            .collect();
        if is_legacy_owner {
            self.apply_block_changes(&changes);
        }
        if events.dragon_completion.is_some() {
            self.end_flash_time = 0.45;
            self.audio_manager
                .play_sound(crate::audio::SoundId::Explosion);
            self.player_state.add_experience(120);
            if is_legacy_owner {
                self.apply_block_changes(&[
                    ((96, 75, 0), BlockType::EndGateway),
                    ((1000, 65, 0), BlockType::EndGateway),
                ]);
            }
        }
    }

    pub(super) fn safe_dimension_spawn_y(&mut self, x: i32, z: i32) -> f32 {
        let top = if self.current_dimension == crate::dimension::Dimension::Nether {
            120
        } else {
            180
        };
        for y in (2..=top).rev() {
            if self
                .chunk_manager
                .get_block(x, y - 1, z)
                .properties()
                .is_solid
                && self
                    .chunk_manager
                    .get_block(x, y, z)
                    .properties()
                    .is_passable
                && self
                    .chunk_manager
                    .get_block(x, y + 1, z)
                    .properties()
                    .is_passable
            {
                return y as f32;
            }
        }
        let floor = match self.current_dimension {
            crate::dimension::Dimension::Nether => BlockType::Netherrack,
            crate::dimension::Dimension::End => BlockType::EndStone,
            crate::dimension::Dimension::Overworld => BlockType::Stone,
        };
        self.apply_block_changes(&[
            ((x, 63, z), floor),
            ((x, 64, z), BlockType::Air),
            ((x, 65, z), BlockType::Air),
        ]);
        64.0
    }

    pub(super) fn build_linked_nether_portal(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        spawn_y: i32,
    ) -> Vec3 {
        let base_x = chunk_x * CHUNK_WIDTH as i32 + 6;
        let base_z = chunk_z * CHUNK_DEPTH as i32 + 8;
        let height = self.chunk_manager.dimension.height();
        let clamp_min = height.min_y + 1;
        let clamp_max = height.max_y_exclusive() - 5;
        let base_y = (spawn_y - 1).clamp(clamp_min, clamp_max);
        let mut changes = Vec::new();
        for x in base_x..=base_x + 3 {
            changes.push(((x, base_y, base_z), BlockType::Obsidian));
            changes.push(((x, base_y + 4, base_z), BlockType::Obsidian));
        }
        for y in base_y + 1..=base_y + 3 {
            changes.push(((base_x, y, base_z), BlockType::Obsidian));
            changes.push(((base_x + 3, y, base_z), BlockType::Obsidian));
            changes.push(((base_x + 1, y, base_z), BlockType::NetherPortal));
            changes.push(((base_x + 2, y, base_z), BlockType::NetherPortal));
        }
        self.apply_block_changes(&changes);
        Vec3::new(
            base_x as f32 + 1.5,
            base_y as f32 + 1.0,
            base_z as f32 + 0.5,
        )
    }

    pub(super) fn apply_weather_block_change(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        block: BlockType,
    ) {
        if !self.presentation_topology().is_legacy_owner() {
            return;
        }
        let old = self.chunk_manager.get_block(wx, wy, wz);
        if old == block {
            return;
        }
        self.chunk_manager.set_block(wx, wy, wz, block);
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (wx, wy, wz),
            crate::redstone::Direction::North,
        );

        let old_properties = old.properties();
        let new_properties = block.properties();
        let mut dirty_chunks = std::collections::HashSet::new();
        if old_properties.is_solid != new_properties.is_solid {
            if new_properties.is_solid {
                crate::lighting::update_sky_light_after_placed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    &mut dirty_chunks,
                );
            } else {
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    &mut dirty_chunks,
                );
            }
        }
        if old_properties.light_emission != new_properties.light_emission {
            crate::lighting::update_block_light_after_removed(
                &mut self.chunk_manager,
                wx,
                wy,
                wz,
                old_properties.light_emission,
                &mut dirty_chunks,
            );
            if new_properties.light_emission > 0 {
                crate::lighting::update_block_light_after_placed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    new_properties.light_emission,
                    &mut dirty_chunks,
                );
            }
        }
        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Weather);
        self.invalidate_block_mesh_dependencies(wx, wy, wz, DependencyReason::Weather);
        // Fan weather-driven block placement out to connected clients.
        self.broadcast_block_change(wx, wy, wz, block);
    }

    pub(super) fn legacy_apply_lightning_effects(
        &mut self,
        strike: crate::network::protocol::LightningStrike,
        strike_pos: Vec3,
        player_pos: Vec3,
    ) {
        for entity in &mut self.entity_manager.entities {
            if entity.entity_type == crate::entity::EntityType::RemotePlayer {
                continue;
            }
            let horizontal = glam::Vec2::new(
                entity.position.x - strike_pos.x,
                entity.position.z - strike_pos.z,
            )
            .length();
            if entity.health > 0.0 && horizontal <= 3.5 {
                entity.health -= 10.0;
                entity.fire_aspect_timer = entity.fire_aspect_timer.max(5.0);
            }
        }
        let player_horizontal =
            glam::Vec2::new(player_pos.x - strike_pos.x, player_pos.z - strike_pos.z).length();
        if player_horizontal <= 3.5 {
            self.take_damage(10.0, DamageSource::Lightning);
        }

        let fire_y = strike.y;
        let support_y = fire_y - 1;
        let support = self.chunk_manager.get_block(strike.x, support_y, strike.z);
        if fire_y < CHUNK_HEIGHT as i32
            && support.properties().is_solid
            && !matches!(
                support,
                BlockType::Water | BlockType::Lava | BlockType::Ice | BlockType::Snow
            )
            && self.chunk_manager.get_block(strike.x, fire_y, strike.z) == BlockType::Air
        {
            self.apply_weather_block_change(strike.x, fire_y, strike.z, BlockType::Fire);
        }
    }

    pub(super) fn apply_redstone_update(&mut self, update: crate::redstone::RedstoneUpdate) {
        if !self.presentation_topology().is_legacy_owner() {
            return;
        }
        let mut dirty_chunks = std::collections::HashSet::new();
        let mut broadcast: Vec<((i32, i32, i32), BlockType)> = Vec::new();
        for mutation in update.mutations {
            let (wx, wy, wz) = mutation.pos;
            let old_properties = mutation.old_block.properties();
            let new_properties = mutation.new_block.properties();

            if old_properties.is_solid != new_properties.is_solid {
                if new_properties.is_solid {
                    crate::lighting::update_sky_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                } else {
                    crate::lighting::update_sky_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                }
            }
            if old_properties.light_emission != new_properties.light_emission {
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    old_properties.light_emission,
                    &mut dirty_chunks,
                );
                if new_properties.light_emission > 0 {
                    crate::lighting::update_block_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        new_properties.light_emission,
                        &mut dirty_chunks,
                    );
                }
            }
            mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
            broadcast.push(((wx, wy, wz), mutation.new_block));
        }

        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Redstone);

        // Fan the redstone-driven block mutations out to connected clients.
        for ((x, y, z), block) in broadcast {
            self.broadcast_block_change(x, y, z, block);
        }

        // Observer baseline/pulse revisions are authoritative block-entity
        // mutations even though the observer block id itself stays unchanged.
        // Replicate them through the same chunk-revision path as container
        // updates so reconnecting/joining clients cannot retain stale pulse
        // state.
        for ((x, y, z), entity) in update.block_entity_changes {
            self.chunk_manager.mark_block_entity_dirty(x, z);
            self.broadcast_block_entity_delta(x, y, z, Some(entity));
        }

        for action in update.actions {
            match action {
                crate::redstone::RedstoneAction::Explode { pos } => {
                    let center =
                        Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
                    let mut dirty_meshes = std::collections::HashSet::new();
                    let removed = crate::mob::explode(
                        center,
                        4.0,
                        &mut self.chunk_manager,
                        &mut dirty_meshes,
                        &mut self.player_physics,
                        &mut self.player_state,
                        true,
                        self.game_mode,
                        1.0,
                    );
                    self.invalidate_chunk_meshes(dirty_meshes, DependencyReason::Redstone);
                    for (x, y, z) in removed {
                        self.broadcast_block_change(x, y, z, BlockType::Air);
                    }
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::Explosion);
                }
                crate::redstone::RedstoneAction::Dispense {
                    pos,
                    facing,
                    dropper,
                } => {
                    self.execute_container_dispense_action(pos, facing, dropper);
                }
                crate::redstone::RedstoneAction::PlayNote { pos, note } => {
                    let sound_pos =
                        Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
                    let listener_right =
                        Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                            .normalize_or_zero();
                    self.audio_manager.play_sound_3d(
                        crate::audio::SoundId::Note(note),
                        sound_pos,
                        self.camera.position,
                        listener_right,
                    );
                }
            }
        }

        if update.propagation_overflowed {
            eprintln!("[Redstone] propagation pass limit reached; continuing next tick");
        }
    }

    pub(super) fn consume_container_slot_one(&mut self, pos: (i32, i32, i32), slot: usize) -> bool {
        let Some(entity) = self
            .chunk_manager
            .get_block_entity(pos.0, pos.1, pos.2)
            .cloned()
        else {
            return false;
        };
        let Some(stack) = entity.get_stack(slot).copied() else {
            return false;
        };
        let mut updated = entity;
        updated.set_stack(
            slot,
            (stack.count > 1).then_some(crate::inventory::ItemStack {
                count: stack.count - 1,
                ..stack
            }),
        );
        self.chunk_manager
            .set_block_entity(pos.0, pos.1, pos.2, Some(updated));
        self.chunk_manager.mark_block_entity_dirty(pos.0, pos.2);
        true
    }

    pub(super) fn replace_container_slot(
        &mut self,
        pos: (i32, i32, i32),
        slot: usize,
        stack: Option<crate::inventory::ItemStack>,
    ) -> bool {
        let Some(mut entity) = self
            .chunk_manager
            .get_block_entity(pos.0, pos.1, pos.2)
            .cloned()
        else {
            return false;
        };
        entity.set_stack(slot, stack);
        self.chunk_manager
            .set_block_entity(pos.0, pos.1, pos.2, Some(entity));
        self.chunk_manager.mark_block_entity_dirty(pos.0, pos.2);
        true
    }

    pub(super) fn apply_automation_block_change(&mut self, pos: (i32, i32, i32), block: BlockType) {
        let old = self.chunk_manager.get_block(pos.0, pos.1, pos.2);
        if old == block {
            return;
        }
        self.chunk_manager.set_block(pos.0, pos.1, pos.2, block);
        let mut dirty_chunks = std::collections::HashSet::new();
        if block.properties().is_solid {
            crate::lighting::update_sky_light_after_placed(
                &mut self.chunk_manager,
                pos.0,
                pos.1,
                pos.2,
                &mut dirty_chunks,
            );
        } else {
            crate::lighting::update_sky_light_after_removed(
                &mut self.chunk_manager,
                pos.0,
                pos.1,
                pos.2,
                &mut dirty_chunks,
            );
        }
        if block.properties().light_emission > 0 {
            crate::lighting::update_block_light_after_placed(
                &mut self.chunk_manager,
                pos.0,
                pos.1,
                pos.2,
                block.properties().light_emission,
                &mut dirty_chunks,
            );
        }
        mark_block_mesh_dependencies(&mut dirty_chunks, pos.0, pos.2);
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Redstone);
        self.redstone
            .on_block_changed(&self.chunk_manager, pos, crate::redstone::Direction::North);
        self.broadcast_block_change(pos.0, pos.1, pos.2, block);
    }

    pub(super) fn execute_container_dispense_action(
        &mut self,
        pos: (i32, i32, i32),
        facing: crate::redstone::Direction,
        is_dropper: bool,
    ) {
        use crate::inventory::{Item, ItemStack};

        if !self.presentation_topology().is_legacy_owner() {
            return;
        }

        let delta = facing.delta();
        let front_pos = (pos.0 + delta.0, pos.1 + delta.1, pos.2 + delta.2);
        // A loaded source must not resolve an unloaded destination as Air and
        // then consume/spawn an item across the streaming boundary.
        if !self
            .chunk_manager
            .is_block_loaded(front_pos.0, front_pos.1, front_pos.2)
        {
            return;
        }
        let spawn_pos = Vec3::new(
            pos.0 as f32 + 0.5 + delta.0 as f32 * 0.7,
            pos.1 as f32 + 0.5 + delta.1 as f32 * 0.7,
            pos.2 as f32 + 0.5 + delta.2 as f32 * 0.7,
        );

        let seed = (pos.0 as u64)
            ^ ((pos.1 as u64) << 16)
            ^ ((pos.2 as u64) << 32)
            ^ self.redstone.current_tick();

        let Some(source) = self
            .chunk_manager
            .get_block_entity(pos.0, pos.1, pos.2)
            .cloned()
        else {
            return;
        };
        let Some(slot_idx) = source.select_random_non_empty_slot(seed) else {
            return;
        };
        let Some(stack) = source.get_stack(slot_idx).copied() else {
            return;
        };
        let one = ItemStack { count: 1, ..stack };
        let mut changed = false;
        let mut target_changed = false;

        if is_dropper {
            // A dropper first attempts a sided, metadata-preserving insertion;
            // a full/invalid target falls back to dropping the same one-item
            // stack.  Both outcomes are successful and consume exactly one.
            let target = self
                .chunk_manager
                .get_block_entity(front_pos.0, front_pos.1, front_pos.2)
                .cloned();
            if let Some(target) = target {
                let mut target_after = target;
                if target_after.try_insert_item(Some(facing.opposite()), one) {
                    let mut source_after = source.clone();
                    source_after.set_stack(
                        slot_idx,
                        (stack.count > 1).then_some(ItemStack {
                            count: stack.count - 1,
                            ..stack
                        }),
                    );
                    self.chunk_manager
                        .set_block_entity(pos.0, pos.1, pos.2, Some(source_after));
                    self.chunk_manager.set_block_entity(
                        front_pos.0,
                        front_pos.1,
                        front_pos.2,
                        Some(target_after),
                    );
                    changed = true;
                    target_changed = true;
                }
            }
            if !changed {
                self.spawn_dropped_stack(one, spawn_pos);
                self.consume_container_slot_one(pos, slot_idx);
                changed = true;
            }
        } else {
            match stack.item {
                Item::Arrow => {
                    let id = self
                        .entity_manager
                        .spawn(crate::entity::EntityType::Arrow, spawn_pos);
                    if let Some(arrow) = self.entity_manager.get_by_id_mut(id) {
                        arrow.velocity =
                            Vec3::new(delta.0 as f32, delta.1 as f32, delta.2 as f32) * 18.0;
                        arrow.friendly_projectile = true;
                        arrow.projectile_damage = 4.0;
                    }
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::ArrowShoot);
                    self.consume_container_slot_one(pos, slot_idx);
                    changed = true;
                }
                Item::SplashPotion => {
                    let id = self
                        .entity_manager
                        .spawn(crate::entity::EntityType::SplashPotion, spawn_pos);
                    if let Some(potion) = self.entity_manager.get_by_id_mut(id) {
                        potion.velocity =
                            Vec3::new(delta.0 as f32, delta.1 as f32, delta.2 as f32) * 10.0;
                        potion.potion = stack.potion;
                    }
                    self.consume_container_slot_one(pos, slot_idx);
                    changed = true;
                }
                Item::Bucket => {
                    let filled =
                        match self
                            .chunk_manager
                            .get_block(front_pos.0, front_pos.1, front_pos.2)
                        {
                            BlockType::Water => Some(Item::WaterBucket),
                            BlockType::Lava => Some(Item::LavaBucket),
                            _ => None,
                        };
                    if let Some(filled) = filled {
                        self.apply_automation_block_change(front_pos, BlockType::Air);
                        self.replace_container_slot(
                            pos,
                            slot_idx,
                            Some(ItemStack {
                                item: filled,
                                ..stack
                            }),
                        );
                        changed = true;
                    }
                }
                Item::WaterBucket | Item::LavaBucket => {
                    if self
                        .chunk_manager
                        .get_block(front_pos.0, front_pos.1, front_pos.2)
                        == BlockType::Air
                    {
                        let place_block = if stack.item == Item::WaterBucket {
                            BlockType::Water
                        } else {
                            BlockType::Lava
                        };
                        self.apply_automation_block_change(front_pos, place_block);
                        self.replace_container_slot(
                            pos,
                            slot_idx,
                            Some(ItemStack {
                                item: Item::Bucket,
                                ..stack
                            }),
                        );
                        changed = true;
                    }
                }
                Item::FlintAndSteel => {
                    let target =
                        self.chunk_manager
                            .get_block(front_pos.0, front_pos.1, front_pos.2);
                    let below =
                        self.chunk_manager
                            .get_block(front_pos.0, front_pos.1 - 1, front_pos.2);
                    if target == BlockType::Air && below.properties().is_solid {
                        self.apply_automation_block_change(front_pos, BlockType::Fire);
                        self.consume_container_slot_one(pos, slot_idx);
                        changed = true;
                    }
                }
                _ => {
                    // The item is a valid dispenser payload even when this
                    // simplified runtime has no special entity for it: drop a
                    // full metadata-bearing stack item and consume one.
                    self.spawn_dropped_stack(one, spawn_pos);
                    self.consume_container_slot_one(pos, slot_idx);
                    changed = true;
                }
            }
        }

        if changed {
            self.chunk_manager.mark_block_entity_dirty(pos.0, pos.2);
            self.redstone
                .mark_container_changed(&self.chunk_manager, pos);
            let entity = self
                .chunk_manager
                .get_block_entity(pos.0, pos.1, pos.2)
                .cloned();
            self.broadcast_block_entity_delta(pos.0, pos.1, pos.2, entity);
            if target_changed {
                self.chunk_manager
                    .mark_block_entity_dirty(front_pos.0, front_pos.2);
                self.redstone
                    .mark_container_changed(&self.chunk_manager, front_pos);
                let target_entity = self
                    .chunk_manager
                    .get_block_entity(front_pos.0, front_pos.1, front_pos.2)
                    .cloned();
                self.broadcast_block_entity_delta(
                    front_pos.0,
                    front_pos.1,
                    front_pos.2,
                    target_entity,
                );
            }
        }
    }

    pub(super) fn update_dropped_items_and_orbs(&mut self, dt: f32) {
        if !self.presentation_topology().is_legacy_owner() {
            return;
        }
        let mut remove_ids = Vec::new();
        let mut merges = Vec::new();

        for entity in &mut self.entity_manager.entities {
            if matches!(
                entity.entity_type,
                crate::entity::EntityType::DroppedItem | crate::entity::EntityType::ExperienceOrb
            ) {
                entity.update_physics(dt, &self.chunk_manager);
                entity.life_time += dt;
                entity.pickup_cooldown = (entity.pickup_cooldown - dt).max(0.0);

                if entity.life_time > 300.0 {
                    remove_ids.push(entity.id);
                }
            }
        }

        // Dropped item merging
        let dropped_items: Vec<(u64, Vec3, Item, u32)> = self
            .entity_manager
            .get_entities_by_type(crate::entity::EntityType::DroppedItem)
            .filter(|e| e.dropped_item.is_some())
            .map(|e| {
                (
                    e.id,
                    e.position,
                    e.dropped_item.unwrap(),
                    e.dropped_count.max(1),
                )
            })
            .collect();

        for i in 0..dropped_items.len() {
            for j in (i + 1)..dropped_items.len() {
                let (id_a, pos_a, item_a, count_a) = dropped_items[i];
                let (id_b, pos_b, item_b, count_b) = dropped_items[j];

                if item_a == item_b
                    && pos_a.distance(pos_b) < 1.0
                    && count_a + count_b <= item_a.properties().max_stack
                {
                    merges.push((id_a, id_b, count_a + count_b));
                }
            }
        }

        for (target_id, source_id, new_count) in merges {
            if let Some(target) = self.entity_manager.get_by_id_mut(target_id) {
                target.dropped_count = new_count;
                if let Some(ref mut stack) = target.dropped_stack {
                    stack.count = new_count;
                }
            }
            remove_ids.push(source_id);
        }

        for id in remove_ids {
            self.entity_manager.remove_by_id(id);
        }
    }

    pub(super) fn legacy_take_damage_with_attacker(
        &mut self,
        amount: f32,
        source: DamageSource,
        attacker_pos: Option<[f32; 3]>,
        attacker_item: Option<Item>,
    ) {
        let can_damage = !self.player_state.is_dead && self.player_state.invulnerable_time <= 0.0;
        if !can_damage {
            return;
        }

        // Check shield blocking
        let is_blocking = self
            .player_state
            .using_item
            .as_ref()
            .map_or(false, |u| u.action == crate::player::ItemUseAction::Block);
        let yaw = self.camera.yaw;
        let pos = [
            self.player_physics.position.x,
            self.player_physics.position.y,
            self.player_physics.position.z,
        ];
        if is_blocking && crate::player::can_shield_block(yaw, pos, attacker_pos, source) {
            self.audio_manager
                .play_sound(crate::audio::SoundId::ShieldBlock);

            // Reduce shield durability in holding slot
            if let Some(ref using) = self.player_state.using_item {
                let slot = using.slot;
                let mut shield_stack = match slot {
                    crate::player::HandSlot::MainHand(i) => {
                        self.inventory.hotbar.get(i).copied().flatten()
                    }
                    crate::player::HandSlot::OffHand => self.inventory.offhand,
                };
                if let Some(ref mut stack) = shield_stack {
                    if stack.durability > 0 {
                        let dur_loss = if amount > 3.0 {
                            (amount.floor() as u32) + 1
                        } else {
                            1
                        };
                        if stack.durability <= dur_loss {
                            match slot {
                                crate::player::HandSlot::MainHand(i) => {
                                    self.inventory.hotbar[i] = None
                                }
                                crate::player::HandSlot::OffHand => self.inventory.offhand = None,
                            }
                            self.audio_manager
                                .play_sound(crate::audio::SoundId::ShieldBreak);
                            self.player_state.using_item = None;
                        } else {
                            stack.durability -= dur_loss;
                            match slot {
                                crate::player::HandSlot::MainHand(i) => {
                                    self.inventory.hotbar[i] = Some(*stack)
                                }
                                crate::player::HandSlot::OffHand => {
                                    self.inventory.offhand = Some(*stack)
                                }
                            }
                        }
                    }
                }
            }

            // Check if attacker used an Axe -> disable shield!
            if let Some(att_item) = attacker_item {
                if att_item
                    .tool_properties()
                    .map_or(false, |t| t.tool_type == ToolType::Axe)
                {
                    self.player_state.shield_disable_ticks = 100;
                    self.player_state.using_item = None;
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::ShieldBreak);
                }
            }

            return; // No damage taken when shield blocked!
        }

        let mut total_armor = 0.0f32;
        let mut total_toughness = 0.0f32;
        for armor_slot in self.inventory.armor.iter().flatten() {
            if let Some(props) = armor_slot.item.armor_properties() {
                total_armor += props.armor_points;
                total_toughness += props.toughness;
            }
        }
        let total_epf =
            crate::enchantment::epf_sum(&self.inventory.armor, source == DamageSource::Fall);

        let final_damage = crate::player::calculate_damage_reduction(
            amount,
            source,
            total_armor,
            total_toughness,
            total_epf,
        );

        let died = self.player_state.take_damage(final_damage, source);

        if died {
            self.player_physics.set_flying(false);
            self.jump_taps.reset();
            self.audio_manager
                .play_sound(crate::audio::SoundId::PlayerDeath);
            println!("[Debug] Player died due to: {:?}", source);

            let pos = self.player_physics.position + Vec3::new(0.0, 1.0, 0.0);
            let mut to_drop = Vec::new();
            for slot in self
                .inventory
                .hotbar
                .iter()
                .chain(self.inventory.main.iter())
                .chain(self.inventory.armor.iter())
            {
                if let Some(stack) = slot {
                    to_drop.push(*stack);
                }
            }
            if let Some(offhand) = self.inventory.offhand {
                to_drop.push(offhand);
            }
            if let Some(dragged) = self.inventory.dragged {
                to_drop.push(dragged);
            }
            for craft_slot in &self.inventory.craft_input {
                if let Some(stack) = craft_slot {
                    to_drop.push(*stack);
                }
            }

            if !self.world_rules.keep_inventory {
                for stack in to_drop {
                    self.spawn_dropped_stack(stack, pos);
                }
            }

            let xp_drop = self.player_state.death_experience_drop();
            if xp_drop > 0 {
                self.spawn_xp_orb(xp_drop, pos);
            }

            if !self.world_rules.keep_inventory {
                self.inventory.clear();
            }
        } else {
            self.audio_manager
                .play_sound(crate::audio::SoundId::PlayerHurt);
        }
    }

    pub(super) fn legacy_respawn(&mut self) {
        if self.world_rules.hardcore && self.player_state.is_dead {
            // Hardcore worlds never respawn a dead player into Survival.  The
            // permitted recovery path is a read-only Spectator observer.
            self.player_state.is_dead = false;
            self.player_state.health = self.player_state.max_health;
            self.set_game_mode(GameMode::Spectator);
            self.sync_cursor_mode();
            return;
        }
        self.player_physics.set_flying(false);
        self.jump_taps.reset();

        let mut spawn_pos = None;
        let mut spawn_dim = crate::dimension::Dimension::Overworld;

        if let (Some(bed_p), Some(dim)) = (
            self.player_state.spawn_point,
            self.player_state.spawn_dimension,
        ) {
            let chunk_pos = (bed_p[0], bed_p[1], bed_p[2]);
            if self
                .chunk_manager
                .get_block(chunk_pos.0, chunk_pos.1, chunk_pos.2)
                == crate::world::BlockType::Bed
            {
                let (safe_p, safe) =
                    crate::world::find_safe_spawn_position(&self.chunk_manager, chunk_pos);
                if safe {
                    spawn_pos = Some(safe_p);
                    spawn_dim = dim;
                }
            }
            if spawn_pos.is_none() {
                self.player_state.spawn_point = None;
                self.player_state.spawn_dimension = None;
                println!("[Debug] Your home bed was missing or obstructed");
            }
        }

        if spawn_pos.is_none() {
            spawn_dim = crate::dimension::Dimension::Overworld;
            let target_p = (self.world_spawn.0, self.world_spawn.1, self.world_spawn.2);
            let (safe_p, safe) =
                crate::world::find_safe_spawn_position(&self.chunk_manager, target_p);
            if safe {
                spawn_pos = Some(safe_p);
            } else {
                self.chunk_manager.set_block(
                    target_p.0,
                    target_p.1 - 1,
                    target_p.2,
                    crate::world::BlockType::Cobblestone,
                );
                self.chunk_manager.set_block(
                    target_p.0,
                    target_p.1,
                    target_p.2,
                    crate::world::BlockType::Air,
                );
                self.chunk_manager.set_block(
                    target_p.0,
                    target_p.1 + 1,
                    target_p.2,
                    crate::world::BlockType::Air,
                );
                spawn_pos = Some(Vec3::new(
                    target_p.0 as f32 + 0.5,
                    target_p.1 as f32,
                    target_p.2 as f32 + 0.5,
                ));
            }
        }

        if self.current_dimension != spawn_dim {
            self.switch_dimension(spawn_dim);
        }

        let target_vec = spawn_pos.unwrap_or_else(|| Vec3::new(8.0, 80.0, 8.0));
        self.player_physics.position = target_vec;
        self.player_physics.velocity = glam::Vec3::ZERO;
        self.player_physics.on_ground = false;
        self.player_physics.highest_y = target_vec.y;

        self.player_state.reset_for_respawn();
        self.void_damage_timer = 0.0;

        self.sync_cursor_mode();

        println!("[Debug] Player respawned at spawn point");
    }

    pub(super) fn legacy_execute_command(
        &mut self,
        command: crate::commands::Command,
    ) -> Option<String> {
        use crate::commands::{Command, CommandTarget, TimeCommand, WeatherCommand};

        let target_is_local = |target: Option<&CommandTarget>| {
            target.is_none()
                || matches!(
                    target,
                    Some(CommandTarget::SelfPlayer | CommandTarget::NearestPlayer)
                )
        };
        let target_is_single_local = |target: &CommandTarget| {
            matches!(
                target,
                CommandTarget::SelfPlayer | CommandTarget::NearestPlayer
            )
        };

        match command {
            Command::Help(command) => Some(crate::commands::help_text(command.as_deref()).into()),
            Command::GameMode { mode, target } => {
                if !target_is_local(target.as_ref()) {
                    Some(self.translate("command.only_local_player"))
                } else if self.world_rules.hardcore
                    && self.player_state.is_dead
                    && mode == GameMode::Survival
                {
                    Some(self.translate("command.hardcore_survival"))
                } else {
                    self.set_game_mode(mode);
                    let mode = format!("{mode:?}");
                    Some(
                        self.translation_catalog
                            .format_lookup("command.gamemode_set", &[("mode", &mode)]),
                    )
                }
            }
            Command::Difficulty(difficulty) => {
                self.difficulty = if self.world_rules.hardcore {
                    Difficulty::Hard
                } else {
                    difficulty
                };
                let difficulty = format!("{:?}", self.difficulty);
                Some(
                    self.translation_catalog
                        .format_lookup("command.difficulty_set", &[("difficulty", &difficulty)]),
                )
            }
            Command::GameRule { rule, value } => {
                if let Some(value) = value {
                    let changed = if matches!(
                        rule.as_str(),
                        "playerssleepingpercentage" | "sleepingpercentage" | "sleeping_percentage"
                    ) {
                        value
                            .parse::<u8>()
                            .ok()
                            .map(|value| {
                                self.world_rules.set_sleeping_percentage(value);
                                true
                            })
                            .unwrap_or(false)
                    } else if let Some(value) = parse_command_bool(&value) {
                        self.world_rules.set(&rule, value).is_ok()
                    } else {
                        false
                    };
                    if changed {
                        self.set_world_rules(self.world_rules);
                        self.broadcast_world_rules();
                        Some(
                            self.translation_catalog
                                .format_lookup("command.gamerule_updated", &[("rule", &rule)]),
                        )
                    } else {
                        Some(
                            self.translation_catalog
                                .format_lookup("command.gamerule_invalid", &[("rule", &rule)]),
                        )
                    }
                } else if let Some(current) = self.world_rules.value(&rule) {
                    Some(format!("{rule} = {current}"))
                } else if matches!(
                    rule.as_str(),
                    "playerssleepingpercentage" | "sleepingpercentage" | "sleeping_percentage"
                ) {
                    Some(format!(
                        "playersSleepingPercentage = {}",
                        self.world_rules.sleeping_percentage
                    ))
                } else {
                    Some(format!("Unknown game rule: {rule}."))
                }
            }
            Command::Time(time) => {
                match time {
                    TimeCommand::Set(ticks) => self.world_time.ticks = ticks,
                    TimeCommand::Add(ticks) => {
                        self.world_time.ticks = self.world_time.ticks.saturating_add(ticks)
                    }
                }
                self.broadcast_time_sync();
                let ticks = self.world_time.ticks.to_string();
                Some(
                    self.translation_catalog
                        .format_lookup("command.time_set", &[("ticks", &ticks)]),
                )
            }
            Command::Weather(weather) => {
                let (kind, duration) = match weather {
                    WeatherCommand::Clear(duration) => (crate::weather::Weather::Clear, duration),
                    WeatherCommand::Rain(duration) => (crate::weather::Weather::Rain, duration),
                    WeatherCommand::Thunder(duration) => {
                        (crate::weather::Weather::Thunder, duration)
                    }
                };
                self.weather.set_weather(kind, duration);
                self.broadcast_time_sync();
                let weather = format!("{kind:?}");
                Some(
                    self.translation_catalog
                        .format_lookup("command.weather_set", &[("weather", &weather)]),
                )
            }
            Command::Teleport { target, position } => {
                if !target_is_single_local(&target) {
                    Some(self.translate("command.only_local_player"))
                } else if self.current_dimension.height().contains_y(position[1]) {
                    self.player_physics.position = Vec3::new(
                        position[0] as f32 + 0.5,
                        position[1] as f32,
                        position[2] as f32 + 0.5,
                    );
                    self.player_physics.velocity = Vec3::ZERO;
                    self.camera.position = self.player_physics.position + Vec3::Y * 1.6;
                    let x = position[0].to_string();
                    let y = position[1].to_string();
                    let z = position[2].to_string();
                    Some(
                        self.translation_catalog.format_lookup(
                            "command.teleported",
                            &[("x", &x), ("y", &y), ("z", &z)],
                        ),
                    )
                } else {
                    Some(self.translate("command.teleport_outside"))
                }
            }
            Command::Give {
                target,
                item,
                count,
            } => {
                if !target_is_single_local(&target) {
                    Some(self.translate("command.only_local_player"))
                } else {
                    let remainder = self.inventory.add_stack(ItemStack::new(item, count));
                    let received = remainder
                        .as_ref()
                        .map_or(count, |remaining| count - remaining.count);
                    let received = received.to_string();
                    let item = self.localized_item_name(item);
                    Some(
                        self.translation_catalog.format_lookup(
                            "command.gave",
                            &[("count", &received), ("item", &item)],
                        ),
                    )
                }
            }
            Command::Kill(target) => {
                if !target_is_local(target.as_ref()) {
                    Some(self.translate("command.only_local_player"))
                } else {
                    self.player_state.invulnerable_time = 0.0;
                    self.take_damage(1.0e9, DamageSource::Void);
                    Some(self.translate("command.killed"))
                }
            }
            Command::SpawnPoint { target, position } => {
                if !target_is_single_local(&target) {
                    Some(self.translate("command.only_local_player"))
                } else {
                    let position = position.unwrap_or([
                        self.player_physics.position.x.floor() as i32,
                        self.player_physics.position.y.floor() as i32,
                        self.player_physics.position.z.floor() as i32,
                    ]);
                    self.player_state.spawn_point = Some(position);
                    self.player_state.spawn_dimension = Some(self.current_dimension);
                    let x = position[0].to_string();
                    let y = position[1].to_string();
                    let z = position[2].to_string();
                    Some(self.translation_catalog.format_lookup(
                        "command.spawn_point_set",
                        &[("x", &x), ("y", &y), ("z", &z)],
                    ))
                }
            }
            Command::SetWorldSpawn(position) => {
                let position = position.unwrap_or([
                    self.player_physics.position.x.floor() as i32,
                    self.player_physics.position.y.floor() as i32,
                    self.player_physics.position.z.floor() as i32,
                ]);
                if crate::dimension::Dimension::Overworld
                    .height()
                    .contains_y(position[1])
                {
                    self.world_spawn = (position[0], position[1], position[2]);
                    let x = position[0].to_string();
                    let y = position[1].to_string();
                    let z = position[2].to_string();
                    Some(self.translation_catalog.format_lookup(
                        "command.world_spawn_set",
                        &[("x", &x), ("y", &y), ("z", &z)],
                    ))
                } else {
                    Some(self.translate("command.world_spawn_outside"))
                }
            }
            Command::Locate(structure) => {
                let structure_id = match structure.as_str() {
                    "dungeon" => Some(crate::structure::StructureId::Dungeon),
                    "mineshaft" => Some(crate::structure::StructureId::Mineshaft),
                    "village" => Some(crate::structure::StructureId::Village),
                    "stronghold" => Some(crate::structure::StructureId::Stronghold),
                    "fortress" | "nether_fortress" => {
                        Some(crate::structure::StructureId::NetherFortress)
                    }
                    "endcity" | "end_city" => Some(crate::structure::StructureId::EndCity),
                    _ => None,
                };
                match structure_id {
                    Some(id) => {
                        let current = [
                            self.player_physics.position.x.floor() as i32,
                            self.player_physics.position.y.floor() as i32,
                            self.player_physics.position.z.floor() as i32,
                        ];
                        match crate::structure::locate_structure(
                            id,
                            (current[0], current[1], current[2]),
                            self.world_seed,
                            self.current_dimension,
                        ) {
                            Some((x, y, z)) => {
                                let x = x.to_string();
                                let y = y.to_string();
                                let z = z.to_string();
                                Some(self.translation_catalog.format_lookup(
                                    "command.nearest_structure",
                                    &[("structure", &structure), ("x", &x), ("y", &y), ("z", &z)],
                                ))
                            }
                            None => Some(self.translation_catalog.format_lookup(
                                "command.no_structure",
                                &[("structure", &structure)],
                            )),
                        }
                    }
                    None => {
                        Some(self.translation_catalog.format_lookup(
                            "command.unknown_structure",
                            &[("structure", &structure)],
                        ))
                    }
                }
            }
            Command::Seed => {
                let seed = self.world_seed.to_string();
                Some(
                    self.translation_catalog
                        .format_lookup("command.seed", &[("seed", &seed)]),
                )
            }
            Command::SaveAll => match self.save_synchronously() {
                Ok(()) => Some(self.translate("command.saved")),
                Err(error) => {
                    let reason = error.to_string();
                    Some(
                        self.translation_catalog
                            .format_lookup("command.save_failed", &[("reason", &reason)]),
                    )
                }
            },
        }
    }
}

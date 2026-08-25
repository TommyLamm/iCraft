//! Leftover renderer-owned world tick for launches without an embedded runtime.
//!
//! Live Singleplayer / Host never enter these methods: they tick
//! `ServerRuntime` via `tick_authority_boundary` and only keep presentation
//! (keys, sprint latch, footsteps, `update_chunks`). This module is a child of
//! `state` (`#[path]`) so it can see private `State` fields without making
//! leftover simulation authoritative. Compiles only under `cfg(test)` or
//! feature `legacy_owner`.

use super::*;

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
                ((total_overworld_players * self.world_rules.sleeping_percentage as usize + 99)
                    / 100)
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
            let yaw_sin = self.camera.yaw.sin();
            let yaw_cos = self.camera.yaw.cos();
            let right = Vec3::new(-yaw_sin, 0.0, yaw_cos).normalize_or_zero();
            let is_raining = matches!(
                self.weather.current,
                crate::weather::Weather::Rain | crate::weather::Weather::Thunder
            );
            let mut mob_dirty_meshes = std::collections::HashSet::new();
            let listener_pos = self.player_physics.position + Vec3::new(0.0, 1.6, 0.0);
            let audio_manager = &mut self.audio_manager;
            let exploded_blocks = crate::mob::update_mobs(
                &mut self.entity_manager,
                &mut self.chunk_manager,
                &mut mob_dirty_meshes,
                &mut self.player_physics,
                &mut self.player_state,
                self.game_mode,
                self.world_time.sky_light_level(),
                is_raining,
                dt,
                |event, pos| {
                    let sound_id = match event {
                        crate::mob::MobSoundEvent::ArrowShoot => crate::audio::SoundId::ArrowShoot,
                        crate::mob::MobSoundEvent::CreeperIgnition => {
                            crate::audio::SoundId::CreeperIgnition
                        }
                        crate::mob::MobSoundEvent::Explosion => crate::audio::SoundId::Explosion,
                        crate::mob::MobSoundEvent::PlayerDeath => {
                            crate::audio::SoundId::PlayerDeath
                        }
                    };
                    audio_manager.play_sound_3d(sound_id, pos, listener_pos, right);
                },
                self.potion_effects.has_invisibility(),
                crate::enchantment::protection_multiplier(&self.inventory.armor, false),
                authoritative,
                self.world_rules.mob_griefing,
            );
            self.invalidate_chunk_meshes(mob_dirty_meshes, DependencyReason::Mob);
            for (x, y, z) in exploded_blocks {
                self.broadcast_block_change(x, y, z, BlockType::Air);
            }
            self.perf_recorder.record(
                crate::perf::ScopeId::HostileMobs,
                hostile_mobs_started.elapsed(),
            );

            // Update passive mobs
            let passive_mobs_started = Instant::now();
            let mut passive_dirty_meshes = std::collections::HashSet::new();
            let grazed_blocks = crate::passive_mob::update_passive_mobs(
                &mut self.entity_manager,
                &mut self.chunk_manager,
                &mut passive_dirty_meshes,
                &self.player_physics,
                &mut self.inventory,
                self.game_mode,
                dt,
                self.total_time,
                authoritative,
                self.world_rules.mob_griefing,
            );
            self.invalidate_chunk_meshes(passive_dirty_meshes, DependencyReason::Mob);
            for (x, y, z) in grazed_blocks {
                self.broadcast_block_change(x, y, z, BlockType::Dirt);
            }

            // Spawn passive mobs (daytime spawn)
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
}

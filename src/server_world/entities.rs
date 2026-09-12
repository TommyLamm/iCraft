use super::*;

impl ServerWorld {
    /// Execute one authoritative dispenser/dropper edge with the candidate id
    /// supplied by AuthorityCore.  The method returns whether an entity was
    /// spawned; block/entity mutations are queued for the normal snapshot
    /// fanout. Invalid, unloaded, empty, or unsupported no-op cases consume
    /// nothing and produce no entity.
    pub fn execute_redstone_dispense(&mut self, action: RedstoneAction, entity_id: u64) -> bool {
        let RedstoneAction::Dispense {
            pos,
            facing,
            dropper,
        } = action
        else {
            return false;
        };
        let source_block = if dropper {
            BlockType::Dropper
        } else {
            BlockType::Dispenser
        };
        if !self.valid_coordinate(pos.0, pos.1, pos.2)
            || self.get_block(pos.0, pos.1, pos.2) != source_block
            || !self.chunks.is_block_loaded(pos.0, pos.1, pos.2)
        {
            return false;
        }
        let delta = facing.delta();
        let front = (pos.0 + delta.0, pos.1 + delta.1, pos.2 + delta.2);
        if !self.valid_coordinate(front.0, front.1, front.2)
            || !self.chunks.is_block_loaded(front.0, front.1, front.2)
        {
            return false;
        }
        let Some(source) = self.get_block_entity(pos.0, pos.1, pos.2).cloned() else {
            return false;
        };
        if !source.matches_block_type(source_block) {
            return false;
        }
        let seed = (self.seed as u64)
            ^ ((self.dimension as u8 as u64) << 56)
            ^ (pos.0 as i64 as u64).rotate_left(7)
            ^ (pos.1 as i64 as u64).rotate_left(19)
            ^ (pos.2 as i64 as u64).rotate_left(31)
            ^ self.redstone.current_tick();
        let Some(slot) = source.select_random_non_empty_slot(seed) else {
            return false;
        };
        let Some(stack) = source.get_stack(slot).copied() else {
            return false;
        };
        let one = crate::inventory::ItemStack { count: 1, ..stack };
        let spawn_pos = Vec3::new(
            pos.0 as f32 + 0.5 + delta.0 as f32 * 0.7,
            pos.1 as f32 + 0.5 + delta.1 as f32 * 0.7,
            pos.2 as f32 + 0.5 + delta.2 as f32 * 0.7,
        );
        let direction = Vec3::new(delta.0 as f32, delta.1 as f32, delta.2 as f32);

        let mut source_after = source.clone();
        let mut target_after = None;
        let mut spawn = None;
        let mut block_change = None;

        // Replace exactly one source item while preserving the remaining
        // stack.  Bucket items normally have max-stack one, but malformed or
        // legacy stacks are handled atomically instead of turning an entire
        // stack into filled buckets.
        let replace_one = |entity: &mut BlockEntity,
                           slot: usize,
                           original: crate::inventory::ItemStack,
                           replacement: crate::inventory::Item|
         -> bool {
            if original.count == 1 {
                entity.set_stack(
                    slot,
                    Some(crate::inventory::ItemStack {
                        item: replacement,
                        ..original
                    }),
                );
                true
            } else {
                entity.set_stack(
                    slot,
                    Some(crate::inventory::ItemStack {
                        count: original.count - 1,
                        ..original
                    }),
                );
                entity.try_insert_item(
                    None,
                    crate::inventory::ItemStack {
                        item: replacement,
                        count: 1,
                        ..original
                    },
                )
            }
        };

        if dropper {
            if let Some(target) = self.get_block_entity(front.0, front.1, front.2).cloned() {
                let mut candidate = target;
                if candidate.try_insert_item(Some(facing.opposite()), one) {
                    target_after = Some(candidate);
                }
            }
            if target_after.is_none() {
                spawn = Some((EntityType::DroppedItem, one, Vec3::new(0.0, 0.2, 0.0)));
            }
            source_after.set_stack(
                slot,
                (stack.count > 1).then_some(crate::inventory::ItemStack {
                    count: stack.count - 1,
                    ..stack
                }),
            );
        } else {
            match stack.item {
                crate::inventory::Item::Arrow => {
                    spawn = Some((EntityType::Arrow, one, direction * 18.0));
                    source_after.set_stack(
                        slot,
                        (stack.count > 1).then_some(crate::inventory::ItemStack {
                            count: stack.count - 1,
                            ..stack
                        }),
                    );
                }
                crate::inventory::Item::SplashPotion => {
                    spawn = Some((EntityType::SplashPotion, one, direction * 10.0));
                    source_after.set_stack(
                        slot,
                        (stack.count > 1).then_some(crate::inventory::ItemStack {
                            count: stack.count - 1,
                            ..stack
                        }),
                    );
                }
                crate::inventory::Item::Bucket => {
                    let filled = match self.get_block(front.0, front.1, front.2) {
                        BlockType::Water
                            if self.chunks.get_fluid_level(front.0, front.1, front.2) == 0
                                && !self.chunks.get_fluid_falling(front.0, front.1, front.2) =>
                        {
                            Some(crate::inventory::Item::WaterBucket)
                        }
                        BlockType::Lava
                            if self.chunks.get_fluid_level(front.0, front.1, front.2) == 0
                                && !self.chunks.get_fluid_falling(front.0, front.1, front.2) =>
                        {
                            Some(crate::inventory::Item::LavaBucket)
                        }
                        _ => None,
                    };
                    let Some(filled) = filled else {
                        return false;
                    };
                    block_change = Some(BlockType::Air);
                    if !replace_one(&mut source_after, slot, stack, filled) {
                        return false;
                    }
                }
                crate::inventory::Item::WaterBucket | crate::inventory::Item::LavaBucket => {
                    if self.get_block(front.0, front.1, front.2) != BlockType::Air {
                        return false;
                    }
                    block_change = Some(if stack.item == crate::inventory::Item::WaterBucket {
                        BlockType::Water
                    } else {
                        BlockType::Lava
                    });
                    if !replace_one(
                        &mut source_after,
                        slot,
                        stack,
                        crate::inventory::Item::Bucket,
                    ) {
                        return false;
                    }
                }
                crate::inventory::Item::FlintAndSteel => {
                    if self.get_block(front.0, front.1, front.2) != BlockType::Air
                        || !self
                            .get_block(front.0, front.1 - 1, front.2)
                            .properties()
                            .is_solid
                    {
                        return false;
                    }
                    block_change = Some(BlockType::Fire);
                    source_after.set_stack(
                        slot,
                        (stack.count > 1).then_some(crate::inventory::ItemStack {
                            count: stack.count - 1,
                            ..stack
                        }),
                    );
                }
                _ => {
                    spawn = Some((EntityType::DroppedItem, one, Vec3::new(0.0, 0.2, 0.0)));
                    source_after.set_stack(
                        slot,
                        (stack.count > 1).then_some(crate::inventory::ItemStack {
                            count: stack.count - 1,
                            ..stack
                        }),
                    );
                }
            }
        }

        // Validate the allocator candidate before committing any source,
        // target, or block mutation. A stale/invalid id must be a true no-op.
        if spawn.is_some() && (entity_id == 0 || self.entities.get_by_id(entity_id).is_some()) {
            return false;
        }

        // Commit all block-entity state only after the action has passed every
        // validation branch.  Each changed position receives one revision.
        self.chunks
            .set_block_entity(pos.0, pos.1, pos.2, Some(source_after));
        self.chunks.mark_block_entity_dirty(pos.0, pos.2);
        self.redstone.mark_container_changed(&self.chunks, pos);
        let source_mutation = self.touch_revision(pos.0, pos.1, pos.2);
        self.pending_mutations.push(source_mutation);

        if let Some(target) = target_after {
            self.chunks
                .set_block_entity(front.0, front.1, front.2, Some(target));
            self.chunks.mark_block_entity_dirty(front.0, front.2);
            self.redstone.mark_container_changed(&self.chunks, front);
            let target_mutation = self.touch_revision(front.0, front.1, front.2);
            self.pending_mutations.push(target_mutation);
        }
        if let Some(block) = block_change {
            let Ok(Some(mutation)) = self.set_block(front.0, front.1, front.2, block, 0) else {
                return false;
            };
            self.pending_mutations.push(mutation);
        }
        if let Some((entity_type, stack, velocity)) = spawn {
            let mut entity = crate::entity::Entity::new(entity_id, entity_type, spawn_pos);
            entity.velocity = velocity;
            match entity_type {
                EntityType::Arrow => {
                    entity.friendly_projectile = true;
                    entity.projectile_damage = 4.0;
                }
                EntityType::SplashPotion => entity.potion = stack.potion,
                EntityType::DroppedItem => {
                    entity.dropped_item = Some(stack.item);
                    entity.dropped_count = stack.count;
                    entity.dropped_stack = Some(stack);
                    entity.pickup_cooldown = 0.5;
                }
                _ => {}
            }
            self.entities.insert_indexed_entity(entity);
            return true;
        }
        false
    }

    pub(super) fn tick_furnaces(&mut self, simulation_chunks: &BTreeSet<(i32, i32)>) -> Vec<WorldMutation> {
        let mut positions = Vec::new();
        for &(cx, cz) in simulation_chunks {
            let Some(chunk) = self.chunks.chunks.get(&(cx, cz)) else {
                continue;
            };
            for &encoded in chunk.furnace_positions() {
                let (lx, y, lz) = crate::world::Chunk::decode_torch_position(encoded);
                positions.push((chunk_origin(cx) + lx as i32, y, chunk_origin(cz) + lz as i32));
            }
        }
        positions.sort_unstable();
        let mut mutations = Vec::new();
        for (x, y, z) in positions {
            let Some(BlockEntity::Furnace(furnace)) = self.chunks.get_block_entity_mut(x, y, z)
            else {
                continue;
            };
            let was_lit = furnace.is_lit;
            let result = furnace.tick(&self.recipe_manager);
            if !result.slot_changed && !result.lit_changed {
                continue;
            }
            furnace.revision = furnace.revision.wrapping_add(1);
            let is_lit = furnace.is_lit;
            let _ = furnace;
            self.redstone
                .mark_container_changed(&self.chunks, (x, y, z));
            if was_lit != is_lit {
                let mut lit_state = crate::world::BlockState::decode(self.get_block_state(x, y, z));
                lit_state.is_open = is_lit;
                if let Ok(Some(event)) =
                    self.set_block(x, y, z, BlockType::Furnace, lit_state.encode())
                {
                    mutations.push(event);
                }
            } else {
                mutations.push(self.touch_revision(x, y, z));
            }
        }
        mutations
    }

    pub(super) fn tick_entities(&mut self, players: &[(PlayerId, [f32; 3], f32, f32)]) {
        // Peaceful is an authority policy, not merely a spawn-rate hint:
        // already-loaded hostile entities are removed at the next fixed tick.
        // `do_mob_spawning=false` deliberately does not take this path, so it
        // cannot freeze an existing hostile entity's AI.
        if matches!(self.difficulty, Difficulty::Peaceful) {
            let before = self.entities.entities.len();
            self.entities
                .entities
                .retain(|entity| !entity.entity_type.is_hostile());
            if self.entities.entities.len() != before {
                self.entities.rebuild_indexes();
            }
        }
        let mut player_positions: Vec<_> = players
            .iter()
            .map(|(id, position, yaw, pitch)| (*id, *position, *yaw, *pitch))
            .collect();
        player_positions.sort_by_key(|(id, _, _, _)| *id);
        if self.rules.do_mob_spawning && self.dimension == Dimension::Overworld {
            for (_, position, _, _) in &player_positions {
                let player = Vec3::from_array(*position);
                let sky_light = if (self.time % 24_000) < 12_000 { 15 } else { 4 };
                crate::passive_mob::spawn_passive_mobs(
                    &mut self.entities,
                    &self.chunks,
                    player,
                    sky_light,
                    self.time as f32 * FIXED_DT,
                );
                if self.allows_hostile_spawning() {
                    crate::mob::spawn_mobs(
                        &mut self.entities,
                        &self.chunks,
                        player,
                        sky_light,
                        self.time as f32 * FIXED_DT,
                    );
                }
            }
        }
        let chunks = &self.chunks;
        let mut checksum_inputs_changed = false;
        let mut moved_ids = std::mem::take(&mut self.entities.scratch.id_list);
        moved_ids.clear();
        let chase_range_sq = HOSTILE_CHASE_RANGE * HOSTILE_CHASE_RANGE;

        // Pre-pass: hostile chase + idle skip flags (sequential — mutates AI
        // targeting from sorted player list). Physics itself is parallel.
        let mut run_physics = vec![false; self.entities.entities.len()];
        for (index, entity) in self.entities.entities.iter_mut().enumerate() {
            if entity.entity_type == EntityType::FishingHook {
                continue;
            }
            entity.action_cooldown = (entity.action_cooldown - FIXED_DT).max(0.0);
            entity.invulnerable_time = (entity.invulnerable_time - FIXED_DT).max(0.0);
            entity.fire_aspect_timer = (entity.fire_aspect_timer - FIXED_DT).max(0.0);

            // Hostile chase: only write velocity when a player is in range and
            // the desired horizontal speed differs. Out-of-range / no-player
            // ticks must not re-assign the same chase vector (keeps EntityState
            // fingerprints and Plan 08 checksum idle reuse honest).
            let mut chasing = false;
            if entity.entity_type.is_hostile() && !matches!(self.difficulty, Difficulty::Peaceful)
            {
                let nearest = player_positions.iter().min_by(|(_, left, _, _), (_, right, _, _)| {
                    entity
                        .position
                        .distance_squared(Vec3::from_array(*left))
                        .total_cmp(&entity.position.distance_squared(Vec3::from_array(*right)))
                });
                if let Some((_, target, _, _)) = nearest.filter(|(_, target, _, _)| {
                    entity.position.distance_squared(Vec3::from_array(*target)) <= chase_range_sq
                }) {
                    chasing = true;
                    let direction =
                        (Vec3::from_array(*target) - entity.position).normalize_or_zero();
                    let speed = self.difficulty.hostile_chase_speed_milli() as f32 / 1_000.0;
                    let desired_x = direction.x * 1.2 * speed;
                    let desired_z = direction.z * 1.2 * speed;
                    if entity.velocity.x != desired_x || entity.velocity.z != desired_z {
                        entity.velocity.x = desired_x;
                        entity.velocity.z = desired_z;
                    }
                    entity.target_player = true;
                } else if entity.target_player {
                    entity.target_player = false;
                }
            }

            // Stationary living / sitting entities skip AI phase bumps and
            // physics. Dropped items keep the existing grounded skip inside
            // `update_physics` (pickup cooldown still ticks there). Unloaded
            // column freeze stays inside `update_physics` for movers.
            if entity_skips_idle_physics(entity, chasing) {
                continue;
            }

            entity.ai_phase = entity.ai_phase.wrapping_add(1);
            entity.ai_timer += FIXED_DT;
            run_physics[index] = true;
            // Movers always bump ai_phase; mark checksum dirty here so the
            // parallel pass only needs to report position changes.
            checksum_inputs_changed = true;
        }

        // Parallel physics: read-only chunk neighborhoods, collect movers, then
        // sort ids before syncing spatial buckets (checksum determinism).
        let physics_results: Vec<(u64, bool)> = self
            .entities
            .entities
            .par_iter_mut()
            .enumerate()
            .filter_map(|(index, entity)| {
                if !run_physics[index] {
                    return None;
                }
                let prev_position = entity.position;
                let cx = (entity.position.x / 16.0).floor() as i32;
                let cz = (entity.position.z / 16.0).floor() as i32;
                let neighborhood = chunks.column_neighborhood_view(cx, cz);
                entity.update_physics(FIXED_DT, &neighborhood);
                let moved = entity.position != prev_position;
                Some((entity.id, moved))
            })
            .collect();

        for (id, moved) in physics_results {
            if moved {
                moved_ids.push(id);
            }
        }
        moved_ids.sort_unstable();
        moved_ids.dedup();
        if checksum_inputs_changed {
            self.entities.mark_checksum_inputs_changed();
        }
        self.entities.sync_entity_positions(&moved_ids);
        self.entities.scratch.id_list = moved_ids;

        if player_positions.is_empty() {
            return;
        }

        let boss_players: Vec<(Vec3, Vec3)> = player_positions
            .iter()
            .map(|(_, position, yaw, pitch)| {
                (Vec3::from_array(*position), look_from_yaw_pitch(*yaw, *pitch))
            })
            .collect();
        let focus = boss_players
            .iter()
            .min_by(|(left, _), (right, _)| {
                left.length_squared().total_cmp(&right.length_squared())
            })
            .map(|(pos, _)| *pos)
            .unwrap_or(Vec3::ZERO);

        // When the AI loop already dirtied checksum inputs, skip the
        // before/after key capture. Otherwise detect boss-path pose /
        // ai_phase / membership changes (including within-chunk moves).
        let epoch_before = self.entities.checksum_epoch();
        let keys_before = (!checksum_inputs_changed).then(|| entity_checksum_keys(&self.entities));
        crate::boss::ensure_dimension_entities(
            self.dimension,
            &mut self.entities,
            &self.chunks,
            focus,
            self.time as f32 * FIXED_DT,
        );
        let boss_events = crate::boss::update_dimension_entities(
            self.dimension,
            &mut self.entities,
            &self.chunks,
            &boss_players,
            FIXED_DT,
            crate::inventory::GameMode::Survival,
        );
        if let Some(before) = keys_before {
            let epoch_changed = self.entities.checksum_epoch() != epoch_before;
            if !epoch_changed && before != entity_checksum_keys(&self.entities) {
                self.entities.mark_checksum_inputs_changed();
            }
        }
        if boss_events.dragon_completion.is_some() {
            self.handle_dragon_completion();
        }
    }

    pub(crate) fn handle_dragon_completion(&mut self) {
        const EXIT_Y: i32 = 73;
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Ok(Some(mutation)) = self.set_block(dx, EXIT_Y, dz, BlockType::EndPortal, 0)
                {
                    self.pending_mutations.push(mutation);
                }
            }
        }
        if let Ok(Some(mutation)) = self.set_block(0, EXIT_Y + 5, 0, BlockType::DragonEgg, 0) {
            self.pending_mutations.push(mutation);
        }
        if let Ok(Some(mutation)) = self.set_block(8, EXIT_Y + 1, 0, BlockType::EndGateway, 0) {
            self.pending_mutations.push(mutation);
        }
    }

    pub(crate) fn checksum(&mut self, mutations: &[WorldMutation]) -> u64 {
        // Stable FNV-1a over authoritative values. Block revisions are mixed
        // from the running XOR aggregate (updated on mutation) so idle ticks
        // do not scan the resident map. Entities contribute a cached sorted
        // fingerprint so idle (no spawn / despawn / pose / ai_phase change)
        // ticks skip sort + full-table hash.
        let mut hash = crate::rng::FNV_OFFSET;
        crate::rng::fnv1a_write(&mut hash, &self.time.to_le_bytes());
        crate::rng::fnv1a_write(&mut hash, &self.revisions.current().to_le_bytes());
        crate::rng::fnv1a_write(
            &mut hash,
            &[
                self.rules.keep_inventory as u8,
                self.rules.mob_griefing as u8,
            ],
        );
        crate::rng::fnv1a_write(
            &mut hash,
            &[
                self.rules.do_daylight_cycle as u8,
                self.rules.do_mob_spawning as u8,
                self.difficulty.as_u8(),
            ],
        );
        for mutation in mutations {
            crate::rng::fnv1a_write(&mut hash, &mutation.dimension.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &mutation.position.0.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &mutation.position.1.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &mutation.position.2.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &mutation.block.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &mutation.state.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &mutation.raw_fluid.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &mutation.revision.to_le_bytes());
        }
        crate::rng::fnv1a_write(&mut hash, &self.block_revision_checksum.to_le_bytes());
        let entity_fingerprint = self.entity_checksum_fingerprint();
        crate::rng::fnv1a_write(&mut hash, &entity_fingerprint.to_le_bytes());
        hash
    }

    pub(super) fn entity_checksum_fingerprint(&mut self) -> u64 {
        if let Some(cached) = self.entities.cached_entity_fingerprint() {
            return cached;
        }
        let mut hash = crate::rng::FNV_OFFSET;
        let mut order: Vec<usize> = (0..self.entities.entities.len()).collect();
        order.sort_unstable_by_key(|&index| {
            let entity = &self.entities.entities[index];
            (
                entity.id,
                entity.position.x.to_bits(),
                entity.position.y.to_bits(),
                entity.position.z.to_bits(),
                entity.ai_phase,
            )
        });
        for index in order {
            let entity = &self.entities.entities[index];
            crate::rng::fnv1a_write(&mut hash, &entity.id.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &entity.position.x.to_bits().to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &entity.position.y.to_bits().to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &entity.position.z.to_bits().to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &entity.ai_phase.to_le_bytes());
            crate::rng::fnv1a_write(&mut hash, &[entity.entity_type.to_wire()]);
            let legacy_payload = entity
                .dropped_item
                .map(|item| crate::inventory::ItemStack::new(item, entity.dropped_count.max(1)));
            let payload = entity.dropped_stack.as_ref().or(legacy_payload.as_ref());
            if let Some(stack) = payload {
                let wire = ItemWire::from_stack(stack);
                crate::rng::fnv1a_write(&mut hash, &wire.item.to_le_bytes());
                crate::rng::fnv1a_write(&mut hash, &wire.count.to_le_bytes());
                crate::rng::fnv1a_write(&mut hash, &wire.durability.to_le_bytes());
                crate::rng::fnv1a_write(&mut hash, &wire.enchantments);
                crate::rng::fnv1a_write(&mut hash, &wire.custom_name);
                crate::rng::fnv1a_write(&mut hash, &wire.can_break.to_le_bytes());
                crate::rng::fnv1a_write(&mut hash, &wire.can_place_on.to_le_bytes());
                if let Some(potion) = wire.potion {
                    crate::rng::fnv1a_write(
                        &mut hash,
                        &[1, potion.kind, potion.level, potion.splash as u8],
                    );
                    crate::rng::fnv1a_write(&mut hash, &potion.duration_seconds.to_le_bytes());
                } else {
                    crate::rng::fnv1a_write(&mut hash, &[0]);
                }
            } else {
                crate::rng::fnv1a_write(&mut hash, &[0]);
            }
        }
        self.entities.store_entity_fingerprint(hash);
        hash
    }
}

pub(super) fn entity_checksum_keys(entities: &EntityManager) -> Vec<(u64, u32, u32, u32, u8)> {
    entities
        .entities
        .iter()
        .map(|entity| {
            (
                entity.id,
                entity.position.x.to_bits(),
                entity.position.y.to_bits(),
                entity.position.z.to_bits(),
                entity.ai_phase,
            )
        })
        .collect()
}

/// Living entities that are truly idle skip `update_physics` and `ai_phase`
/// bumps so Plan 08 entity fingerprints stay reusable. Non-living movers
/// (drops, projectiles, vehicles) keep their existing physics paths.
pub(super) fn entity_skips_idle_physics(entity: &Entity, chasing: bool) -> bool {
    if chasing || !entity.entity_type.is_living() {
        return false;
    }
    if entity.velocity.length_squared() > ENTITY_IDLE_VELOCITY_EPS {
        return false;
    }
    entity.is_sitting
        || entity.entity_type.is_anchored()
        || entity.on_ground
        || entity.entity_type.uses_flying_physics()
}

pub(super) fn look_from_yaw_pitch(yaw: f32, pitch: f32) -> Vec3 {
    if !yaw.is_finite() || !pitch.is_finite() {
        return Vec3::NEG_Z;
    }
    let yaw = yaw.to_radians();
    let pitch = pitch.to_radians();
    let horizontal = pitch.cos();
    Vec3::new(-yaw.sin() * horizontal, -pitch.sin(), yaw.cos() * horizontal)
}

pub(super) fn block_revision_fingerprint(position: (i32, i32, i32), revision: u64) -> u64 {
    let mut hash = crate::rng::FNV_OFFSET;
    crate::rng::fnv1a_write(&mut hash, &position.0.to_le_bytes());
    crate::rng::fnv1a_write(&mut hash, &position.1.to_le_bytes());
    crate::rng::fnv1a_write(&mut hash, &position.2.to_le_bytes());
    crate::rng::fnv1a_write(&mut hash, &revision.to_le_bytes());
    hash
}

pub(super) fn operation_position(operation: &GameplayOperation) -> Option<(i32, i32, i32)> {
    match operation {
        GameplayOperation::BlockAction { x, y, z, .. }
        | GameplayOperation::Sleep { x, y, z }
        | GameplayOperation::Container { x, y, z, .. }
        | GameplayOperation::ContainerClick { x, y, z, .. }
        | GameplayOperation::FurnaceTakeOutput { x, y, z, .. }
        | GameplayOperation::Enchant { x, y, z, .. }
        | GameplayOperation::Brew { x, y, z, .. }
        | GameplayOperation::Anvil { x, y, z, .. }
        | GameplayOperation::FluidUse { x, y, z, .. } => Some((*x, *y, *z)),
        GameplayOperation::Craft {
            station: Some([x, y, z]),
            ..
        } => Some((*x, *y, *z)),
        _ => None,
    }
}


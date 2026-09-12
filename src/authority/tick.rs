use super::contract::{AuthoritySnapshot, SessionInventorySlot, WorldMutation, FIXED_TICK_HZ};
use super::{stack_from_slot, AuthorityCore};
use crate::authority::interest::chunks_around;
use crate::block_entity::BlockEntity;
use crate::dimension::Dimension;
use crate::inventory::ItemStack;
use crate::network::protocol::PlayerId;
use std::collections::{BTreeMap, BTreeSet};

impl AuthorityCore {
    /// Execute one fixed tick for every loaded dimension. Sessions are sorted
    /// by their BTreeMap key within each dimension, so AI/automation and
    /// mutation order do not depend on transport arrival order. Revisions are
    /// dimension-scoped; `(WorldMutation.dimension, revision)` is the stable
    /// routing/persistence identity.
    /// Execute one fixed tick. Tests and callers without interest unions use
    /// the empty-map fallback (session poses → `chunks_around`).
    pub fn tick(&mut self) -> AuthoritySnapshot {
        let empty = BTreeMap::new();
        self.tick_with_simulation_unions(&empty)
    }

    /// Execute one fixed tick using runtime-provided simulation unions.
    pub fn tick_with_simulation_unions(
        &mut self,
        simulation_unions: &BTreeMap<Dimension, BTreeSet<(i32, i32)>>,
    ) -> AuthoritySnapshot {
        self.apply_pending_worldgen(crate::server_runtime::MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK);
        self.fixed_tick = self.fixed_tick.wrapping_add(1).max(1);
        let dimensions: Vec<Dimension> = self.dimensions().collect();
        let mut mutations_by_dimension: BTreeMap<Dimension, Vec<WorldMutation>> = BTreeMap::new();
        let mut loaded_chunks = 0usize;
        let mut entities = 0usize;

        for dimension in dimensions.iter().copied() {
            self.tick_session_domains(dimension);
            self.tick_mining(dimension);
            self.tick_portal_travel(dimension);
            let players: Vec<(PlayerId, [f32; 3], f32, f32)> = self
                .session_ids_in_dimension(dimension)
                .iter()
                .copied()
                .filter_map(|id| {
                    self.sessions.get(&id).map(|session| {
                        (session.id, session.position, session.yaw, session.pitch)
                    })
                })
                .collect();
            let simulation_chunks = simulation_unions.get(&dimension).cloned().unwrap_or_else(|| {
                let distance = self.config.render_distance.clamp(0, 32) as u8;
                let mut union = BTreeSet::new();
                for (_, position, _, _) in &players {
                    if position.iter().all(|value| value.is_finite()) {
                        union.extend(chunks_around(*position, distance));
                    }
                }
                union
            });
            // One world_mut for tick + redstone drain; dispense needs a fresh
            // borrow so AuthorityCore can allocate global entity ids.
            let (mut world_mutations, actions) = {
                let world = self
                    .world_mut(dimension)
                    .expect("loaded dimension missing from world map");
                loaded_chunks = loaded_chunks.saturating_add(world.chunks.chunks.len());
                entities = entities.saturating_add(world.entities.entities.len());
                let world_mutations = world.tick(&players, &simulation_chunks);
                let actions = world.take_pending_redstone_actions();
                (world_mutations, actions)
            };
            for action in actions {
                let candidate = self.next_unique_entity_id(dimension, None);
                let spawned = self
                    .world_mut_expect(dimension)
                    .execute_redstone_dispense(action, candidate);
                if spawned {
                    self.claim_entity_id(candidate);
                }
            }
            world_mutations.extend(self.world_mut_expect(dimension).take_pending_mutations());
            mutations_by_dimension.insert(dimension, world_mutations);
        }
        self.last_tick_loaded_chunks = loaded_chunks;
        self.last_tick_entities = entities;

        for mutation in std::mem::take(&mut self.pending_mutations) {
            if let Some(dimension) = Dimension::from_wire(mutation.dimension) {
                mutations_by_dimension
                    .entry(dimension)
                    .or_default()
                    .push(mutation);
            }
        }

        let mut mutations = Vec::new();
        for dimension in dimensions.iter().copied() {
            let entries = mutations_by_dimension.entry(dimension).or_default();
            entries.sort_by_key(|mutation| (mutation.revision, mutation.position));
            mutations.extend(entries.iter().copied());
        }

        let mut checksums = Vec::with_capacity(dimensions.len());
        let mut revision = 0;
        for dimension in dimensions.iter().copied() {
            let entries = mutations_by_dimension
                .get(&dimension)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let world = self
                .world_mut(dimension)
                .expect("loaded dimension missing from world map");
            revision = revision.max(world.revisions.current());
            checksums.push((dimension, world.checksum(entries)));
        }

        let session_updates = self.take_dirty_session_updates();
        let snapshot = AuthoritySnapshot {
            tick: self.fixed_tick,
            revision,
            checksum: aggregate_dimension_checksums(&checksums),
            mutations,
            session_updates,
        };
        // Replace rather than cloning the previous snapshot's session vector.
        // The returned value clones only this tick's dirty session_updates.
        let _previous = std::mem::replace(&mut self.last_snapshot, snapshot);
        self.last_snapshot.clone()
    }

    fn tick_session_domains(&mut self, dimension: Dimension) {
        use crate::authority::transactions::{self, BrewTick, WorkstationContext};
        use crate::inventory::GameMode;

        let ids: Vec<_> = self.session_ids_in_dimension(dimension).to_vec();
        for id in ids {
            let Some((position, game_mode, original)) = self
                .sessions
                .get(&id)
                .map(|session| (session.position, session.game_mode, session.gameplay))
            else {
                continue;
            };
            let mut candidate = original;

            candidate.invulnerability_ticks = candidate.invulnerability_ticks.saturating_sub(1);
            candidate.shield_cooldown_ticks = candidate.shield_cooldown_ticks.saturating_sub(1);
            if candidate.shield_cooldown_ticks > 0 {
                candidate.shield_active = false;
            }
            candidate.attack_cooldown_ticks = candidate
                .attack_cooldown_ticks
                .saturating_add(1)
                .min(super::ATTACK_COOLDOWN_TICKS);

            if let Some(pending) = candidate.brew {
                let block = self
                    .world_ref(dimension)
                    .expect("loaded dimension missing from world map")
                    .get_block(pending.station[0], pending.station[1], pending.station[2]);
                let context = WorkstationContext::at(pending.station, block);
                match transactions::tick_brew(&mut candidate, context) {
                    Ok(BrewTick::Ready) => {}
                    Ok(BrewTick::Brewing { .. }) => {}
                    Err(_) => candidate.brew = None,
                }
            }

            let previous_hook = original.fishing_hook;
            if candidate.fishing_hook.is_some() {
                match self
                    .world_ref(dimension)
                    .expect("loaded dimension missing from world map")
                    .fishing_context(&candidate, position, game_mode != GameMode::Creative)
                {
                    Ok(context) => {
                        if crate::authority::fishing::tick(&mut candidate, context).is_err() {
                            candidate.fishing_hook = None;
                        }
                    }
                    Err(_) => candidate.fishing_hook = None,
                }
            }

            if candidate == original {
                continue;
            }
            let revision = {
                let world = self
                    .world_mut(dimension)
                    .expect("loaded dimension missing from world map");
                world.sync_authority_hook(previous_hook, candidate.fishing_hook, id);
                world.revisions.allocate()
            };
            candidate.revision = revision;
            if let Some(session) = self.sessions.get_mut(&id) {
                session.gameplay = candidate;
                // Autonomous cooldown, brew and hook ticks publish a newer
                // owner-private projection, but they are not a client-authored
                // transaction baseline. Advancing `last_revision` here makes
                // every in-flight reel/cancel stale before TCP ingress. This
                // matches the mining tick seam: accepted requests and durable
                // mutations advance the anti-stale baseline; fixed-tick
                // presentation progress advances only gameplay.revision.
            }
            self.mark_session_update(id);
        }
    }

    fn tick_mining(&mut self, dimension: Dimension) {
        let ids: Vec<_> = self.session_ids_in_dimension(dimension).to_vec();
        for id in ids {
            let Some((position, game_mode, progress)) = self
                .sessions
                .get(&id)
                .map(|session| (session.position, session.game_mode, session.gameplay.mining))
            else {
                continue;
            };
            let Some(progress) = progress else {
                continue;
            };
            if progress.dimension != dimension as u8 {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            let target = (progress.target[0], progress.target[1], progress.target[2]);
            let Some(block) = self
                .world_ref(dimension)
                .expect("loaded dimension missing from world map")
                .chunks
                .get_loaded_block(target.0, target.1, target.2)
            else {
                self.clear_mining_progress(id, dimension);
                continue;
            };
            let expected_block = crate::world::BlockType::from_wire(progress.block);
            let expected_state = self
                .world_ref(dimension)
                .expect("loaded dimension missing from world map")
                .get_block_state(target.0, target.1, target.2);
            if expected_block != Some(block) || expected_state != progress.state {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            let eye = glam::Vec3::from_array(position) + glam::Vec3::new(0.0, 1.62, 0.0);
            let target_center = glam::Vec3::new(
                target.0 as f32 + 0.5,
                target.1 as f32 + 0.5,
                target.2 as f32 + 0.5,
            );
            if eye.distance(target_center) > crate::interaction::PLAYER_REACH {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            if block == crate::world::BlockType::Air
                || !self
                    .world_ref(dimension)
                    .expect("loaded dimension missing from world map")
                    .has_block_line_of_sight(position, progress.look_milli, target)
            {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            let selected_index = if progress.hand == 1 {
                40
            } else {
                self.sessions
                    .get(&id)
                    .map_or(0, |session| session.gameplay.selected_hotbar_slot)
            };
            if selected_index != progress.slot_index {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            let held_matches = self
                .sessions
                .get(&id)
                .and_then(|session| session.gameplay.slot(selected_index).flatten())
                == progress.held.map(SessionInventorySlot::from);
            if !held_matches {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            let held_stack = stack_from_slot(progress.held);
            let policy = crate::game_rules::GameModePolicy::for_rules(
                game_mode,
                &self
                    .world_ref(dimension)
                    .expect("loaded dimension missing from world map")
                    .rules,
            );
            if !policy.can_break_stack(held_stack.as_ref(), block) {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            if game_mode == crate::inventory::GameMode::Creative {
                let _ = self.commit_mining_break(id, dimension, target, held_stack, game_mode);
                continue;
            }
            let duration =
                crate::authority::mining::mining_time_seconds(block, held_stack.as_ref());
            if !duration.is_finite() || duration <= 0.0 || duration == f32::MAX {
                self.clear_mining_progress(id, dimension);
                continue;
            }
            let step = ((1_000.0 / (duration * FIXED_TICK_HZ as f32)).ceil() as u16).max(1);
            let next = progress.progress_milli.saturating_add(step);
            if next >= 1_000 {
                let _ = self.commit_mining_break(id, dimension, target, held_stack, game_mode);
            } else {
                let revision = self
                    .world_mut(dimension)
                    .expect("loaded dimension missing from world map")
                    .revisions
                    .allocate();
                if let Some(session) = self.sessions.get_mut(&id) {
                    if let Some(active) = session.gameplay.mining.as_mut() {
                        active.progress_milli = next;
                    }
                    session.gameplay.revision = revision;
                }
                self.mark_session_update(id);
            }
        }
    }

    pub(crate) fn clear_mining_progress(&mut self, id: PlayerId, dimension: Dimension) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.gameplay.mining.take().is_none() {
            return;
        }
        let revision = self
            .world_mut(dimension)
            .expect("loaded dimension missing from world map")
            .revisions
            .allocate();
        if let Some(session) = self.sessions.get_mut(&id) {
            session.gameplay.revision = revision;
        }
        self.mark_session_update(id);
    }

    pub(crate) fn commit_mining_break(
        &mut self,
        id: PlayerId,
        dimension: Dimension,
        target: (i32, i32, i32),
        held_stack: Option<crate::inventory::ItemStack>,
        game_mode: crate::inventory::GameMode,
    ) -> bool {
        let Some(session) = self.sessions.get(&id) else {
            return false;
        };
        let Some(progress) = session.gameplay.mining else {
            return false;
        };
        if session.dimension != dimension as u8 || progress.target != [target.0, target.1, target.2]
        {
            return false;
        }
        let Some(old_block) = self
            .world_ref(dimension)
            .expect("loaded dimension missing from world map")
            .chunks
            .get_loaded_block(target.0, target.1, target.2)
        else {
            self.clear_mining_progress(id, dimension);
            return false;
        };
        if old_block == crate::world::BlockType::Air
            || crate::world::BlockType::from_wire(progress.block) != Some(old_block)
            || self
                .world_ref(dimension)
                .expect("loaded dimension missing from world map")
                .get_block_state(target.0, target.1, target.2)
                != progress.state
        {
            self.clear_mining_progress(id, dimension);
            return false;
        }
        let rewards = crate::authority::mining::calculate_block_break_rewards(
            old_block,
            self.world_ref(dimension)
                .expect("loaded dimension missing from world map")
                .get_block_state(target.0, target.1, target.2),
            target,
            held_stack.as_ref(),
            game_mode,
        );
        // Preflight every session-side consequence on a copy. XP/level
        // overflow or a stale durability slot must abort before any entity ID
        // is claimed or world block is changed.
        let mut next_gameplay = session.gameplay;
        if !next_gameplay.grant_experience(rewards.xp) {
            return false;
        }
        next_gameplay.mining = None;
        if rewards.tool_damaged && game_mode != crate::inventory::GameMode::Creative {
            let salt = (target.0 as u32)
                ^ (target.1 as u32).rotate_left(11)
                ^ (target.2 as u32).rotate_left(22);
            if held_stack.as_ref().is_some_and(|stack| {
                crate::enchantment::should_consume_durability(&stack.enchantments, salt)
            }) {
                let slot = usize::from(progress.slot_index);
                let Some(Some(current)) = next_gameplay.inventory.get_mut(slot) else {
                    return false;
                };
                if current.item.durability > 1 {
                    current.item.durability -= 1;
                } else {
                    next_gameplay.inventory[slot] = None;
                }
            }
        }
        // A block-entity-backed container is removed by set_block(Air).  Copy
        // its complete non-empty stacks only after every session-side
        // preflight above has succeeded, and keep the source entity untouched
        // until the block mutation commits.
        let mut drops = rewards.drops;
        if let Some(block_entity) = self
            .world_ref(dimension)
            .expect("loaded dimension missing from world map")
            .get_block_entity(target.0, target.1, target.2)
        {
            let slots: Box<dyn Iterator<Item = &ItemStack> + '_> = match block_entity {
                BlockEntity::Chest(chest) => Box::new(chest.inventory.slots.iter().flatten()),
                BlockEntity::Furnace(furnace) => Box::new(furnace.slots.iter().flatten()),
                BlockEntity::Hopper(hopper) => Box::new(hopper.slots.iter().flatten()),
                BlockEntity::Dispenser(dispenser) => Box::new(dispenser.slots.iter().flatten()),
                BlockEntity::Dropper(dropper) => Box::new(dropper.slots.iter().flatten()),
                BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {
                    Box::new(std::iter::empty())
                }
            };
            drops.extend(slots.filter(|stack| stack.count > 0).copied());
        }
        // Reserve every entity id before changing source or inventory. Gaps in
        // the global allocator are harmless; reusing an id after a failed
        // request would not be. Prepare all dropped entities first: no
        // presentation snapshot can interleave with this synchronous commit,
        // and rollback keeps a failed mutation from losing a prepared drop.
        let mut entity_ids = Vec::with_capacity(drops.len());
        for _ in &drops {
            let candidate = self.next_unique_entity_id(dimension, Some(id));
            if candidate == 0 {
                return false;
            }
            self.claim_entity_id(candidate);
            entity_ids.push(candidate);
        }
        let drop_position = [
            target.0 as f32 + 0.5,
            target.1 as f32 + 0.5,
            target.2 as f32 + 0.5,
        ];
        let mut prepared_ids = Vec::with_capacity(entity_ids.len());
        for (entity_id, stack) in entity_ids.iter().copied().zip(drops.iter().copied()) {
            if !self
                .world_mut(dimension)
                .expect("loaded dimension missing from world map")
                .spawn_dropped_item(entity_id, drop_position, stack)
            {
                for prepared_id in prepared_ids {
                    self.world_mut(dimension)
                        .expect("loaded dimension missing from world map")
                        .remove_authority_entity(prepared_id);
                }
                return false;
            }
            prepared_ids.push(entity_id);
        }
        let Ok(Some(mutation)) = self
            .world_mut(dimension)
            .expect("loaded dimension missing from world map")
            .set_block(
                target.0,
                target.1,
                target.2,
                crate::world::BlockType::Air,
                0,
            )
        else {
            for prepared_id in prepared_ids {
                self.world_mut(dimension)
                    .expect("loaded dimension missing from world map")
                    .remove_authority_entity(prepared_id);
            }
            return false;
        };
        self.pending_mutations.push(mutation);
        if let Some(session) = self.sessions.get_mut(&id) {
            session.gameplay = next_gameplay;
            session.gameplay.revision = mutation.revision;
            session.last_revision = mutation.revision;
        }
        self.mark_session_update(id);
        true
    }
}

fn aggregate_dimension_checksums(checksums: &[(Dimension, u64)]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for (dimension, checksum) in checksums {
        hash ^= u64::from(*dimension as u8);
        hash = hash.wrapping_mul(0x100000001b3);
        for byte in checksum.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

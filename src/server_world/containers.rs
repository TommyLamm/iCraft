use super::*;

impl ServerWorld {
    /// Remove one exact viewer registration and report whether it existed.
    pub fn close_container_viewer(
        &mut self,
        player_id: PlayerId,
        position: (i32, i32, i32),
    ) -> bool {
        let Some(viewers) = self.container_viewers.get_mut(&position) else {
            return false;
        };
        let removed = viewers.remove(&player_id);
        if viewers.is_empty() {
            self.container_viewers.remove(&position);
        }
        removed
    }

    /// Remove a viewer during a forced lifecycle transition (distance,
    /// interest departure, transfer, logout, or disconnect).  Unlike the
    /// explicit `ContainerAction::Close` path, this helper owns the last-viewer
    /// chest edge and queues its primary mutation exactly once.  Double-chest
    /// viewer registrations are removed as one logical session.
    pub fn close_container_viewer_forced(
        &mut self,
        player_id: PlayerId,
        position: (i32, i32, i32),
    ) -> bool {
        let positions = self.container_positions(position);
        let mut removed = false;
        for position in &positions {
            removed |= self.close_container_viewer(player_id, *position);
        }
        if !removed {
            return false;
        }
        let has_viewer = positions.iter().any(|position| {
            self.container_viewers
                .get(position)
                .is_some_and(|viewers| !viewers.is_empty())
        });
        if !has_viewer {
            if let Some(mutation) = self.set_chest_open_state(position, false) {
                self.pending_mutations.push(mutation);
            }
        }
        true
    }

    /// Forced cleanup for every container currently registered to a player.
    /// Positions are collected before mutation so the helper is safe when a
    /// double chest removes both half registrations on its first pass.
    pub fn close_container_viewers_forced(&mut self, player_id: PlayerId) {
        let positions: Vec<_> = self
            .container_viewers
            .iter()
            .filter_map(|(position, viewers)| viewers.contains(&player_id).then_some(*position))
            .collect();
        for position in positions {
            self.close_container_viewer_forced(player_id, position);
        }
    }

    pub fn take_container_closures(&mut self) -> Vec<ContainerClosure> {
        std::mem::take(&mut self.pending_container_closures)
    }

    pub fn take_pending_mutations(&mut self) -> Vec<WorldMutation> {
        std::mem::take(&mut self.pending_mutations)
    }

    /// Drain redstone dispense actions emitted by the completed fixed tick.
    /// The vector is sorted by source position/facing so an authority caller
    /// can allocate ids and execute actions independently of HashMap order.
    pub fn take_pending_redstone_actions(&mut self) -> Vec<RedstoneAction> {
        let mut actions = std::mem::take(&mut self.pending_redstone_actions);
        actions.sort_unstable_by_key(|action| match *action {
            RedstoneAction::Dispense {
                pos,
                facing,
                dropper,
            } => (pos, facing as u8, dropper as u8),
            RedstoneAction::Explode { pos } => (pos, u8::MAX - 1, 0),
            RedstoneAction::PlayNote { pos, note } => (pos, u8::MAX, note),
        });
        actions
    }

    pub fn remove_passenger(&mut self, player_id: PlayerId) {
        for entity in &mut self.entities.entities {
            entity
                .passengers
                .retain(|passenger| *passenger != player_id);
        }
    }

    pub fn remove_authority_entity(&mut self, entity_id: u64) {
        let _ = self.entities.remove_by_id(entity_id);
    }

    pub fn fishing_context(
        &self,
        gameplay: &SessionGameplayState,
        player_position: [f32; 3],
        consume_durability: bool,
    ) -> Result<FishingDomainContext, RejectReason> {
        let hook = gameplay
            .fishing_hook
            .ok_or(RejectReason::InvalidState)?;
        let probe = crate::authority::fishing::water_probe_position(gameplay)?;
        let block_position = [
            probe[0].div_euclid(1_000),
            probe[1].div_euclid(1_000),
            probe[2].div_euclid(1_000),
        ];
        let open_water = self.get_block(block_position[0], block_position[1], block_position[2])
            == BlockType::Water;
        Ok(FishingDomainContext {
            world_seed: self.seed as u64 ^ (u64::from(self.dimension as u8) << 32),
            hook_entity_id: hook.entity_id,
            player_position_milli: position_to_milli_opt(player_position)
                .ok_or(RejectReason::InvalidState)?,
            open_water,
            water_surface_y_milli: open_water
                .then_some(block_position[1].saturating_mul(1_000).saturating_add(800)),
            consume_durability,
        })
    }

    pub fn sync_authority_hook(
        &mut self,
        previous: Option<SessionFishingHookState>,
        current: Option<SessionFishingHookState>,
        player_id: PlayerId,
    ) {
        if let Some(previous) = previous {
            if current.map_or(true, |current| current.entity_id != previous.entity_id) {
                let _ = self.entities.remove_by_id(previous.entity_id);
            }
        }
        let Some(current) = current else {
            return;
        };
        let position = milli_to_vec3(current.position_milli);
        let velocity = milli_to_vec3(current.velocity_milli);
        if let Some(entity) = self.entities.get_by_id_mut(current.entity_id) {
            entity.position = position;
            entity.velocity = velocity;
            entity.owner_id = Some(player_id);
        } else {
            let mut entity =
                crate::entity::Entity::new(current.entity_id, EntityType::FishingHook, position);
            entity.velocity = velocity;
            entity.owner_id = Some(player_id);
            self.entities.entities.push(entity);
        }
        self.entities.rebuild_indexes();
    }

    pub fn take_furnace_output(
        &mut self,
        gameplay: &mut SessionGameplayState,
        position: [i32; 3],
        count: u16,
    ) -> Result<WorldMutation, RejectReason> {
        let block = self.get_block(position[0], position[1], position[2]);
        let Some(BlockEntity::Furnace(furnace)) = self
            .get_block_entity(position[0], position[1], position[2])
            .cloned()
        else {
            return Err(RejectReason::InvalidState);
        };
        let mut next_furnace = furnace;
        transactions::execute_furnace_take_output(
            gameplay,
            &mut next_furnace,
            WorkstationContext::at(position, block),
            count,
        )
        .map_err(|_| RejectReason::InvalidState)?;
        self.chunks.set_block_entity(
            position[0],
            position[1],
            position[2],
            Some(BlockEntity::Furnace(next_furnace)),
        );
        self.redstone
            .mark_container_changed(&self.chunks, (position[0], position[1], position[2]));
        Ok(self.touch_revision(position[0], position[1], position[2]))
    }

    pub fn bookshelf_power(&self, position: [i32; 3]) -> u8 {
        let mut count = 0u8;
        for y in [position[1], position[1] + 1] {
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    if dx.abs().max(dz.abs()) != 2 {
                        continue;
                    }
                    let gap = (position[0] + dx.signum(), y, position[2] + dz.signum());
                    if self.get_block(gap.0, gap.1, gap.2) == BlockType::Air
                        && self.get_block(position[0] + dx, y, position[2] + dz)
                            == BlockType::Bookshelf
                    {
                        count = count.saturating_add(1).min(15);
                    }
                }
            }
        }
        count
    }

    pub fn has_line_of_sight(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        let origin = Vec3::from_array(from) + Vec3::new(0.0, 1.62, 0.0);
        let target = Vec3::from_array(to) + Vec3::new(0.0, 0.9, 0.0);
        !crate::culling::is_los_blocked(origin, target, |x, y, z| {
            crate::culling::is_section_occluder(self.get_block(x, y, z))
        })
    }

    pub fn spawn_authority_drop(
        &mut self,
        entity_id: u64,
        slot: SessionInventorySlot,
        position: [f32; 3],
    ) -> bool {
        if entity_id == 0 || self.entities.get_by_id(entity_id).is_some() {
            return false;
        }
        let Some(stack) = slot.to_stack() else {
            return false;
        };
        let mut entity = crate::entity::Entity::new(
            entity_id,
            EntityType::DroppedItem,
            Vec3::from_array(position),
        );
        entity.dropped_item = Some(stack.item);
        entity.dropped_count = stack.count;
        entity.dropped_stack = Some(stack);
        entity.pickup_cooldown = 0.5;
        self.entities.entities.push(entity);
        self.entities.rebuild_indexes();
        true
    }

    pub fn spawn_authority_experience(
        &mut self,
        entity_id: u64,
        experience: u32,
        position: [f32; 3],
    ) -> bool {
        if entity_id == 0 || experience == 0 || self.entities.get_by_id(entity_id).is_some() {
            return false;
        }
        let mut entity = crate::entity::Entity::new(
            entity_id,
            EntityType::ExperienceOrb,
            Vec3::from_array(position),
        );
        entity.xp_value = experience;
        self.entities.entities.push(entity);
        self.entities.rebuild_indexes();
        true
    }

    pub fn container_viewers_at(
        &self,
        position: (i32, i32, i32),
    ) -> impl Iterator<Item = &PlayerId> {
        self.container_viewers
            .get(&position)
            .into_iter()
            .flat_map(|viewers| viewers.iter())
    }

    pub(super) fn container_positions(&self, position: (i32, i32, i32)) -> Vec<(i32, i32, i32)> {
        let mut positions = vec![position];
        if matches!(
            self.get_block(position.0, position.1, position.2),
            BlockType::Chest | BlockType::EndCityChest
        ) {
            if let Some(partner) = crate::block_entity::double_chest_partner(&self.chunks, position)
            {
                positions.push(partner);
            }
        }
        positions
    }

    /// Apply a binary open/closed edge to both halves of a double chest.  The
    /// primary half is returned to the caller; the partner mutation is queued
    /// so the authority tick publishes both state edges in deterministic order.
    pub(super) fn set_chest_open_state(
        &mut self,
        position: (i32, i32, i32),
        open: bool,
    ) -> Option<WorldMutation> {
        if !matches!(
            self.get_block(position.0, position.1, position.2),
            BlockType::Chest | BlockType::EndCityChest
        ) {
            return None;
        }
        let mut state = crate::world::BlockState::decode(
            self.get_block_state(position.0, position.1, position.2),
        );
        if state.is_open == open {
            return None;
        }
        state.is_open = open;
        self.chunks
            .set_block_state(position.0, position.1, position.2, state.encode());
        let primary = self.touch_revision(position.0, position.1, position.2);
        if let Some(partner) = crate::block_entity::double_chest_partner(&self.chunks, position) {
            let mut partner_state = crate::world::BlockState::decode(
                self.get_block_state(partner.0, partner.1, partner.2),
            );
            if partner_state.is_open != open {
                partner_state.is_open = open;
                self.chunks.set_block_state(
                    partner.0,
                    partner.1,
                    partner.2,
                    partner_state.encode(),
                );
                let partner_mutation = self.touch_revision(partner.0, partner.1, partner.2);
                self.pending_mutations.push(partner_mutation);
            }
        }
        Some(primary)
    }

}


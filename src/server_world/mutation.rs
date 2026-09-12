use super::*;
use super::entities::operation_position;

impl ServerWorld {
    /// Apply a real voxel mutation and return the revision-bearing event.
    pub fn set_block(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        block: BlockType,
        state: u8,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        if !self.valid_coordinate(x, y, z) {
            return Err(RejectReason::InvalidCoordinate);
        }
        let (cx, cz) = chunk_xz(x, z);
        self.materialize_chunk(cx, cz);
        if !self.chunk_is_resident(cx, cz) {
            return Err(RejectReason::InvalidState);
        }
        let old_block = self.get_block(x, y, z);
        let old_state = self.get_block_state(x, y, z);
        if old_block == block && old_state == state {
            return Ok(None);
        }
        let mut chest_partner_to_close = None;
        if old_block != block
            && matches!(
                old_block,
                BlockType::Chest
                    | BlockType::EndCityChest
                    | BlockType::Furnace
                    | BlockType::Hopper
                    | BlockType::Dispenser
                    | BlockType::Dropper
            )
        {
            let mut positions = vec![(x, y, z)];
            if matches!(old_block, BlockType::Chest | BlockType::EndCityChest) {
                if let Some(partner) =
                    crate::block_entity::double_chest_partner(&self.chunks, (x, y, z))
                {
                    positions.push(partner);
                    chest_partner_to_close = Some(partner);
                }
            }
            for position in positions {
                if let Some(viewers) = self.container_viewers.remove(&position) {
                    for player_id in viewers {
                        self.pending_container_closures.push(ContainerClosure {
                            player_id,
                            dimension: self.dimension,
                            position,
                        });
                    }
                }
            }
        }
        self.chunks.set_block(x, y, z, block);
        self.chunks.set_block_state(x, y, z, state);
        if let Some(entity) = default_stub_for_block(block) {
            if self.chunks.get_block_entity(x, y, z).is_none() {
                self.chunks.set_block_entity(x, y, z, Some(entity));
            }
        } else if self.chunks.get_block_entity(x, y, z).is_some() {
            self.chunks.set_block_entity(x, y, z, None);
        }
        let facing = crate::world::BlockState::decode(state).facing;
        self.redstone
            .on_block_changed(&self.chunks, (x, y, z), facing);
        if let Some(partner) = chest_partner_to_close {
            if matches!(
                self.get_block(partner.0, partner.1, partner.2),
                BlockType::Chest | BlockType::EndCityChest
            ) {
                let mut partner_state = crate::world::BlockState::decode(
                    self.get_block_state(partner.0, partner.1, partner.2),
                );
                if partner_state.is_open {
                    partner_state.is_open = false;
                    self.chunks.set_block_state(
                        partner.0,
                        partner.1,
                        partner.2,
                        partner_state.encode(),
                    );
                    let mutation = self.touch_revision(partner.0, partner.1, partner.2);
                    self.pending_mutations.push(mutation);
                }
            }
        }
        let revision = self.revisions.allocate();
        self.set_block_revision((x, y, z), revision);
        self.chunk_revisions
            .insert(chunk_xz(x, z), revision);

        if block == BlockType::Fire {
            if let Some(interior) =
                crate::dimension::detect_nether_frame((x, y, z), |x, y, z| self.get_block(x, y, z))
            {
                for pos in interior {
                    if let Ok(Some(mutation)) =
                        self.set_block(pos.0, pos.1, pos.2, BlockType::NetherPortal, 0)
                    {
                        self.pending_mutations.push(mutation);
                    }
                }
            }
        }
        if crate::world::BlockState::decode(state).is_open && block == BlockType::EndPortalFrame {
            if let Some(interior) =
                crate::dimension::detect_completed_end_portal((x, y, z), |x, y, z| {
                    self.get_block(x, y, z) == BlockType::EndPortalFrame
                        && crate::world::BlockState::decode(self.get_block_state(x, y, z)).is_open
                })
            {
                for pos in interior {
                    if let Ok(Some(mutation)) =
                        self.set_block(pos.0, pos.1, pos.2, BlockType::EndPortal, 0)
                    {
                        self.pending_mutations.push(mutation);
                    }
                }
            }
        }

        Ok(Some(WorldMutation {
            dimension: self.dimension as u8,
            position: (x, y, z),
            block: block.to_wire(),
            state,
            raw_fluid: self.chunks.get_fluid_raw(x, y, z),
            revision,
        }))
    }

    /// Apply one already-authorized placement. The caller owns the inventory
    /// transaction; this method only validates loaded support and publishes a
    /// revision-bearing world mutation.
    pub fn apply_block_place(
        &mut self,
        position: (i32, i32, i32),
        face: [i8; 3],
        block: BlockType,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        if face.iter().any(|component| !matches!(*component, -1..=1))
            || face
                .iter()
                .map(|component| i16::from(*component).abs())
                .sum::<i16>()
                != 1
            || block == BlockType::Air
        {
            return Err(RejectReason::InvalidState);
        }
        let (x, y, z) = position;
        if !self.valid_coordinate(x, y, z) {
            return Err(RejectReason::InvalidCoordinate);
        }
        let Some(existing) = self.chunks.get_loaded_block(x, y, z) else {
            return Err(RejectReason::InvalidState);
        };
        if existing != BlockType::Air {
            return Err(RejectReason::InvalidState);
        }
        let support = (
            x.saturating_sub(i32::from(face[0])),
            y.saturating_sub(i32::from(face[1])),
            z.saturating_sub(i32::from(face[2])),
        );
        if !self.valid_coordinate(support.0, support.1, support.2)
            || !self.chunks.can_place_block_with_support(block, x, y, z)
        {
            return Err(RejectReason::InvalidState);
        }
        self.set_block(x, y, z, block, 0)
    }

    /// Verify the authenticated pose's sightline reaches exactly one loaded
    /// block. Missing chunks are never treated as transparent by the action
    /// boundary; the target itself is checked before this helper is called.
    pub fn has_block_line_of_sight(
        &self,
        position: [f32; 3],
        look_milli: [i16; 3],
        target: (i32, i32, i32),
    ) -> bool {
        let direction = Vec3::new(
            f32::from(look_milli[0]),
            f32::from(look_milli[1]),
            f32::from(look_milli[2]),
        )
        .normalize_or_zero();
        if direction == Vec3::ZERO {
            return false;
        }
        let origin = Vec3::from_array(position) + Vec3::new(0.0, 1.62, 0.0);
        let target_vec = Vec3::new(
            target.0 as f32 + 0.5,
            target.1 as f32 + 0.5,
            target.2 as f32 + 0.5,
        );
        if origin.distance(target_vec) > crate::interaction::PLAYER_REACH {
            return false;
        }
        let Some(hit) = crate::interaction::raycast(
            origin,
            direction,
            crate::interaction::PLAYER_REACH,
            &self.chunks,
            crate::interaction::RaycastTargetPolicy::Break,
        ) else {
            return false;
        };
        (
            hit.block_pos.x as i32,
            hit.block_pos.y as i32,
            hit.block_pos.z as i32,
        ) == target
    }

    pub fn spawn_dropped_item(
        &mut self,
        entity_id: u64,
        position: [f32; 3],
        stack: crate::inventory::ItemStack,
    ) -> bool {
        if entity_id == 0 || self.entities.get_by_id(entity_id).is_some() || stack.count == 0 {
            return false;
        }
        if !self.ensure_entity(entity_id, EntityType::DroppedItem, position, 0.0) {
            return false;
        }
        let Some(entity) = self.entities.get_by_id_mut(entity_id) else {
            return false;
        };
        entity.dropped_item = Some(stack.item);
        entity.dropped_count = stack.count;
        entity.dropped_stack = Some(stack);
        true
    }

    /// Apply one authoritative water-bucket edge.  The world mutation is
    /// intentionally separate from the session inventory transaction: callers
    /// validate and prepare the exact hand slot first, then publish this
    /// revision only after the target/face has been accepted.
    pub fn apply_fluid_use(
        &mut self,
        position: (i32, i32, i32),
        face: [i8; 3],
        source: crate::inventory::Item,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        let (x, y, z) = position;
        if !self.valid_coordinate(x, y, z)
            || face.iter().any(|component| !matches!(*component, -1..=1))
            || face
                .iter()
                .map(|component| i16::from(*component).abs())
                .sum::<i16>()
                != 1
        {
            return Err(RejectReason::InvalidState);
        }
        let (cx, cz) = chunk_xz(x, z);
        self.materialize_chunk(cx, cz);
        if !self.chunk_is_resident(cx, cz) {
            return Err(RejectReason::InvalidState);
        }

        match source {
            crate::inventory::Item::WaterBucket => {
                if self.get_block(x, y, z).is_waterloggable() {
                    if self.chunks.set_waterlogged(x, y, z, true) {
                        return Ok(Some(self.touch_revision(x, y, z)));
                    }
                    return Err(RejectReason::InvalidState);
                }

                let target = (
                    x.saturating_add(i32::from(face[0])),
                    y.saturating_add(i32::from(face[1])),
                    z.saturating_add(i32::from(face[2])),
                );
                if !self.valid_coordinate(target.0, target.1, target.2) {
                    return Err(RejectReason::InvalidCoordinate);
                }
                let Some(target_block) = self.chunks.get_loaded_block(target.0, target.1, target.2)
                else {
                    return Err(RejectReason::InvalidState);
                };
                if target_block != BlockType::Air {
                    return Err(RejectReason::InvalidState);
                }
                self.chunks
                    .set_block(target.0, target.1, target.2, BlockType::Water);
                self.chunks.set_block_state(target.0, target.1, target.2, 0);
                self.chunks.set_fluid_raw(target.0, target.1, target.2, 0);
                Ok(Some(self.touch_revision(target.0, target.1, target.2)))
            }
            crate::inventory::Item::Bucket => {
                if self.chunks.is_waterlogged(x, y, z) {
                    self.chunks.set_waterlogged(x, y, z, false);
                    return Ok(Some(self.touch_revision(x, y, z)));
                }
                if self.get_block(x, y, z) != BlockType::Water
                    || self.chunks.get_fluid_level(x, y, z) != 0
                    || self.chunks.get_fluid_falling(x, y, z)
                {
                    return Err(RejectReason::InvalidState);
                }
                self.chunks.set_block(x, y, z, BlockType::Air);
                self.chunks.set_block_state(x, y, z, 0);
                self.chunks.set_fluid_raw(x, y, z, 0);
                Ok(Some(self.touch_revision(x, y, z)))
            }
            _ => Err(RejectReason::InvalidState),
        }
    }

    pub fn validate_request(
        &self,
        request: &GameplayRequest,
        dimension: Dimension,
        position: [f32; 3],
        operator: bool,
    ) -> Result<(), RejectReason> {
        if dimension != self.dimension {
            return Err(RejectReason::InvalidDimension);
        }
        if !position.iter().all(|value| value.is_finite()) {
            return Err(RejectReason::InvalidState);
        }
        if let Some((x, y, z)) = operation_position(&request.operation) {
            if !self.valid_coordinate(x, y, z) {
                return Err(RejectReason::InvalidCoordinate);
            }
            let distance = Vec3::from_array(position)
                .distance_squared(Vec3::new(x as f32, y as f32, z as f32));
            if distance > crate::interaction::player_reach_squared() {
                return Err(RejectReason::TooFar);
            }
        }
        if matches!(&request.operation, GameplayOperation::Command { .. }) && !operator {
            return Err(RejectReason::PermissionDenied);
        }
        Ok(())
    }

    pub fn ensure_entity(
        &mut self,
        entity_id: u64,
        entity_type: EntityType,
        position: [f32; 3],
        health: f32,
    ) -> bool {
        if let Some(entity) = self.entities.get_by_id(entity_id) {
            return entity.entity_type == entity_type;
        }
        let mut entity =
            crate::entity::Entity::new(entity_id, entity_type, Vec3::from_array(position));
        entity.health = health.clamp(0.0, entity.max_health);
        self.entities.entities.push(entity);
        self.entities.rebuild_indexes();
        true
    }

    /// Execute a villager offer atomically against a session's compact
    /// inventory.  Costs are checked before either item is removed and the
    /// complete state is restored if the sell stack cannot fit.
    pub fn apply_trade(
        &mut self,
        gameplay: &mut SessionGameplayState,
        villager_id: u64,
        offer_index: u16,
        player_position: [f32; 3],
    ) -> Result<(), RejectReason> {
        let Some(villager) = self.entities.get_by_id(villager_id) else {
            return Err(RejectReason::InvalidState);
        };
        if villager.entity_type != EntityType::Villager || villager.health <= 0.0 {
            return Err(RejectReason::InvalidState);
        }
        if villager
            .position
            .distance_squared(Vec3::from_array(player_position))
            > crate::interaction::player_reach_squared()
        {
            return Err(RejectReason::TooFar);
        }
        let Some(offer) = villager.offers.get(usize::from(offer_index)).cloned() else {
            return Err(RejectReason::InvalidState);
        };
        if offer.is_out_of_stock() {
            return Err(RejectReason::InvalidState);
        }
        let cost_a = offer.effective_cost_a(0.0);
        let cost_b = offer.buy_b.map(|stack| stack.count).unwrap_or(0);
        let buy_a = offer.buy_a.item.to_u32();
        let buy_b = offer.buy_b.map(|stack| stack.item.to_u32());
        if gameplay.count_item(buy_a) < cost_a
            || buy_b.is_some_and(|item| gameplay.count_item(item) < cost_b)
        {
            return Err(RejectReason::InvalidState);
        }
        let original = *gameplay;
        let _ = gameplay.remove_item(buy_a, cost_a);
        if let Some(item) = buy_b {
            if !gameplay.remove_item(item, cost_b) {
                *gameplay = original;
                return Err(RejectReason::InvalidState);
            }
        }
        let sell = SessionInventorySlot::from_wire(
            crate::network::protocol::ItemWire::from_stack(&offer.sell),
            offer.sell.can_break,
            offer.sell.can_place_on,
        );
        if !gameplay.add_slot(sell) {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        }
        if (0..crate::authority::contract::SESSION_INVENTORY_SLOTS).any(|index| {
            transactions::brew_locks_slot(&original, index as u8)
                && original.inventory[index] != gameplay.inventory[index]
        }) {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        }
        let Some(villager) = self.entities.get_by_id_mut(villager_id) else {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        };
        let Some(offer) = villager.offers.get_mut(usize::from(offer_index)) else {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        };
        offer.uses = offer.uses.saturating_add(1);
        villager.villager_xp = villager.villager_xp.saturating_add(offer.xp_reward);
        Ok(())
    }

    /// Mount or dismount a player in the headless entity graph.  Entity
    /// passenger lists are authoritative; presentation roots only project the
    /// resulting `mounted_entity` session value.
    pub fn apply_mount(
        &mut self,
        player_id: PlayerId,
        entity_id: u64,
        player_position: [f32; 3],
    ) -> Result<Option<u64>, RejectReason> {
        if entity_id == 0 {
            for entity in &mut self.entities.entities {
                entity
                    .passengers
                    .retain(|passenger| *passenger != player_id);
            }
            return Ok(None);
        }
        let Some(vehicle) = self.entities.get_by_id(entity_id) else {
            return Err(RejectReason::InvalidState);
        };
        if !matches!(
            vehicle.entity_type,
            EntityType::Boat | EntityType::Minecart | EntityType::Horse
        ) {
            return Err(RejectReason::InvalidState);
        }
        if vehicle
            .position
            .distance_squared(Vec3::from_array(player_position))
            > crate::interaction::player_reach_squared()
        {
            return Err(RejectReason::TooFar);
        }
        if vehicle.passengers.contains(&player_id) {
            return Ok(Some(entity_id));
        }
        let capacity = if vehicle.entity_type == EntityType::Boat {
            2
        } else {
            1
        };
        if vehicle.passengers.len() >= capacity {
            return Err(RejectReason::InvalidState);
        }
        for entity in &mut self.entities.entities {
            entity
                .passengers
                .retain(|passenger| *passenger != player_id);
        }
        let Some(vehicle) = self.entities.get_by_id_mut(entity_id) else {
            return Err(RejectReason::InvalidState);
        };
        vehicle.passengers.push(player_id);
        Ok(Some(entity_id))
    }

    pub(super) fn ensure_container_slot(&self, x: i32, y: i32, z: i32, slot: u16) -> Result<(), RejectReason> {
        let Some(entity) = self.chunks.get_block_entity(x, y, z) else {
            return Err(RejectReason::InvalidState);
        };
        let Some(access) = ContainerAccess::for_entity(entity) else {
            return Err(RejectReason::InvalidState);
        };
        if usize::from(slot) >= access.slot_count {
            return Err(RejectReason::InvalidState);
        }
        Ok(())
    }

    pub fn open_container(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
        player_id: PlayerId,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        self.ensure_container_slot(x, y, z, slot)?;
        let position = (x, y, z);
        let container_positions = self.container_positions(position);
        let had_viewer = container_positions.iter().any(|position| {
            self.container_viewers
                .get(position)
                .is_some_and(|viewers| !viewers.is_empty())
        });
        for position in &container_positions {
            self.container_viewers
                .entry(*position)
                .or_default()
                .insert(player_id);
        }
        let state_mutation = if !had_viewer {
            self.set_chest_open_state(position, true)
        } else {
            None
        };
        Ok(Some(
            state_mutation.unwrap_or_else(|| self.touch_revision(x, y, z)),
        ))
    }

    pub fn close_container(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
        player_id: PlayerId,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        self.ensure_container_slot(x, y, z, slot)?;
        let position = (x, y, z);
        let container_positions = self.container_positions(position);
        let mut removed = false;
        for position in &container_positions {
            removed |= self.close_container_viewer(player_id, *position);
        }
        let has_viewer = container_positions.iter().any(|position| {
            self.container_viewers
                .get(position)
                .is_some_and(|viewers| !viewers.is_empty())
        });
        let state_mutation = if removed && !has_viewer {
            self.set_chest_open_state(position, false)
        } else {
            None
        };
        Ok(Some(
            state_mutation.unwrap_or_else(|| self.touch_revision(x, y, z)),
        ))
    }

    pub fn sleep_player(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        _player_id: PlayerId,
    ) -> Result<Option<WorldMutation>, RejectReason> {
        // Bed presence is the only sleep gate. A former `sleeping_players`
        // insert-only set latched the first success forever (second `/sleep`
        // always `InvalidState`) and was never read or cleared on wake/dawn.
        if self.get_block(x, y, z) != BlockType::Bed {
            return Err(RejectReason::InvalidState);
        }
        Ok(Some(self.touch_revision(x, y, z)))
    }

    pub fn set_time(&mut self, time: u64) {
        self.time = time;
    }

    pub fn add_time(&mut self, time: u64) {
        self.time = self.time.wrapping_add(time);
    }

    pub fn set_gamerule(&mut self, rule: &str, value: Option<&str>) -> Result<(), RejectReason> {
        let Some(value) = value else {
            return Err(RejectReason::InvalidState);
        };
        if let Ok(bool_value) = value.parse::<bool>() {
            self.rules
                .set(rule, bool_value)
                .map_err(|_| RejectReason::InvalidState)?;
        } else if matches!(
            rule,
            "playerssleepingpercentage" | "sleepingpercentage" | "sleeping_percentage"
        ) {
            let percentage = value
                .parse::<u8>()
                .map_err(|_| RejectReason::InvalidState)?;
            self.rules.set_sleeping_percentage(percentage);
        } else {
            return Err(RejectReason::InvalidState);
        }
        Ok(())
    }

    pub(super) fn touch_revision(&mut self, x: i32, y: i32, z: i32) -> WorldMutation {
        let revision = self.revisions.allocate();
        self.set_block_revision((x, y, z), revision);
        self.chunk_revisions
            .insert(chunk_xz(x, z), revision);
        WorldMutation {
            dimension: self.dimension as u8,
            position: (x, y, z),
            block: self.get_block(x, y, z).to_wire(),
            state: self.get_block_state(x, y, z),
            raw_fluid: self.chunks.get_fluid_raw(x, y, z),
            revision,
        }
    }

    /// Record a fluid-only (or fluid-plus-block) mutation that has already
    /// been applied by the authoritative fluid carrier.  Fluid ticking writes
    /// directly to `WorldColumns` so it can schedule neighboring cells; this
    /// helper is the single revision/publication seam for that write.
    pub(super) fn record_fluid_mutation(&mut self, mutation: FluidMutation) -> WorldMutation {
        let (x, y, z) = mutation.position;
        let revision = self.revisions.allocate();
        self.set_block_revision((x, y, z), revision);
        self.chunk_revisions
            .insert(chunk_xz(x, z), revision);
        WorldMutation {
            dimension: self.dimension as u8,
            position: mutation.position,
            block: mutation.block.to_wire(),
            state: self.get_block_state(x, y, z),
            raw_fluid: mutation.raw_fluid,
            revision,
        }
    }

    /// Build the simulation-union set from player poses (tests / AuthorityCore
    /// fallback when runtime interest is not supplying a cached union).
    pub fn simulation_union_from_players(
        &self,
        players: &[(PlayerId, [f32; 3], f32, f32)],
    ) -> BTreeSet<(i32, i32)> {
        let simulation_distance = self.chunks.simulation_distance.clamp(0, 32) as u8;
        let mut simulation_chunks = BTreeSet::new();
        for (_, position, _, _) in players {
            if position.iter().all(|value| value.is_finite()) {
                simulation_chunks.extend(chunks_around(*position, simulation_distance));
            }
        }
        simulation_chunks
    }

    pub(super) fn refresh_plate_occupants(&mut self, players: &[(PlayerId, [f32; 3], f32, f32)]) {
        let mut columns: Vec<(PlayerId, i32, i32)> = players
            .iter()
            .filter(|(_, position, _, _)| position.iter().all(|value| value.is_finite()))
            .map(|(id, position, _, _)| {
                (
                    *id,
                    (position[0] / 16.0).floor() as i32,
                    (position[2] / 16.0).floor() as i32,
                )
            })
            .collect();
        columns.sort_unstable();
        if columns == self.plate_occupant_columns {
            // Same columns: still refresh block cells so plates track walking
            // within a chunk without rebuilding the column key.
            self.plate_occupants.clear();
            for (_, position, _, _) in players {
                if position.iter().all(|value| value.is_finite()) {
                    self.plate_occupants.push((
                        position[0].floor() as i32,
                        position[1].floor() as i32,
                        position[2].floor() as i32,
                    ));
                }
            }
            self.plate_occupants.sort_unstable();
            self.plate_occupants.dedup();
            return;
        }
        self.plate_occupant_columns = columns;
        self.plate_occupants.clear();
        for (_, position, _, _) in players {
            if position.iter().all(|value| value.is_finite()) {
                self.plate_occupants.push((
                    position[0].floor() as i32,
                    position[1].floor() as i32,
                    position[2].floor() as i32,
                ));
            }
        }
        self.plate_occupants.sort_unstable();
        self.plate_occupants.dedup();
    }

}


//! Leftover village / raid, vehicle / fishing, furnace, and hopper ticks.
//!
//! Live Singleplayer / Host never enter these methods: they already early-return
//! when an embedded runtime or Join client is present. This module is a child
//! of `state` (`#[path]`) so it can see private `State` fields without making
//! leftover simulation authoritative.

use super::*;

impl State {
    pub fn update_village_and_raid_systems(&mut self, dt: f32) {
        if self.has_in_process_runtime() {
            return;
        }
        if self.player_state.hero_of_the_village_timer > 0.0 {
            self.player_state.hero_of_the_village_timer =
                (self.player_state.hero_of_the_village_timer - dt).max(0.0);
        }

        if !self.is_authoritative() {
            return;
        }

        let dim = self.current_dimension;
        self.poi_manager.update_village_clusters(dim);

        let player_pos = (
            self.player_physics.position.x.floor() as i32,
            self.player_physics.position.y.floor() as i32,
            self.player_physics.position.z.floor() as i32,
        );

        // Check Bad Omen raid trigger
        let triggered_village_data = if self.player_state.bad_omen_level > 0 {
            self.poi_manager
                .villages
                .iter()
                .find(|v| {
                    v.dimension == dim
                        && (v.center.0 - player_pos.0).abs() <= 48
                        && (v.center.2 - player_pos.2).abs() <= 48
                })
                .map(|v| (v.id, v.center))
        } else {
            None
        };

        if let Some((v_id, v_center)) = triggered_village_data {
            let omen_level = self.player_state.bad_omen_level;
            self.player_state.bad_omen_level = 0;
            let raid_id = self
                .raid_manager
                .trigger_raid(v_id, v_center, dim, omen_level);
            self.trigger_advancement(crate::advancements::AdvancementTrigger::VoluntaryExile);

            let wave_info = crate::village::raid::RaidWave::for_wave(1);
            let mut spawned_ids = Vec::new();

            for i in 0..wave_info.pillager_count {
                let offset_x = i as i32 * 3 - 5;
                let offset_z = 24;
                let spawn_pos = Vec3::new(
                    (v_center.0 + offset_x) as f32,
                    (v_center.1 + 1) as f32,
                    (v_center.2 + offset_z) as f32,
                );
                let id = self
                    .entity_manager
                    .spawn(crate::entity::EntityType::Pillager, spawn_pos);
                if i == 0 {
                    if let Some(idx) = self.entity_manager.id_to_index.get(&id).copied() {
                        self.entity_manager.entities[idx].is_raid_captain = true;
                    }
                }
                spawned_ids.push(id);
            }

            if let Some(raid) = self.raid_manager.get_raid_mut(raid_id) {
                raid.spawned_mob_ids = spawned_ids;
            }
        }

        // Tick active raids
        let mut raid_victories = Vec::new();
        let raid_ids: Vec<u64> = self.raid_manager.active_raids.keys().copied().collect();

        for raid_id in raid_ids {
            let mut spawn_next_wave = false;
            let mut wave_to_spawn = 1;
            let mut raid_center = (0, 0, 0);

            if let Some(raid) = self.raid_manager.get_raid_mut(raid_id) {
                if raid.is_active() {
                    raid.spawned_mob_ids.retain(|id| {
                        self.entity_manager
                            .id_to_index
                            .get(id)
                            .map(|&idx| self.entity_manager.entities[idx].health > 0.0)
                            .unwrap_or(false)
                    });

                    if raid.spawned_mob_ids.is_empty() {
                        if raid.wave_timer > 0.0 {
                            raid.wave_timer -= dt;
                        } else if raid.current_wave >= raid.max_waves {
                            raid.status = crate::village::raid::RaidStatus::Victory;
                            raid_victories.push(raid.id);
                        } else {
                            raid.current_wave += 1;
                            raid.wave_timer = 5.0;
                            spawn_next_wave = true;
                            wave_to_spawn = raid.current_wave;
                            raid_center = raid.center;
                        }
                    }
                }
            }

            if spawn_next_wave {
                let wave_info = crate::village::raid::RaidWave::for_wave(wave_to_spawn);
                let mut new_mob_ids = Vec::new();

                for i in 0..wave_info.pillager_count {
                    let offset_x = i as i32 * 3 - 5;
                    let offset_z = 20 + wave_to_spawn as i32 * 4;
                    let spawn_pos = Vec3::new(
                        (raid_center.0 + offset_x) as f32,
                        (raid_center.1 + 1) as f32,
                        (raid_center.2 + offset_z) as f32,
                    );
                    let id = self
                        .entity_manager
                        .spawn(crate::entity::EntityType::Pillager, spawn_pos);
                    if i == 0 {
                        if let Some(idx) = self.entity_manager.id_to_index.get(&id).copied() {
                            self.entity_manager.entities[idx].is_raid_captain = true;
                        }
                    }
                    new_mob_ids.push(id);
                }

                for i in 0..wave_info.ravager_count {
                    let spawn_pos = Vec3::new(
                        (raid_center.0 + i as i32 * 4) as f32,
                        (raid_center.1 + 1) as f32,
                        (raid_center.2 + 25) as f32,
                    );
                    let id = self
                        .entity_manager
                        .spawn(crate::entity::EntityType::Ravager, spawn_pos);
                    new_mob_ids.push(id);
                }

                if let Some(raid) = self.raid_manager.get_raid_mut(raid_id) {
                    raid.spawned_mob_ids = new_mob_ids;
                }
            }
        }

        for _ in raid_victories {
            self.player_state.hero_of_the_village_timer = 2400.0;
            self.trigger_advancement(crate::advancements::AdvancementTrigger::HeroOfTheVillage);
        }

        // Tick Villagers, Iron Golems, Pillagers
        let mut new_baby_spawns = Vec::new();
        let mut pillager_attack_positions = Vec::new();
        let mut golem_attack_positions = Vec::new();

        for entity in self.entity_manager.entities.iter_mut() {
            if entity.health <= 0.0 {
                continue;
            }

            match entity.entity_type {
                crate::entity::EntityType::Villager => {
                    if entity.age < 0.0 {
                        entity.age += dt;
                    }
                    if entity.breed_cooldown > 0.0 {
                        entity.breed_cooldown = (entity.breed_cooldown - dt).max(0.0);
                    }

                    let vpos = (
                        entity.position.x.floor() as i32,
                        entity.position.y.floor() as i32,
                        entity.position.z.floor() as i32,
                    );

                    if entity.profession == crate::village::poi::VillagerProfession::Unemployed {
                        for prof in [
                            crate::village::poi::VillagerProfession::Farmer,
                            crate::village::poi::VillagerProfession::Librarian,
                            crate::village::poi::VillagerProfession::Armorer,
                            crate::village::poi::VillagerProfession::Cleric,
                        ] {
                            if let Some(job_pos) = self.poi_manager.claim_poi(
                                dim,
                                crate::village::poi::PoiType::JobSite(prof),
                                entity.id,
                                vpos,
                                32.0,
                            ) {
                                entity.profession = prof;
                                entity.job_poi = Some(job_pos);
                                entity.offers = crate::village::trade::generate_offers_for_level(
                                    prof,
                                    crate::village::trade::VillagerLevel::Novice,
                                );
                                break;
                            }
                        }
                    } else if entity.job_poi.is_none() && entity.villager_xp == 0 {
                        entity.profession = crate::village::poi::VillagerProfession::Unemployed;
                        entity.offers.clear();
                    }

                    if let Some(job_pos) = entity.job_poi {
                        let dist_sq = (vpos.0 - job_pos.0).pow(2)
                            + (vpos.1 - job_pos.1).pow(2)
                            + (vpos.2 - job_pos.2).pow(2);
                        if dist_sq <= 9 && entity.restock_count_today < 2 {
                            entity.restock_count_today += 1;
                            for offer in &mut entity.offers {
                                offer.uses = 0;
                            }
                        }
                    }

                    if entity.home_poi.is_none() {
                        if let Some(bed_pos) = self.poi_manager.claim_poi(
                            dim,
                            crate::village::poi::PoiType::Bed,
                            entity.id,
                            vpos,
                            32.0,
                        ) {
                            entity.home_poi = Some(bed_pos);
                        }
                    }

                    if entity.age >= 0.0 && entity.food_count >= 3 && entity.breed_cooldown <= 0.0 {
                        let unclaimed_beds = self
                            .poi_manager
                            .get_unclaimed_beds_in_radius(dim, vpos, 32.0);
                        if unclaimed_beds > 0 {
                            entity.food_count -= 3;
                            entity.breed_cooldown = 300.0;
                            new_baby_spawns.push(entity.position);
                        }
                    }
                }
                crate::entity::EntityType::Pillager => {
                    entity.action_cooldown -= dt;
                    if entity.action_cooldown <= 0.0 {
                        pillager_attack_positions.push(entity.position);
                        entity.action_cooldown = 1.5;
                    }
                }
                crate::entity::EntityType::IronGolem => {
                    entity.action_cooldown -= dt;
                    if entity.action_cooldown <= 0.0 {
                        golem_attack_positions.push(entity.position);
                        entity.action_cooldown = 1.0;
                    }
                }
                _ => {}
            }
        }

        for pos in pillager_attack_positions {
            let target_id = self
                .entity_manager
                .query_radius_types(pos, 16.0, &[crate::entity::EntityType::Villager])
                .find(|v| v.health > 0.0)
                .map(|v| v.id);
            if let Some(vid) = target_id {
                if let Some(idx) = self.entity_manager.id_to_index.get(&vid).copied() {
                    self.entity_manager.entities[idx].health =
                        (self.entity_manager.entities[idx].health - 4.0).max(0.0);
                }
            }
        }

        for pos in golem_attack_positions {
            let target_id = self
                .entity_manager
                .query_radius_types(
                    pos,
                    16.0,
                    &[
                        crate::entity::EntityType::Pillager,
                        crate::entity::EntityType::Zombie,
                        crate::entity::EntityType::Ravager,
                    ],
                )
                .find(|h| h.health > 0.0)
                .map(|h| h.id);
            if let Some(hid) = target_id {
                if let Some(idx) = self.entity_manager.id_to_index.get(&hid).copied() {
                    self.entity_manager.entities[idx].health =
                        (self.entity_manager.entities[idx].health - 12.0).max(0.0);
                }
            }
        }

        for pos in new_baby_spawns {
            let baby_id = self
                .entity_manager
                .spawn(crate::entity::EntityType::Villager, pos);
            if let Some(idx) = self.entity_manager.id_to_index.get(&baby_id).copied() {
                self.entity_manager.entities[idx].age = -1200.0;
            }
        }

        for village in &self.poi_manager.villages {
            if village.bed_count >= 3 {
                let golem_count = self
                    .entity_manager
                    .query_radius_types(
                        Vec3::new(
                            village.center.0 as f32,
                            village.center.1 as f32,
                            village.center.2 as f32,
                        ),
                        48.0,
                        &[crate::entity::EntityType::IronGolem],
                    )
                    .count();
                if golem_count == 0 {
                    let spawn_pos = Vec3::new(
                        village.center.0 as f32 + 2.0,
                        village.center.1 as f32 + 1.0,
                        village.center.2 as f32 + 2.0,
                    );
                    self.entity_manager
                        .spawn(crate::entity::EntityType::IronGolem, spawn_pos);
                }
            }
        }
    }
    pub fn update_vehicles_and_fishing(&mut self, dt: f32) {
        if !self.presentation_topology().is_legacy_owner() {
            return;
        }
        if self.is_authoritative() {
            let entity_ids: Vec<(u64, crate::entity::EntityType)> = self
                .entity_manager
                .entities
                .iter()
                .map(|e| (e.id, e.entity_type))
                .collect();

            for (id, etype) in entity_ids {
                match etype {
                    crate::entity::EntityType::Boat => {
                        if let Some(idx) = self.entity_manager.id_to_index.get(&id).copied() {
                            let entity = &mut self.entity_manager.entities[idx];
                            let mut boat =
                                crate::vehicle::BoatState::new(entity.position, entity.yaw);
                            let cm = &self.chunk_manager;
                            boat.tick(
                                dt,
                                |x, y, z| cm.get_block(x, y, z) == BlockType::Water,
                                |x, y, z| cm.get_block(x, y, z).properties().is_solid,
                            );
                            entity.position = boat.pos_vec3();
                            entity.yaw = boat.yaw;
                        }
                    }
                    crate::entity::EntityType::Minecart => {
                        if let Some(idx) = self.entity_manager.id_to_index.get(&id).copied() {
                            let entity = &mut self.entity_manager.entities[idx];
                            let mut cart = crate::rail::MinecartState::new(entity.position);
                            cart.set_vel(entity.velocity);
                            let cm = &self.chunk_manager;
                            cart.tick(
                                dt,
                                |x, y, z| {
                                    let b = cm.get_block(x, y, z);
                                    let rtype = match b {
                                        BlockType::Rail => Some(crate::rail::RailType::Normal),
                                        BlockType::PoweredRail => {
                                            Some(crate::rail::RailType::Powered)
                                        }
                                        BlockType::DetectorRail => {
                                            Some(crate::rail::RailType::Detector)
                                        }
                                        BlockType::ActivatorRail => {
                                            Some(crate::rail::RailType::Activator)
                                        }
                                        _ => None,
                                    }?;
                                    Some((rtype, crate::rail::RailShape::NorthSouth, false))
                                },
                                |_x, _y, _z, _p| {},
                            );
                            entity.position = cart.pos_vec3();
                            entity.velocity = cart.vel_vec3();
                        }
                    }
                    _ => {}
                }
            }

            // Synchronize mounted passenger positions
            let local_player_id = 0u64;
            if let Some(vehicle_id) = self.mount_manager.get_vehicle(local_player_id) {
                if let Some(idx) = self.entity_manager.id_to_index.get(&vehicle_id).copied() {
                    let vehicle = &self.entity_manager.entities[idx];
                    let passengers = self.mount_manager.get_passengers(vehicle_id);
                    if let Some(seat_idx) = passengers.iter().position(|&id| id == local_player_id)
                    {
                        let offset = match vehicle.entity_type {
                            crate::entity::EntityType::Boat => {
                                crate::vehicle::BoatState::seat_offset(seat_idx)
                            }
                            _ => crate::vehicle::SeatOffset::new(0.0, 0.75, 0.0),
                        };
                        let seat_pos = offset.world_position(vehicle.position, vehicle.yaw);
                        self.player_physics.position = seat_pos;
                        self.player_physics.velocity = glam::Vec3::ZERO;
                    }
                }
            }
        }

        // Fishing manager tick
        let mut player_positions = std::collections::HashMap::new();
        player_positions.insert(0u64, self.player_physics.position);
        for (&id, remote) in &self.remote_players {
            player_positions.insert(
                id,
                remote
                    .snapshots
                    .back()
                    .map(|s| s.position)
                    .unwrap_or(glam::Vec3::ZERO),
            );
        }

        let cm = &self.chunk_manager;
        self.fishing_manager.tick(
            dt,
            &player_positions,
            |x, y, z| cm.get_block(x, y, z) == BlockType::Water,
            |_pos| {},
        );
    }
    pub(super) fn update_hopper_power_states(&mut self) {
        if !self.presentation_topology().is_legacy_owner() {
            return;
        }
        let mut positions = Vec::new();
        for (&(cx, cz), chunk) in &self.chunk_manager.chunks {
            for (local, entity) in chunk.iter_block_entities() {
                if matches!(entity, crate::block_entity::BlockEntity::Hopper(_)) {
                    positions.push((
                        cx * CHUNK_WIDTH as i32 + local.0 as i32,
                        local.1 as i32,
                        cz * CHUNK_DEPTH as i32 + local.2 as i32,
                    ));
                }
            }
        }
        positions.sort_unstable();
        for (x, y, z) in positions {
            let powered = self
                .redstone
                .block_state_at(&self.chunk_manager, (x, y, z))
                .power
                > 0;
            if let Some(crate::block_entity::BlockEntity::Hopper(hopper)) =
                self.chunk_manager.get_block_entity_mut(x, y, z)
            {
                if hopper.is_powered != powered {
                    hopper.is_powered = powered;
                    hopper.revision = hopper.revision.wrapping_add(1);
                    self.chunk_manager.mark_block_entity_dirty(x, z);
                    self.redstone
                        .mark_container_changed(&self.chunk_manager, (x, y, z));
                    let entity = self.chunk_manager.get_block_entity(x, y, z).cloned();
                    self.broadcast_block_entity_delta(x, y, z, entity);
                }
            }
        }
    }
    pub(super) fn update_furnaces(&mut self, dt: f32) {
        if !self.presentation_topology().is_legacy_owner() {
            return;
        }
        self.furnace_tick_timer += dt;
        while self.furnace_tick_timer >= 0.05 {
            self.furnace_tick_timer -= 0.05;

            let mut block_changes = Vec::new();
            let mut entity_changes = Vec::new();
            for ((cx, cz), chunk) in self.chunk_manager.chunks.iter_mut() {
                let chunk_x = *cx;
                let chunk_z = *cz;
                let local_entities: Vec<((u8, i16, u8), crate::block_entity::BlockEntity)> = chunk
                    .iter_block_entities()
                    .map(|(pos, e)| (pos, e.clone()))
                    .collect();

                for ((bx, by, bz), entity) in local_entities {
                    if let crate::block_entity::BlockEntity::Furnace(mut furnace) = entity {
                        let tick_res = furnace.tick(&self.recipe_manager);
                        if tick_res.slot_changed || tick_res.lit_changed {
                            furnace.revision = furnace.revision.wrapping_add(1);
                            let is_lit = furnace.is_lit;
                            let slots_wire: Vec<_> = furnace
                                .slots
                                .iter()
                                .map(|s| {
                                    s.as_ref()
                                        .map(crate::network::protocol::ItemWire::from_stack)
                                })
                                .collect();

                            let updated_entity =
                                crate::block_entity::BlockEntity::Furnace(furnace.clone());
                            let _ = chunk.insert_block_entity(bx, by, bz, updated_entity.clone());
                            let world_x = chunk_x * 16 + bx as i32;
                            let world_y = by as i32;
                            let world_z = chunk_z * 16 + bz as i32;
                            entity_changes.push(((world_x, world_y, world_z), updated_entity));

                            if tick_res.lit_changed {
                                let new_block = if is_lit {
                                    BlockType::FurnaceLit
                                } else {
                                    BlockType::Furnace
                                };
                                block_changes.push(((world_x, world_y, world_z), new_block));
                            }

                            let has_session = self
                                .container_sessions
                                .sessions
                                .iter()
                                .filter(|session| {
                                    session.dimension == self.current_dimension as u8
                                        && session.x == world_x
                                        && session.y == world_y
                                        && session.z == world_z
                                })
                                .next()
                                .is_some();
                            for session in
                                self.container_sessions
                                    .sessions
                                    .iter_mut()
                                    .filter(|session| {
                                        session.dimension == self.current_dimension as u8
                                            && session.x == world_x
                                            && session.y == world_y
                                            && session.z == world_z
                                    })
                            {
                                session.revision = furnace.revision;
                            }
                            if has_session {
                                for slot_idx in 0..3 {
                                    self.network.broadcast_container_slot_update(
                                        self.current_dimension as u8,
                                        furnace.revision,
                                        world_x,
                                        world_y,
                                        world_z,
                                        slot_idx as u16,
                                        slots_wire.get(slot_idx).cloned().flatten(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if !block_changes.is_empty() {
                self.apply_block_changes(&block_changes);
            }
            for ((x, y, z), entity) in entity_changes {
                self.chunk_manager.mark_block_entity_dirty(x, z);
                self.redstone
                    .mark_container_changed(&self.chunk_manager, (x, y, z));
                self.broadcast_block_entity_delta(x, y, z, Some(entity));
            }
        }
    }
}

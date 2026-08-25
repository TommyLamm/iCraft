//! Leftover renderer-owned click, item-use, container open, and block mutation.
//!
//! Live Embedded / Join never enter these leftover bodies. This module is a
//! child of `state` (`#[path]`) so it can see private `State` fields without
//! making leftover interaction authoritative. Compiles only under `cfg(test)` or
//! feature `legacy_owner`.

use super::*;

impl State {
    pub fn calculate_mining_time(&self, block: BlockType) -> f32 {
        if self.game_mode == GameMode::Creative {
            return 0.0;
        }
        let hardness = block.properties().hardness;
        if hardness < 0.0 {
            return f32::MAX; // Unbreakable (e.g. bedrock)
        }

        let held_stack = self.inventory.hotbar[self.inventory.selected];
        let held_item = held_stack.map(|s| s.item).unwrap_or(Item::Air);
        let preferred = block.preferred_tool();

        let mut speed_multiplier = 1.0;
        let mut matching_tool = false;

        if let Some(tool_prop) = held_item.tool_properties() {
            if tool_prop.tool_type == preferred && preferred != ToolType::None {
                speed_multiplier = tool_prop.mining_speed;
                matching_tool = true;
            }
        }

        let base_time = if matching_tool || preferred == ToolType::None {
            hardness * 1.5
        } else {
            hardness * 5.0
        };

        let enchantment_multiplier = held_stack
            .map(|stack| crate::enchantment::mining_speed_multiplier(&stack.enchantments))
            .unwrap_or(1.0);
        base_time / (speed_multiplier * enchantment_multiplier)
    }

    pub fn set_block_and_broadcast(
        &mut self,
        requester: crate::network::protocol::PlayerId,
        x: i32,
        y: i32,
        z: i32,
        block_wire: u32,
        state: u8,
    ) {
        let lighting_started = Instant::now();
        let block = match BlockType::from_wire(block_wire) {
            Some(b) => b,
            None => return,
        };
        if !validate_remote_block_request(&self.remote_players, requester, (x, y, z))
            || !self.can_place_block_at(x, y, z, block)
        {
            return;
        }
        let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
            return;
        };
        if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
            return;
        }
        if !self
            .chunk_manager
            .can_place_block_with_support(block, x, y, z)
        {
            return;
        }
        let prev = self.chunk_manager.get_block(x, y, z);
        let prev_state = self.chunk_manager.get_block_state(x, y, z);
        if prev == block && prev_state == state {
            // Echo the authoritative value to correct a requesting client's
            // prediction, but do not mark an unchanged chunk as mutated.
            let cx = x.div_euclid(CHUNK_WIDTH as i32);
            let cz = z.div_euclid(CHUNK_DEPTH as i32);
            let revision = self
                .mutation_revisions
                .latest(self.current_dimension, cx, cz);
            self.network.broadcast_block_change(
                self.current_dimension,
                revision,
                x,
                y,
                z,
                block_wire,
                state,
            );
            return;
        }
        self.play_chest_state_edge((x, y, z), prev, prev_state, block, state);
        let Some(mut dirty_chunks) =
            apply_synced_block_change(&mut self.chunk_manager, x, y, z, block, state)
        else {
            return;
        };
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (x, y, z),
            crate::redstone::Direction::North,
        );
        self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Block);
        self.broadcast_block_change(x, y, z, block);
        let lighting_elapsed = lighting_started.elapsed();
        self.lighting_time_frame += lighting_elapsed;
        self.lighting_scopes_frame.record(
            crate::perf::LightingSource::Block as usize,
            lighting_elapsed,
        );
    }

    pub fn break_block(&mut self, pos: glam::Vec3) {
        if !self.presentation_topology().is_legacy_owner() {
            let _ = self.submit_local_authority_block_action(
                crate::network::protocol::BlockActionKind::StartBreak,
                pos.x as i32,
                pos.y as i32,
                pos.z as i32,
                [0, 0, 0],
                BlockType::Air,
            );
            return;
        }
        let lighting_started = Instant::now();
        let wx = pos.x as i32;
        let wy = pos.y as i32;
        let wz = pos.z as i32;
        let old_block = self.chunk_manager.get_block(wx, wy, wz);
        if old_block == BlockType::Air {
            return;
        }
        let old_state_raw = self.chunk_manager.get_block_state(wx, wy, wz);
        let old_state = crate::world::BlockState::decode(old_state_raw);
        let chest_partner = if old_block == crate::world::BlockType::Chest
            && old_state.chest_type != crate::world::ChestType::Single
        {
            self.double_chest_partner((wx, wy, wz), old_state.chest_type)
        } else {
            None
        };

        // Chest-specific: extract inventory before the block is destroyed,
        // and handle double-chest partner revert.
        let _chest_inventory_dropped = false;
        if old_block == crate::world::BlockType::Chest {
            let _ = self.drop_chest_inventory((wx, wy, wz));
            self.close_legacy_container_sessions_at((wx, wy, wz));
            if let Some(partner) = chest_partner {
                self.close_legacy_container_sessions_at(partner);
            }
            // If this was part of a double chest, revert the partner to single.
            if let Some(partner) = chest_partner {
                let partner_raw = self
                    .chunk_manager
                    .get_block_state(partner.0, partner.1, partner.2);
                let mut partner_state = crate::world::BlockState::decode(partner_raw);
                partner_state.chest_type = crate::world::ChestType::Single;
                self.chunk_manager.set_block_state(
                    partner.0,
                    partner.1,
                    partner.2,
                    partner_state.encode(),
                );
            }
        }

        self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (wx, wy, wz),
            crate::redstone::Direction::North,
        );
        println!("[Debug] Block mined at ({}, {}, {})", wx, wy, wz);

        let sound_pos = glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
        let listener_right =
            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
        if let Some(mat) = old_block.sound_material() {
            self.audio_manager.play_sound_3d(
                crate::audio::SoundId::BlockBreak(mat),
                sound_pos,
                self.camera.position,
                listener_right,
            );
        }

        // Spawn block-break debris particles (15-25 small quads textured from
        // the broken block's atlas tile).
        {
            let mut rng = (wx as u32)
                .wrapping_mul(2654435761)
                .wrapping_add(wy as u32)
                .wrapping_mul(40503)
                .wrapping_add(wz as u32)
                .wrapping_add(self.total_time.to_bits());
            let count = 15 + (rng % 11) as usize;
            crate::particles::spawn_block_debris(
                &mut self.particles,
                sound_pos,
                old_block,
                count,
                &mut rng,
            );
        }

        let held_stack = self.inventory.hotbar[self.inventory.selected];
        let rewards = calculate_block_break_rewards(
            old_block,
            old_state_raw,
            (wx, wy, wz),
            held_stack.as_ref(),
            self.game_mode,
        );

        for drop in rewards.drops {
            self.spawn_dropped_item(drop.item, sound_pos);
        }
        if rewards.xp > 0 {
            self.player_state.add_experience(rewards.xp);
        }
        if rewards.exhaustion > 0.0 {
            self.player_state.add_exhaustion(rewards.exhaustion);
        }
        if rewards.tool_damaged {
            self.damage_selected_tool(
                (wx as u32) ^ (wy as u32).rotate_left(11) ^ (wz as u32).rotate_left(22),
            );
        }

        // recalculate lighting and redraw chunk
        let mut dirty_chunks = std::collections::HashSet::new();
        crate::lighting::update_sky_light_after_removed(
            &mut self.chunk_manager,
            wx,
            wy,
            wz,
            &mut dirty_chunks,
        );
        crate::lighting::update_block_light_after_removed(
            &mut self.chunk_manager,
            wx,
            wy,
            wz,
            old_block.properties().light_emission,
            &mut dirty_chunks,
        );

        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);

        if old_block == BlockType::OakDoor {
            let other_y = if old_state.is_top { wy - 1 } else { wy + 1 };
            if self.chunk_manager.get_block(wx, other_y, wz) == BlockType::OakDoor {
                self.chunk_manager
                    .set_block(wx, other_y, wz, BlockType::Air);
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    other_y,
                    wz,
                    &mut dirty_chunks,
                );
                mark_block_mesh_dependencies(&mut dirty_chunks, wx, other_y);
                self.broadcast_block_change(wx, other_y, wz, BlockType::Air);
            }
        }

        self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);

        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

        // Fan the authoritative break out to connected clients.
        self.broadcast_block_change(wx, wy, wz, BlockType::Air);
        let lighting_elapsed = lighting_started.elapsed();
        self.lighting_time_frame += lighting_elapsed;
        self.lighting_scopes_frame.record(
            crate::perf::LightingSource::Block as usize,
            lighting_elapsed,
        );
    }

    /// Leftover renderer-owned click path. Live Embedded / Join never enter here.
    pub(super) fn legacy_handle_click(&mut self, is_left_click: bool) {
        if !is_left_click && self.legacy_try_use_held_item() {
            return;
        }

        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();

        if !is_left_click {
            let mut closest_entity: Option<(u64, f32)> = None;
            for entity in self.entity_manager.query_radius(self.camera.position, 4.0) {
                if entity.entity_type == crate::entity::EntityType::Arrow
                    || entity.entity_type == crate::entity::EntityType::HeartParticle
                {
                    continue;
                }
                let aabb = entity.get_aabb();
                if let Some(dist) =
                    crate::entity::ray_intersects_aabb(self.camera.position, dir, &aabb)
                {
                    if dist <= 4.0 {
                        if let Some((_, closest_dist)) = closest_entity {
                            if dist < closest_dist {
                                closest_entity = Some((entity.id, dist));
                            }
                        } else {
                            closest_entity = Some((entity.id, dist));
                        }
                    }
                }
            }

            if let Some((entity_id, _)) = closest_entity {
                if let Some(entity) = self.entity_manager.get_by_id_mut(entity_id) {
                    let held_stack = self.inventory.hotbar[self.inventory.selected].clone();
                    let held_item = held_stack
                        .map(|s| s.item)
                        .unwrap_or(crate::inventory::Item::Air);

                    match entity.entity_type {
                        crate::entity::EntityType::Pig => {
                            if held_item == crate::inventory::Item::Carrot
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Pig entered love mode!");
                                return;
                            }
                        }
                        crate::entity::EntityType::Cow => {
                            if held_item == crate::inventory::Item::Wheat
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Cow entered love mode!");
                                return;
                            }
                            if held_item == crate::inventory::Item::Bucket {
                                self.inventory
                                    .replace_selected_item(crate::inventory::Item::MilkBucket);
                                println!("[Debug] Milked a Cow!");
                                return;
                            }
                        }
                        crate::entity::EntityType::Sheep => {
                            if held_item == crate::inventory::Item::Wheat
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Sheep entered love mode!");
                                return;
                            }
                            if held_item == crate::inventory::Item::Shears && entity.has_wool {
                                let wool_position = entity.position;
                                entity.has_wool = false;
                                self.store_or_drop_generated_item(
                                    crate::inventory::Item::Wool,
                                    wool_position,
                                );
                                println!("[Debug] Sheared a Sheep!");
                                if let Some(stack) =
                                    &mut self.inventory.hotbar[self.inventory.selected]
                                {
                                    if stack.durability > 1 {
                                        stack.durability -= 1;
                                    } else {
                                        self.inventory.hotbar[self.inventory.selected] = None;
                                    }
                                }
                                return;
                            }
                        }
                        crate::entity::EntityType::Chicken => {
                            if held_item == crate::inventory::Item::Seeds
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Chicken entered love mode!");
                                return;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let target_policy = if is_left_click {
            RaycastTargetPolicy::Break
        } else {
            RaycastTargetPolicy::Place
        };
        if let Some(hit) = raycast(
            self.camera.position,
            dir,
            5.0,
            &self.chunk_manager,
            target_policy,
        ) {
            let target = if is_left_click {
                hit.block_pos
            } else {
                let clicked_block = self.chunk_manager.get_block(
                    hit.block_pos.x as i32,
                    hit.block_pos.y as i32,
                    hit.block_pos.z as i32,
                );
                let held = self.inventory.hotbar[self.inventory.selected];
                let clicked_pos = (
                    hit.block_pos.x as i32,
                    hit.block_pos.y as i32,
                    hit.block_pos.z as i32,
                );
                let held_item = held.map(|stack| stack.item).unwrap_or(Item::Air);
                // Hoe tilling
                if matches!(clicked_block, BlockType::Grass | BlockType::Dirt)
                    && held_item.tool_properties().map(|t| t.tool_type)
                        == Some(crate::inventory::ToolType::Hoe)
                {
                    let block_above = self.chunk_manager.get_block(
                        clicked_pos.0,
                        clicked_pos.1 + 1,
                        clicked_pos.2,
                    );
                    if !block_above.properties().is_solid {
                        self.apply_block_changes(&[(clicked_pos, BlockType::Farmland)]);
                        self.chunk_manager.set_block_state(
                            clicked_pos.0,
                            clicked_pos.1,
                            clicked_pos.2,
                            0,
                        );
                        if let Some(stack) = &mut self.inventory.hotbar[self.inventory.selected] {
                            if stack.durability > 1 {
                                stack.durability -= 1;
                            } else {
                                self.inventory.hotbar[self.inventory.selected] = None;
                            }
                        }
                        return;
                    }
                }

                // Bone Meal usage on crops
                if held_item == Item::BoneMeal
                    && matches!(
                        clicked_block,
                        BlockType::WheatCrop | BlockType::CarrotCrop | BlockType::PotatoCrop
                    )
                {
                    let cur_state = self.chunk_manager.get_block_state(
                        clicked_pos.0,
                        clicked_pos.1,
                        clicked_pos.2,
                    );
                    let age = cur_state & 0b111;
                    if age < 7 {
                        let new_age = (age + 3).min(7);
                        self.chunk_manager.set_block_state(
                            clicked_pos.0,
                            clicked_pos.1,
                            clicked_pos.2,
                            new_age,
                        );
                        self.broadcast_block_change(
                            clicked_pos.0,
                            clicked_pos.1,
                            clicked_pos.2,
                            clicked_block,
                        );
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                        return;
                    }
                }

                // Planting seeds / crops on Farmland
                if clicked_block == BlockType::Farmland
                    && matches!(held_item, Item::Seeds | Item::Carrot | Item::Potato)
                {
                    let plant_pos = (clicked_pos.0, clicked_pos.1 + 1, clicked_pos.2);
                    if self
                        .chunk_manager
                        .get_block(plant_pos.0, plant_pos.1, plant_pos.2)
                        == BlockType::Air
                    {
                        let crop_block = match held_item {
                            Item::Seeds => BlockType::WheatCrop,
                            Item::Carrot => BlockType::CarrotCrop,
                            _ => BlockType::PotatoCrop,
                        };
                        self.apply_block_changes(&[(plant_pos, crop_block)]);
                        self.chunk_manager.set_block_state(
                            plant_pos.0,
                            plant_pos.1,
                            plant_pos.2,
                            0,
                        );
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                        return;
                    }
                }
                if clicked_block == BlockType::Obsidian && held_item == Item::FlintAndSteel {
                    if let Some(interior) =
                        crate::dimension::detect_nether_frame(clicked_pos, |x, y, z| {
                            self.chunk_manager.get_block(x, y, z)
                        })
                    {
                        let changes: Vec<_> = interior
                            .into_iter()
                            .map(|position| (position, BlockType::NetherPortal))
                            .collect();
                        self.apply_block_changes(&changes);
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                        return;
                    }
                }
                if clicked_block == BlockType::EndPortalFrame && held_item == Item::EyeOfEnder {
                    self.apply_block_changes(&[(clicked_pos, BlockType::EndPortalFrameFilled)]);
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    if let Some(interior) =
                        crate::dimension::detect_completed_end_portal(clicked_pos, |x, y, z| {
                            self.chunk_manager.get_block(x, y, z)
                        })
                    {
                        let changes: Vec<_> = interior
                            .into_iter()
                            .map(|position| (position, BlockType::EndPortal))
                            .collect();
                        self.apply_block_changes(&changes);
                    }
                    return;
                }
                if matches!(clicked_block, BlockType::Obsidian | BlockType::Bedrock)
                    && held_item == Item::EndCrystal
                {
                    self.entity_manager.spawn(
                        crate::entity::EntityType::EndCrystal,
                        Vec3::new(
                            clicked_pos.0 as f32 + 0.5,
                            clicked_pos.1 as f32 + 1.0,
                            clicked_pos.2 as f32 + 0.5,
                        ),
                    );
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    return;
                }
                if clicked_block == BlockType::RespawnAnchor {
                    if held_item == Item::Glowstone || held_item == Item::GlowstoneDust {
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                        println!("[Respawn Anchor] Charged with Glowstone!");
                        return;
                    }
                    if self.current_dimension == crate::dimension::Dimension::Nether {
                        self.player_state.spawn_point =
                            Some([clicked_pos.0, clicked_pos.1 + 1, clicked_pos.2]);
                        self.player_state.spawn_dimension =
                            Some(crate::dimension::Dimension::Nether);
                        println!(
                            "[Respawn Anchor] Nether spawn point set to ({}, {}, {})",
                            clicked_pos.0,
                            clicked_pos.1 + 1,
                            clicked_pos.2
                        );
                    } else {
                        self.apply_block_changes(&[(clicked_pos, BlockType::Air)]);
                        self.take_damage(50.0, DamageSource::Mob);
                        println!("[Respawn Anchor] Exploded in non-Nether dimension!");
                    }
                    return;
                }
                if held_item == Item::EyeOfEnder && clicked_block != BlockType::EndPortalFrame {
                    if let Some((sx, sy, sz)) = crate::structure::locate_structure(
                        crate::structure::StructureId::Stronghold,
                        (
                            self.camera.position.x as i32,
                            self.camera.position.y as i32,
                            self.camera.position.z as i32,
                        ),
                        self.world_seed,
                        self.current_dimension,
                    ) {
                        println!(
                            "[Eye of Ender] Stronghold located at ({}, {}, {})",
                            sx, sy, sz
                        );
                    }
                    return;
                }
                if clicked_block == BlockType::Bed {
                    if matches!(&self.network, NetworkHandle::Client { .. }) {
                        let _ = self.submit_local_authority_operation(
                            crate::network::protocol::GameplayOperation::Sleep {
                                x: clicked_pos.0,
                                y: clicked_pos.1,
                                z: clicked_pos.2,
                            },
                        );
                        return;
                    }
                    let bed_pos = clicked_pos;
                    if self.current_dimension != crate::dimension::Dimension::Overworld {
                        self.apply_block_changes(&[(bed_pos, BlockType::Air)]);
                        self.take_damage(50.0, DamageSource::Mob);
                        println!("[Debug] Bed exploded in non-Overworld dimension!");
                    } else {
                        let bstate = crate::world::BlockState::decode(
                            self.chunk_manager
                                .get_block_state(bed_pos.0, bed_pos.1, bed_pos.2),
                        );
                        let head_pos = if bstate.is_top {
                            bed_pos
                        } else {
                            (
                                bed_pos.0 + bstate.facing.dx(),
                                bed_pos.1,
                                bed_pos.2 + bstate.facing.dz(),
                            )
                        };
                        self.player_state.spawn_point = Some([head_pos.0, head_pos.1, head_pos.2]);
                        self.player_state.spawn_dimension =
                            Some(crate::dimension::Dimension::Overworld);

                        let time_of_day = self.world_time.ticks % 24000;
                        let is_night = time_of_day >= 12541 && time_of_day <= 23458;
                        let is_storm = self.weather.is_thundering();

                        if !is_night && !is_storm {
                            println!(
                                "[Game] Respawn point set. You can only sleep at night or during thunderstorms."
                            );
                        } else {
                            let pos_vec = Vec3::new(
                                bed_pos.0 as f32 + 0.5,
                                bed_pos.1 as f32 + 0.5,
                                bed_pos.2 as f32 + 0.5,
                            );
                            let nearby_hostiles = self
                                .entity_manager
                                .query_radius(pos_vec, 8.0)
                                .any(|e| e.entity_type.is_hostile() && e.health > 0.0);
                            if nearby_hostiles {
                                println!("[Game] You may not rest now, there are monsters nearby.");
                            } else {
                                self.player_state.is_sleeping = true;
                                self.player_state.sleep_timer = 0.0;
                                self.player_state.bed_pos = Some([bed_pos.0, bed_pos.1, bed_pos.2]);
                                println!("[Game] Sleeping...");
                            }
                        }
                    }
                    return;
                }
                if clicked_block == BlockType::Water
                    && held.is_some_and(|stack| stack.item == Item::GlassBottle)
                {
                    let selected = self.inventory.selected;
                    let original_selected = self.inventory.hotbar[selected];
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    let mut water_bottle = ItemStack::new(Item::Potion, 1);
                    water_bottle.potion = Some(crate::brewing::PotionData::water());
                    if self.inventory.add_stack(water_bottle).is_some() {
                        self.inventory.hotbar[selected] = original_selected;
                    }
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::Chest
                        | BlockType::EndCityChest
                        | BlockType::Furnace
                        | BlockType::FurnaceLit
                        | BlockType::Hopper
                        | BlockType::Dispenser
                        | BlockType::Dropper
                ) {
                    let pos = (
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                    );
                    self.open_chest(pos);
                    return;
                }
                if clicked_block == BlockType::CraftingTable {
                    self.inventory.is_table_open = true;
                    self.inventory.craft_input = vec![None; 9];
                    self.open_inventory();
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::EnchantingTable | BlockType::BrewingStand | BlockType::Anvil
                ) {
                    let kind = match clicked_block {
                        BlockType::EnchantingTable => StationKind::Enchanting,
                        BlockType::BrewingStand => StationKind::Brewing,
                        _ => StationKind::Anvil,
                    };
                    self.open_station(kind, hit.block_pos);
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::OakDoor
                        | BlockType::OakDoorOpen
                        | BlockType::OakTrapdoor
                        | BlockType::OakTrapdoorOpen
                ) {
                    let pos = (
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                    );
                    let (target_block, sound) = match clicked_block {
                        BlockType::OakDoor => {
                            (BlockType::OakDoorOpen, crate::audio::SoundId::UiClick)
                        }
                        BlockType::OakDoorOpen => {
                            (BlockType::OakDoor, crate::audio::SoundId::UiClick)
                        }
                        BlockType::OakTrapdoor => {
                            (BlockType::OakTrapdoorOpen, crate::audio::SoundId::UiClick)
                        }
                        BlockType::OakTrapdoorOpen => {
                            (BlockType::OakTrapdoor, crate::audio::SoundId::UiClick)
                        }
                        _ => unreachable!(),
                    };
                    let cur_raw = self.chunk_manager.get_block_state(pos.0, pos.1, pos.2);
                    let mut bstate = crate::world::BlockState::decode(cur_raw);
                    bstate.is_open = !bstate.is_open;
                    let new_state_raw = bstate.encode();

                    self.chunk_manager
                        .set_block(pos.0, pos.1, pos.2, target_block);
                    self.chunk_manager
                        .set_block_state(pos.0, pos.1, pos.2, new_state_raw);
                    self.broadcast_block_change(pos.0, pos.1, pos.2, target_block);

                    if matches!(clicked_block, BlockType::OakDoor | BlockType::OakDoorOpen) {
                        let other_y = if bstate.is_top { pos.1 - 1 } else { pos.1 + 1 };
                        let other_block = self.chunk_manager.get_block(pos.0, other_y, pos.2);
                        if matches!(other_block, BlockType::OakDoor | BlockType::OakDoorOpen) {
                            let other_raw =
                                self.chunk_manager.get_block_state(pos.0, other_y, pos.2);
                            let mut other_bstate = crate::world::BlockState::decode(other_raw);
                            other_bstate.is_open = bstate.is_open;
                            let other_target = if bstate.is_open {
                                BlockType::OakDoorOpen
                            } else {
                                BlockType::OakDoor
                            };
                            self.chunk_manager
                                .set_block(pos.0, other_y, pos.2, other_target);
                            self.chunk_manager.set_block_state(
                                pos.0,
                                other_y,
                                pos.2,
                                other_bstate.encode(),
                            );
                            self.broadcast_block_change(pos.0, other_y, pos.2, other_target);
                        }
                    }

                    let mut dirty_chunks = std::collections::HashSet::new();
                    mark_block_mesh_dependencies(&mut dirty_chunks, pos.0, pos.2);
                    self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Redstone);
                    self.audio_manager.play_sound(sound);
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::Lever
                        | BlockType::LeverOn
                        | BlockType::StoneButton
                        | BlockType::StoneButtonPressed
                        | BlockType::Repeater
                        | BlockType::RepeaterPowered
                        | BlockType::Comparator
                        | BlockType::ComparatorPowered
                        | BlockType::NoteBlock
                ) {
                    let pos = (
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                    );
                    let update = self.redstone.interact(&mut self.chunk_manager, pos);
                    self.apply_redstone_update(update);
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::UiClick);
                    return;
                }
                hit.block_pos + hit.normal
            };

            let wx = target.x as i32;
            let wy = target.y as i32;
            let wz = target.z as i32;

            let mut dirty_chunks = std::collections::HashSet::new();
            // Resulting block at (wx, wy, wz) after this click, used to fan the
            // authoritative mutation out to connected clients. `None` means the
            // click did not mutate the world (e.g. broke nothing).
            let mut result_block: Option<BlockType> = None;
            if is_left_click {
                let old_block = self.chunk_manager.get_block(wx, wy, wz);
                if old_block != BlockType::Air {
                    if !self.can_break_current_block(old_block) {
                        return;
                    }
                    let chest_partner = if old_block == BlockType::Chest {
                        let old_state = crate::world::BlockState::decode(
                            self.chunk_manager.get_block_state(wx, wy, wz),
                        );
                        if old_state.chest_type != crate::world::ChestType::Single {
                            self.double_chest_partner((wx, wy, wz), old_state.chest_type)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    // Inventory-bearing entities are authoritative state and
                    // must be drained before the block is removed.  The same
                    // path is used for every automation container.
                    if matches!(
                        old_block,
                        BlockType::Chest
                            | BlockType::EndCityChest
                            | BlockType::Furnace
                            | BlockType::FurnaceLit
                            | BlockType::Hopper
                            | BlockType::Dispenser
                            | BlockType::Dropper
                    ) {
                        self.drop_block_entity_inventory((wx, wy, wz));
                        self.close_legacy_container_sessions_at((wx, wy, wz));
                        if let Some(partner) = chest_partner {
                            self.close_legacy_container_sessions_at(partner);
                        }
                    }
                    if old_block == crate::world::BlockType::Chest {
                        // If part of a double chest, revert partner to single.
                        if let Some(partner) = chest_partner {
                            let partner_raw = self
                                .chunk_manager
                                .get_block_state(partner.0, partner.1, partner.2);
                            let mut partner_state = crate::world::BlockState::decode(partner_raw);
                            partner_state.chest_type = crate::world::ChestType::Single;
                            self.chunk_manager.set_block_state(
                                partner.0,
                                partner.1,
                                partner.2,
                                partner_state.encode(),
                            );
                        }
                    }
                    self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
                    self.network
                        .send_action(crate::network::protocol::Action::Break);
                    self.trigger_advancement(crate::advancements::AdvancementTrigger::MineBlock(
                        old_block,
                    ));
                    self.redstone.on_block_changed(
                        &self.chunk_manager,
                        (wx, wy, wz),
                        crate::redstone::Direction::North,
                    );

                    let sound_pos =
                        glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                    let listener_right =
                        glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                            .normalize_or_zero();
                    if let Some(mat) = old_block.sound_material() {
                        self.audio_manager.play_sound_3d(
                            crate::audio::SoundId::BlockBreak(mat),
                            sound_pos,
                            self.camera.position,
                            listener_right,
                        );
                    }

                    if self.game_mode == GameMode::Survival {
                        self.store_or_drop_generated_item(
                            crate::inventory::Item::from_block(old_block),
                            sound_pos,
                        );

                        if old_block == BlockType::Grass {
                            let rng = (wx as u32).wrapping_mul(31).wrapping_add(wz as u32);
                            if rng % 20 == 0 {
                                let drop = match rng % 3 {
                                    0 => crate::inventory::Item::Seeds,
                                    1 => crate::inventory::Item::Wheat,
                                    _ => crate::inventory::Item::Carrot,
                                };
                                self.store_or_drop_generated_item(drop, sound_pos);
                            }
                        }
                        if old_block == BlockType::Bed {
                            let bstate = crate::world::BlockState::decode(
                                self.chunk_manager.get_block_state(wx, wy, wz),
                            );
                            let (ox, oz) = if bstate.is_top {
                                (wx - bstate.facing.dx(), wz - bstate.facing.dz())
                            } else {
                                (wx + bstate.facing.dx(), wz + bstate.facing.dz())
                            };
                            if self.chunk_manager.get_block(ox, wy, oz) == BlockType::Bed {
                                self.chunk_manager.set_block(ox, wy, oz, BlockType::Air);
                                self.broadcast_block_change(ox, wy, oz, BlockType::Air);
                            }
                        }
                    }

                    // Update lighting for removal
                    crate::lighting::update_sky_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                    crate::lighting::update_block_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        old_block.properties().light_emission,
                        &mut dirty_chunks,
                    );
                    self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);
                    result_block = Some(BlockType::Air);
                }
            } else {
                if let Some(placed_block) = self.inventory.get_selected_block() {
                    if placed_block == BlockType::Bed {
                        let facing = crate::redstone::Direction::from_yaw(self.camera.yaw);
                        let hx = wx + facing.dx();
                        let hz = wz + facing.dz();
                        if !self.can_place_block_at(wx, wy, wz, BlockType::Bed)
                            || !self.can_place_block_at(hx, wy, hz, BlockType::Bed)
                        {
                            return;
                        }
                        if !self.chunk_manager.can_place_block_with_support(
                            BlockType::Bed,
                            wx,
                            wy,
                            wz,
                        ) || !self.chunk_manager.can_place_block_with_support(
                            BlockType::Bed,
                            hx,
                            wy,
                            hz,
                        ) {
                            return;
                        }
                        let foot_state = crate::world::BlockState {
                            facing,
                            is_top: false,
                            is_right_hinge: false,
                            is_open: false,
                            chest_type: crate::world::ChestType::Single,
                        };
                        let head_state = crate::world::BlockState {
                            facing,
                            is_top: true,
                            is_right_hinge: false,
                            is_open: false,
                            chest_type: crate::world::ChestType::Single,
                        };
                        self.chunk_manager.set_block(wx, wy, wz, BlockType::Bed);
                        self.chunk_manager
                            .set_block_state(wx, wy, wz, foot_state.encode());
                        self.chunk_manager.set_block(hx, wy, hz, BlockType::Bed);
                        self.chunk_manager
                            .set_block_state(hx, wy, hz, head_state.encode());
                        self.broadcast_block_change(wx, wy, wz, BlockType::Bed);
                        self.broadcast_block_change(hx, wy, hz, BlockType::Bed);

                        let sound_pos =
                            Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                        let listener_right =
                            Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                                .normalize_or_zero();
                        if let Some(mat) = BlockType::Bed.sound_material() {
                            self.audio_manager.play_sound_3d(
                                crate::audio::SoundId::BlockPlace(mat),
                                sound_pos,
                                self.camera.position,
                                listener_right,
                            );
                        }
                        let is_creative = self.game_mode == GameMode::Creative;
                        self.inventory.use_selected_item(is_creative);
                        return;
                    }
                    if placed_block == BlockType::OakDoor {
                        if wy + 1 >= crate::world::CHUNK_HEIGHT as i32 {
                            return;
                        }
                        if !self.can_place_block_at(wx, wy, wz, BlockType::OakDoor)
                            || !self.can_place_block_at(wx, wy + 1, wz, BlockType::OakDoor)
                        {
                            return;
                        }
                        if !self.chunk_manager.can_place_block_with_support(
                            BlockType::OakDoor,
                            wx,
                            wy,
                            wz,
                        ) {
                            return;
                        }
                        let (bottom_state, top_state) =
                            crate::world::BlockState::for_door_placement(
                                &self.chunk_manager,
                                wx,
                                wy,
                                wz,
                                self.camera.yaw,
                            );

                        self.chunk_manager.set_block(wx, wy, wz, BlockType::OakDoor);
                        self.chunk_manager
                            .set_block_state(wx, wy, wz, bottom_state.encode());
                        self.chunk_manager
                            .set_block(wx, wy + 1, wz, BlockType::OakDoor);
                        self.chunk_manager
                            .set_block_state(wx, wy + 1, wz, top_state.encode());

                        self.network
                            .send_action(crate::network::protocol::Action::Place);
                        self.redstone.on_block_changed(
                            &self.chunk_manager,
                            (wx, wy, wz),
                            bottom_state.facing,
                        );

                        let sound_pos =
                            glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                        let listener_right =
                            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                                .normalize_or_zero();
                        if let Some(mat) = BlockType::OakDoor.sound_material() {
                            self.audio_manager.play_sound_3d(
                                crate::audio::SoundId::BlockPlace(mat),
                                sound_pos,
                                self.camera.position,
                                listener_right,
                            );
                        }

                        let is_creative = self.game_mode == GameMode::Creative;
                        self.inventory.use_selected_item(is_creative);

                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            &mut dirty_chunks,
                        );
                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy + 1,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz + 1);

                        self.broadcast_block_change(wx, wy, wz, BlockType::OakDoor);
                        self.broadcast_block_change(wx, wy + 1, wz, BlockType::OakDoor);
                        result_block = Some(BlockType::OakDoor);
                    } else if placed_block == BlockType::OakTrapdoor {
                        if !self.chunk_manager.can_place_block_with_support(
                            placed_block,
                            wx,
                            wy,
                            wz,
                        ) || !self.can_place_block_at(wx, wy, wz, placed_block)
                        {
                            return;
                        }
                        let state =
                            crate::world::BlockState::for_trapdoor_placement(self.camera.yaw);

                        self.chunk_manager
                            .set_block(wx, wy, wz, BlockType::OakTrapdoor);
                        self.chunk_manager
                            .set_block_state(wx, wy, wz, state.encode());

                        self.network
                            .send_action(crate::network::protocol::Action::Place);
                        self.redstone.on_block_changed(
                            &self.chunk_manager,
                            (wx, wy, wz),
                            state.facing,
                        );

                        let sound_pos =
                            glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                        let listener_right =
                            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                                .normalize_or_zero();
                        if let Some(mat) = BlockType::OakTrapdoor.sound_material() {
                            self.audio_manager.play_sound_3d(
                                crate::audio::SoundId::BlockPlace(mat),
                                sound_pos,
                                self.camera.position,
                                listener_right,
                            );
                        }

                        let is_creative = self.game_mode == GameMode::Creative;
                        self.inventory.use_selected_item(is_creative);

                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);

                        self.broadcast_block_change(wx, wy, wz, BlockType::OakTrapdoor);
                        result_block = Some(BlockType::OakTrapdoor);
                    } else {
                        if !self.chunk_manager.can_place_block_with_support(
                            placed_block,
                            wx,
                            wy,
                            wz,
                        ) {
                            return;
                        }
                        if !self.can_place_block_at(wx, wy, wz, placed_block) {
                            return;
                        }

                        let placement_facing =
                            crate::redstone::Direction::from_yaw(self.camera.yaw);
                        self.chunk_manager.set_block(wx, wy, wz, placed_block);
                        if matches!(
                            placed_block,
                            BlockType::Hopper
                                | BlockType::Observer
                                | BlockType::Dispenser
                                | BlockType::Dropper
                        ) {
                            let mut state = crate::world::BlockState::decode(
                                self.chunk_manager.get_block_state(wx, wy, wz),
                            );
                            state.facing = placement_facing;
                            self.chunk_manager
                                .set_block_state(wx, wy, wz, state.encode());
                        }
                        if let Some(mut block_entity) =
                            crate::block_entity::default_stub_for_block(placed_block)
                        {
                            match &mut block_entity {
                                crate::block_entity::BlockEntity::Hopper(hopper) => {
                                    hopper.facing = placement_facing;
                                }
                                crate::block_entity::BlockEntity::Observer(observer) => {
                                    observer.facing = placement_facing;
                                }
                                _ => {}
                            }
                            self.chunk_manager.set_block_entity(
                                wx,
                                wy,
                                wz,
                                Some(block_entity.clone()),
                            );
                            self.broadcast_block_entity_delta(wx, wy, wz, Some(block_entity));
                        }
                        self.network
                            .send_action(crate::network::protocol::Action::Place);
                        self.redstone.on_block_changed(
                            &self.chunk_manager,
                            (wx, wy, wz),
                            placement_facing,
                        );

                        let sound_pos =
                            glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                        let listener_right =
                            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                                .normalize_or_zero();
                        if let Some(mat) = placed_block.sound_material() {
                            self.audio_manager.play_sound_3d(
                                crate::audio::SoundId::BlockPlace(mat),
                                sound_pos,
                                self.camera.position,
                                listener_right,
                            );
                        }

                        let is_creative = self.game_mode == GameMode::Creative;
                        self.inventory.use_selected_item(is_creative);

                        // Update lighting for placement
                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            &mut dirty_chunks,
                        );
                        crate::lighting::update_block_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            placed_block.properties().light_emission,
                            &mut dirty_chunks,
                        );

                        self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);
                        result_block = Some(placed_block);
                    }

                    if matches!(
                        placed_block,
                        BlockType::SoulSand | BlockType::WitherSkeletonSkull
                    ) {
                        if let Some(pattern) =
                            crate::boss::detect_wither_pattern((wx, wy, wz), |position| {
                                self.chunk_manager
                                    .get_block(position.0, position.1, position.2)
                            })
                        {
                            let spawn_pos = pattern.iter().fold(Vec3::ZERO, |sum, &(x, y, z)| {
                                sum + Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)
                            }) / pattern.len() as f32;
                            let removals: Vec<_> = pattern
                                .into_iter()
                                .map(|position| (position, BlockType::Air))
                                .collect();
                            self.apply_block_changes(&removals);
                            // The wither ritual consumes the placed block too;
                            // broadcast that final state before spawning.
                            self.broadcast_block_change(wx, wy, wz, BlockType::Air);
                            self.entity_manager
                                .spawn(crate::entity::EntityType::Wither, spawn_pos);
                            return;
                        }
                    }
                } else {
                    return; // No block selected to place
                }
            }

            mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);

            self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

            // Fan the authoritative player-driven mutation out to clients.
            if let Some(block) = result_block {
                self.broadcast_block_change(wx, wy, wz, block);
            }
        }
    }

    /// Leftover potions / bow / food / milk. Live Embedded / Join never call this.
    fn legacy_try_use_held_item(&mut self) -> bool {
        let held_stack = self.inventory.hotbar[self.inventory.selected];
        let held_item = held_stack
            .map(|s| s.item)
            .unwrap_or(crate::inventory::Item::Air);
        if let Some(potion) = held_stack.and_then(|stack| stack.potion) {
            if potion.splash || held_item == Item::SplashPotion {
                let dir = self.look_direction();
                let id = self.entity_manager.spawn(
                    crate::entity::EntityType::SplashPotion,
                    self.camera.position + dir * 0.5,
                );
                if let Some(projectile) = self.entity_manager.get_by_id_mut(id) {
                    projectile.velocity = dir * 12.0;
                    projectile.potion = Some(potion);
                    projectile.life_time = 3.0;
                }
            } else {
                let healing = self.potion_effects.apply(potion);
                self.player_state.health =
                    (self.player_state.health + healing).min(self.player_state.max_health);
            }
            self.inventory
                .use_selected_item(self.game_mode == GameMode::Creative);
            return true;
        }
        if held_item == Item::MilkBucket {
            self.potion_effects.active.clear();
            if self.game_mode_policy().hunger_enabled {
                self.inventory.replace_selected_item(Item::Bucket);
            }
            return true;
        }
        if held_item == Item::Bow {
            let enchantments = held_stack
                .map(|stack| stack.enchantments)
                .unwrap_or_default();
            let infinity = enchantments.level_of(crate::enchantment::Enchantment::Infinity) > 0;
            if self.game_mode == GameMode::Creative
                || infinity
                || self.inventory.remove_one(Item::Arrow)
            {
                let dir = self.look_direction();
                let id = self.entity_manager.spawn(
                    crate::entity::EntityType::Arrow,
                    self.camera.position + dir * 0.6,
                );
                if let Some(arrow) = self.entity_manager.get_by_id_mut(id) {
                    arrow.velocity = dir * 22.0;
                    arrow.friendly_projectile = true;
                    arrow.projectile_damage = 4.0
                        + enchantments.level_of(crate::enchantment::Enchantment::Power(1)) as f32
                            * 1.25;
                }
            }
            return true;
        }
        if let Some(food_props) = held_item.food_properties() {
            if self.player_state.hunger < 20.0
                || food_props.always_edible
                || self.game_mode == GameMode::Creative
            {
                if let Some(ref mut eating) = self.player_state.eating_state {
                    if eating.item == held_item && eating.slot == self.inventory.selected {
                        eating.ticks_remaining = eating.ticks_remaining.saturating_sub(1);
                        if eating.ticks_remaining == 0 {
                            self.player_state.hunger =
                                (self.player_state.hunger + food_props.hunger).min(20.0);
                            self.player_state.saturation = (self.player_state.saturation
                                + food_props.saturation)
                                .min(self.player_state.hunger);
                            let is_creative = self.game_mode == GameMode::Creative;
                            self.inventory.use_selected_item(is_creative);
                            if let Some(ret) = food_props.return_item {
                                let _ = self
                                    .inventory
                                    .add_stack(crate::inventory::ItemStack::new(ret, 1));
                            }
                            self.trigger_advancement(
                                crate::advancements::AdvancementTrigger::EatFood(held_item),
                            );
                            self.player_state.eating_state = None;
                        }
                        return true;
                    }
                } else {
                    self.player_state.eating_state = Some(crate::player::ActiveEatingState {
                        item: held_item,
                        slot: self.inventory.selected,
                        ticks_remaining: food_props.use_duration_ticks,
                        total_duration: food_props.use_duration_ticks,
                    });
                    return true;
                }
            }
        }
        false
    }

    fn open_station(&mut self, kind: StationKind, position: Vec3) {
        if !self.game_mode_policy().can_use_containers {
            return;
        }
        self.active_station = Some(kind);
        if kind == StationKind::Enchanting {
            let wx = position.x as i32;
            let wy = position.y as i32;
            let wz = position.z as i32;
            let mut shelves = 0;
            for dx in -2i32..=2i32 {
                for dz in -2i32..=2i32 {
                    if dx.abs() != 2 && dz.abs() != 2 {
                        continue;
                    }
                    for dy in 0..=1 {
                        if self.chunk_manager.get_block(wx + dx, wy + dy, wz + dz)
                            == BlockType::Bookshelf
                        {
                            shelves += 1;
                        }
                    }
                }
            }
            self.enchanting.bookshelves = shelves.min(15);
            self.enchanting.seed =
                self.world_time.ticks as u32 ^ wx as u32 ^ (wz as u32).rotate_left(16);
            self.enchanting.refresh();
        }
        self.open_inventory();
    }
}

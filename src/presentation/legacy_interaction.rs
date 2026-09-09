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

    #[allow(dead_code)]
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

    pub(super) fn try_melee_attack(&mut self) -> bool {
        if !self.game_mode_policy().can_target_mobs {
            return false;
        }

        // Raycast forward from camera
        let origin = self.camera.position;
        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();

        let held = self.inventory.hotbar[self.inventory.selected];
        let held_item = held.map(|s| s.item).unwrap_or(Item::Air);
        let mut base_damage = 1.0f32;
        let mut knockback = 0.4f32;
        let mut fire_aspect_level = 0u8;

        if let Some(tool) = held_item.tool_properties() {
            base_damage = tool.damage;
        }

        // Calculate Attack Cooldown Multiplier
        let max_ticks = self.player_state.attack_cooldown_max_ticks;
        let current_ticks = self.player_state.attack_cooldown_ticks;
        let cooldown_progress = (current_ticks as f32 / max_ticks as f32).clamp(0.0, 1.0);
        let cooldown_factor = 0.2 + 0.8 * cooldown_progress * cooldown_progress;

        // Apply Enchantments
        if let Some(stack) = held {
            base_damage +=
                crate::enchantment::attack_damage_bonus(&stack.enchantments) * cooldown_factor;
            knockback += stack
                .enchantments
                .level_of(crate::enchantment::Enchantment::Knockback(1))
                as f32
                * 0.5
                * cooldown_factor;
            fire_aspect_level = stack
                .enchantments
                .level_of(crate::enchantment::Enchantment::FireAspect(1));
        }

        let damage = (base_damage * cooldown_factor).max(1.0);

        // Reset Attack Cooldown
        self.player_state.attack_cooldown_ticks = 0;
        self.player_state.attack_cooldown_max_ticks = held_item.attack_cooldown_ticks();

        let nearest = closest_melee_target(&self.entity_manager, origin, dir, 4.0);

        if let Some(id) = nearest {
            let Some(entity) = self.entity_manager.get_by_id_mut(id) else {
                return false;
            };

            let impact = apply_melee_impact(entity, dir, damage, knockback, fire_aspect_level);
            if impact == MeleeImpact::Invulnerable {
                return true;
            }

            // Damage weapon durability
            if self.game_mode != GameMode::Creative {
                if let Some(ref mut stack) = self.inventory.hotbar[self.inventory.selected] {
                    if stack.durability > 0 {
                        stack.durability = stack.durability.saturating_sub(1);
                        if stack.durability == 0 {
                            self.inventory.hotbar[self.inventory.selected] = None;
                        }
                    }
                }
            }

            // Mob drops
            if let Some(kill) = claim_standard_player_kill(entity) {
                let looting = held
                    .map(|s| {
                        s.enchantments
                            .level_of(crate::enchantment::Enchantment::Looting(1))
                    })
                    .unwrap_or(0);
                self.settle_standard_player_kill(kill, looting);
            }

            return true;
        }

        false
    }

    pub(super) fn legacy_handle_secondary_release(&mut self) {
        if let Some(using) = self.player_state.using_item.take() {
            if using.action == crate::player::ItemUseAction::Bow {
                let charge = (using.ticks_held as f32 / 20.0).min(1.0);
                if charge > 0.1 {
                    let has_infinity = self
                        .inventory
                        .hotbar
                        .get(self.inventory.selected)
                        .copied()
                        .flatten()
                        .map(|stack| {
                            stack
                                .enchantments
                                .level_of(crate::enchantment::Enchantment::Infinity)
                                > 0
                        })
                        .unwrap_or(false);

                    let power_level = self
                        .inventory
                        .hotbar
                        .get(self.inventory.selected)
                        .copied()
                        .flatten()
                        .map(|stack| {
                            stack
                                .enchantments
                                .level_of(crate::enchantment::Enchantment::Power(1))
                        })
                        .unwrap_or(0);

                    let (arrow_slot, has_arrow) = self
                        .inventory
                        .find_item(Item::Arrow)
                        .map(|(slot, _)| (Some(slot), true))
                        .unwrap_or((None, false));
                    if self.game_mode == GameMode::Creative || has_arrow {
                        if self.game_mode != GameMode::Creative && !has_infinity {
                            if let Some(slot) = arrow_slot {
                                self.inventory.remove_at_slot(slot);
                            }
                        }

                        let eye_pos = self.player_physics.position + Vec3::new(0.0, 1.62, 0.0);
                        let dir = Vec3::new(
                            self.camera.yaw.cos() * self.camera.pitch.cos(),
                            self.camera.pitch.sin(),
                            self.camera.yaw.sin() * self.camera.pitch.cos(),
                        )
                        .normalize_or_zero();

                        let arrow_id = self
                            .entity_manager
                            .spawn(crate::entity::EntityType::Arrow, eye_pos + dir * 0.5);

                        let speed = charge * 30.0;
                        let damage =
                            (2.0 + charge * 8.0) * (1.0 + 0.25 * (power_level as f32 + 1.0));

                        if let Some(arrow) = self.entity_manager.get_by_id_mut(arrow_id) {
                            arrow.velocity = dir * speed;
                            arrow.friendly_projectile = true;
                            arrow.projectile_damage = damage;
                        }

                        self.audio_manager
                            .play_sound(crate::audio::SoundId::ArrowShoot);

                        if self.game_mode != GameMode::Creative {
                            if let Some(ref mut stack) =
                                self.inventory.hotbar[self.inventory.selected]
                            {
                                if stack.durability > 0 {
                                    stack.durability = stack.durability.saturating_sub(1);
                                    if stack.durability == 0 {
                                        self.inventory.hotbar[self.inventory.selected] = None;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn apply_block_changes(&mut self, changes: &[((i32, i32, i32), BlockType)]) {
        let lighting_started = Instant::now();
        let mut dirty_chunks = std::collections::HashSet::new();
        let mut broken_unsupported = Vec::new();
        for &((x, y, z), block) in changes {
            let old = self.chunk_manager.get_block(x, y, z);
            if old == block {
                continue;
            }
            let is_chest = matches!(
                old,
                crate::world::BlockType::Chest | crate::world::BlockType::EndCityChest
            );
            if is_chest {
                let partner = self
                    .double_chest_partner((x, y, z), crate::world::ChestType::Left)
                    .or_else(|| {
                        self.double_chest_partner((x, y, z), crate::world::ChestType::Right)
                    });
                self.drop_chest_inventory((x, y, z));
                self.close_legacy_container_sessions_at((x, y, z));
                if let Some(partner) = partner {
                    self.close_legacy_container_sessions_at(partner);
                }
            }
            if matches!(
                old,
                crate::world::BlockType::Furnace
                    | crate::world::BlockType::Hopper
                    | crate::world::BlockType::Dispenser
                    | crate::world::BlockType::Dropper
            ) {
                self.drop_block_entity_inventory((x, y, z));
                self.close_legacy_container_sessions_at((x, y, z));
            }
            self.chunk_manager.set_block(x, y, z, block);
            self.redstone.on_block_changed(
                &self.chunk_manager,
                (x, y, z),
                crate::redstone::Direction::North,
            );

            let old_properties = old.properties();
            let new_properties = block.properties();
            if old_properties.is_opaque() != new_properties.is_opaque() {
                if new_properties.is_opaque() {
                    crate::lighting::update_sky_light_after_placed(
                        &mut self.chunk_manager,
                        x,
                        y,
                        z,
                        &mut dirty_chunks,
                    );
                } else {
                    crate::lighting::update_sky_light_after_removed(
                        &mut self.chunk_manager,
                        x,
                        y,
                        z,
                        &mut dirty_chunks,
                    );
                }
            }
            if old_properties.light_emission != new_properties.light_emission {
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    old_properties.light_emission,
                    &mut dirty_chunks,
                );
                if new_properties.light_emission > 0 {
                    crate::lighting::update_block_light_after_placed(
                        &mut self.chunk_manager,
                        x,
                        y,
                        z,
                        new_properties.light_emission,
                        &mut dirty_chunks,
                    );
                }
            }
            mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
            if block == BlockType::Air {
                if self
                    .presentation_topology()
                    .inventory_decision(PresentationInventoryTarget::UnsupportedBreak)
                    == PresentationInventoryAction::LocalMutate
                {
                    self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
                }
            } else if !self
                .chunk_manager
                .can_place_block_with_support(block, x, y, z)
            {
                if self
                    .presentation_topology()
                    .inventory_decision(PresentationInventoryTarget::UnsupportedBreak)
                    == PresentationInventoryAction::LocalMutate
                {
                    broken_unsupported.push(((x, y, z), block));
                }
            }
        }
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Block);
        for &((x, y, z), block) in changes {
            self.invalidate_block_mesh_dependencies(x, y, z, DependencyReason::Block);
            self.broadcast_block_change(x, y, z, block);
        }
        self.finish_unsupported_breaks(broken_unsupported);
        let lighting_elapsed = lighting_started.elapsed();
        self.lighting_time_frame += lighting_elapsed;
        self.lighting_scopes_frame.record(
            crate::perf::LightingSource::Block as usize,
            lighting_elapsed,
        );
    }

    pub(super) fn check_and_break_unsupported_above(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
    ) {
        let mut broken = Vec::new();
        self.chunk_manager.check_and_break_unsupported_above(
            wx,
            wy,
            wz,
            dirty_chunks,
            |position, block| {
                broken.push((position, block));
            },
        );
        for &(position, _block) in &broken {
            self.broadcast_block_change(position.0, position.1, position.2, BlockType::Air);
        }
        self.finish_unsupported_breaks(broken);
    }

    pub(super) fn check_and_break_unsupported_for_loaded_chunk(
        &mut self,
        cx: i32,
        cz: i32,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
    ) {
        if self
            .presentation_topology()
            .inventory_decision(PresentationInventoryTarget::UnsupportedBreak)
            != PresentationInventoryAction::LocalMutate
        {
            return;
        }
        let mut broken = Vec::new();
        self.chunk_manager
            .check_and_break_unsupported_for_loaded_chunk(
                cx,
                cz,
                dirty_chunks,
                |position, block| {
                    broken.push((position, block));
                },
            );
        for &(position, _block) in &broken {
            self.broadcast_block_change(position.0, position.1, position.2, BlockType::Air);
        }
        self.finish_unsupported_breaks(broken);
    }

    pub(super) fn finish_unsupported_breaks(
        &mut self,
        broken_blocks: Vec<((i32, i32, i32), BlockType)>,
    ) {
        if self
            .presentation_topology()
            .inventory_decision(PresentationInventoryTarget::UnsupportedBreak)
            != PresentationInventoryAction::LocalMutate
        {
            return;
        }
        for ((wx, wy, wz), block) in broken_blocks {
            let rewards =
                calculate_block_break_rewards(block, 0, (wx, wy, wz), None, self.game_mode);
            let sound_pos = glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
            for drop in &rewards.drops {
                self.spawn_dropped_item(drop.item, sound_pos);
            }
            if rewards.xp > 0 {
                self.spawn_xp_orb(rewards.xp, sound_pos);
            }
            if let Some(mat) = block.sound_material() {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::BlockBreak(mat));
            }
        }
    }

    #[allow(dead_code)]
    pub fn handle_client_block_action(
        &mut self,
        requester_id: crate::network::protocol::PlayerId,
        action: crate::network::protocol::Action,
        x: i32,
        y: i32,
        z: i32,
        block_wire: u32,
        held_item_wire: Option<crate::network::protocol::ItemWire>,
    ) {
        if !validate_remote_block_request(&self.remote_players, requester_id, (x, y, z)) {
            self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            return;
        }

        let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
            self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            return;
        };
        if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
            self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            return;
        }

        match action {
            crate::network::protocol::Action::Break => {
                let old_block = self.chunk_manager.get_block(x, y, z);
                let old_state_raw = self.chunk_manager.get_block_state(x, y, z);
                if old_block == BlockType::Air || old_block == BlockType::Bedrock {
                    self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
                    return;
                }

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
                    self.drop_block_entity_inventory((x, y, z));
                }
                // Double chest topology is independent of the inventory drain.
                if old_block == crate::world::BlockType::Chest {
                    let old_state_raw = self.chunk_manager.get_block_state(x, y, z);
                    let old_state = crate::world::BlockState::decode(old_state_raw);
                    if old_state.chest_type != crate::world::ChestType::Single {
                        if let Some(partner) =
                            self.double_chest_partner((x, y, z), old_state.chest_type)
                        {
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
                }

                let mut dirty_chunks = std::collections::HashSet::new();
                self.chunk_manager.set_block(x, y, z, BlockType::Air);
                self.redstone.on_block_changed(
                    &self.chunk_manager,
                    (x, y, z),
                    crate::redstone::Direction::North,
                );
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    &mut dirty_chunks,
                );
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    old_block.properties().light_emission,
                    &mut dirty_chunks,
                );
                mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
                self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
                self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

                self.broadcast_block_change(x, y, z, BlockType::Air);

                let held_stack = held_item_wire.and_then(|w| w.to_stack());
                let rewards = calculate_block_break_rewards(
                    old_block,
                    old_state_raw,
                    (x, y, z),
                    held_stack.as_ref(),
                    self.game_mode,
                );

                let sound_pos = glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                for drop in &rewards.drops {
                    self.spawn_dropped_item(drop.item, sound_pos);
                }

                let drops_wire = rewards
                    .drops
                    .iter()
                    .map(crate::network::protocol::ItemWire::from_stack)
                    .collect();
                self.send_block_action_result(requester_id, x, y, z, true, false, drops_wire);
            }
            crate::network::protocol::Action::Place => {
                let block = BlockType::from_u8(block_wire as u8);
                if block == BlockType::Air {
                    self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
                    return;
                }

                if !self.can_place_block_at(x, y, z, block)
                    || !self
                        .chunk_manager
                        .can_place_block_with_support(block, x, y, z)
                {
                    self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
                    return;
                }

                let mut dirty_chunks = std::collections::HashSet::new();
                self.chunk_manager.set_block(x, y, z, block);
                let facing = crate::redstone::Direction::North;
                if matches!(
                    block,
                    BlockType::Hopper
                        | BlockType::Observer
                        | BlockType::Dispenser
                        | BlockType::Dropper
                ) {
                    let mut state = crate::world::BlockState::decode(
                        self.chunk_manager.get_block_state(x, y, z),
                    );
                    state.facing = facing;
                    self.chunk_manager.set_block_state(x, y, z, state.encode());
                }
                if let Some(block_entity) = crate::block_entity::default_stub_for_block(block) {
                    self.chunk_manager
                        .set_block_entity(x, y, z, Some(block_entity.clone()));
                    self.broadcast_block_entity_delta(x, y, z, Some(block_entity));
                }
                self.redstone
                    .on_block_changed(&self.chunk_manager, (x, y, z), facing);
                let properties = block.properties();
                if properties.is_solid {
                    crate::lighting::update_sky_light_after_placed(
                        &mut self.chunk_manager,
                        x,
                        y,
                        z,
                        &mut dirty_chunks,
                    );
                }
                if properties.light_emission > 0 {
                    crate::lighting::update_block_light_after_placed(
                        &mut self.chunk_manager,
                        x,
                        y,
                        z,
                        properties.light_emission,
                        &mut dirty_chunks,
                    );
                }
                mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
                self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

                self.broadcast_block_change(x, y, z, block);

                let consumed = self.game_mode == GameMode::Survival;
                self.send_block_action_result(requester_id, x, y, z, true, consumed, vec![]);
            }
            _ => {
                self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            }
        }
    }

    pub(super) fn double_chest_partner(
        &self,
        pos: (i32, i32, i32),
        chest_type: crate::world::ChestType,
    ) -> Option<(i32, i32, i32)> {
        let state_raw = self.chunk_manager.get_block_state(pos.0, pos.1, pos.2);
        let state = crate::world::BlockState::decode(state_raw);
        let (dx, dz) = match (state.facing, chest_type) {
            (crate::redstone::Direction::North, crate::world::ChestType::Left) => (-1, 0),
            (crate::redstone::Direction::North, crate::world::ChestType::Right) => (1, 0),
            (crate::redstone::Direction::East, crate::world::ChestType::Left) => (0, -1),
            (crate::redstone::Direction::East, crate::world::ChestType::Right) => (0, 1),
            (crate::redstone::Direction::South, crate::world::ChestType::Left) => (1, 0),
            (crate::redstone::Direction::South, crate::world::ChestType::Right) => (-1, 0),
            (crate::redstone::Direction::West, crate::world::ChestType::Left) => (0, 1),
            (crate::redstone::Direction::West, crate::world::ChestType::Right) => (0, -1),
            _ => return None,
        };
        let partner = (pos.0 + dx, pos.1, pos.2 + dz);
        let partner_block = self
            .chunk_manager
            .get_block(partner.0, partner.1, partner.2);
        if partner_block == crate::world::BlockType::Chest {
            Some(partner)
        } else {
            None
        }
    }

    pub(super) fn drop_chest_inventory(&mut self, pos: (i32, i32, i32)) -> bool {
        let (cx, cz) = (
            pos.0.div_euclid(crate::world::CHUNK_WIDTH as i32),
            pos.2.div_euclid(crate::world::CHUNK_DEPTH as i32),
        );
        let (bx, by, bz) = (
            pos.0.rem_euclid(crate::world::CHUNK_WIDTH as i32) as u8,
            pos.1 as i16,
            pos.2.rem_euclid(crate::world::CHUNK_DEPTH as i32) as u8,
        );
        let items: Vec<crate::inventory::Item> = self
            .chunk_manager
            .chunks
            .get(&(cx, cz))
            .and_then(|chunk| chunk.get_block_entity(bx, by, bz))
            .and_then(|entry| {
                if let crate::block_entity::BlockEntity::Chest(chest_be) = entry {
                    Some(
                        chest_be
                            .inventory
                            .slots
                            .iter()
                            .filter_map(|s| s.as_ref().map(|stack| stack.item))
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default();
        if items.is_empty() {
            return false;
        }
        let sound_pos = glam::Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
        for item in &items {
            self.spawn_dropped_item(*item, sound_pos);
        }
        true
    }

    pub(super) fn drop_block_entity_inventory(&mut self, pos: (i32, i32, i32)) -> bool {
        let Some(mut entity) = self
            .chunk_manager
            .get_block_entity(pos.0, pos.1, pos.2)
            .cloned()
        else {
            return false;
        };
        let stacks = entity.drain_stacks();
        if stacks.is_empty() {
            self.chunk_manager
                .set_block_entity(pos.0, pos.1, pos.2, None);
            self.chunk_manager.mark_block_entity_dirty(pos.0, pos.2);
            self.redstone
                .mark_container_changed(&self.chunk_manager, pos);
            self.broadcast_block_entity_delta(pos.0, pos.1, pos.2, None);
            return false;
        }
        let sound_pos = Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
        for stack in stacks {
            self.spawn_dropped_stack(stack, sound_pos);
        }
        self.chunk_manager
            .set_block_entity(pos.0, pos.1, pos.2, None);
        self.chunk_manager.mark_block_entity_dirty(pos.0, pos.2);
        self.redstone
            .mark_container_changed(&self.chunk_manager, pos);
        self.broadcast_block_entity_delta(pos.0, pos.1, pos.2, None);
        true
    }

    pub(super) fn close_legacy_container_sessions_at(&mut self, position: (i32, i32, i32)) {
        let affected = self.container_sessions.close_by_block(
            self.current_dimension as u8,
            position.0,
            position.1,
            position.2,
        );
        for session in affected {
            if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                let _ = host_to_server.tracked_send(
                    crate::network::server::HostToServer::SendContainerClose {
                        to: session.player_id,
                        dimension: session.dimension,
                        x: session.x,
                        y: session.y,
                        z: session.z,
                    },
                );
            }
        }
        if self.container_target == Some(position) {
            self.force_close_inventory();
        }
    }

    pub(super) fn perform_enchantment(&mut self, index: usize) {
        if self
            .presentation_topology()
            .inventory_decision(PresentationInventoryTarget::Workstation)
            != PresentationInventoryAction::LocalMutate
        {
            return;
        }
        let Some(mut input) = self.enchanting.input else {
            return;
        };
        if !crate::enchantment::can_enchant(input.item) {
            return;
        }
        let option = self.enchanting.options[index];
        let lapis_available = self
            .enchanting
            .lapis
            .filter(|stack| stack.item == Item::LapisLazuli)
            .map(|stack| stack.count)
            .unwrap_or(0);
        let affordable = self.game_mode == GameMode::Creative
            || (lapis_available >= option.lapis_cost as u32
                && self.player_state.experience_level >= option.cost as u32);
        if !affordable {
            return;
        }
        input.enchantments.merge(&option.enchantments);
        self.enchanting.input = Some(input);
        self.trigger_advancement(crate::advancements::AdvancementTrigger::EnchantItem);
        if self.game_mode == GameMode::Survival {
            self.player_state.spend_levels(option.cost as u32);
            if let Some(lapis) = &mut self.enchanting.lapis {
                if lapis.count > option.lapis_cost as u32 {
                    lapis.count -= option.lapis_cost as u32;
                } else {
                    self.enchanting.lapis = None;
                }
            }
        }
        self.enchanting.seed = self.enchanting.seed.wrapping_add(0x9E37_79B9);
        self.enchanting.refresh();
    }

    pub(super) fn legacy_apply_inventory_ui_hit(
        &mut self,
        probe: InventoryHitProbe<SlotType>,
        is_left: bool,
    ) {
        match probe.ui_hit() {
            InventoryHit::CreativeTab { index } => {
                if let Some(tab) = CreativeTab::TABS.get(index).copied() {
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::UiClick);
                    self.inventory.select_creative_tab(tab);
                }
            }
            InventoryHit::RecipeBookToggle => {
                self.recipe_book_open = !self.recipe_book_open;
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
            }
            InventoryHit::Merchant { offer_index } => {
                if self
                    .active_merchant_offers
                    .get(offer_index)
                    .is_some_and(|offer| !offer.is_out_of_stock())
                {
                    let _ = self.execute_active_merchant_trade(offer_index);
                }
            }
            InventoryHit::RecipeBook => {
                if self
                    .presentation_topology()
                    .inventory_decision(PresentationInventoryTarget::Workstation)
                    != PresentationInventoryAction::LocalMutate
                {
                    return;
                }
                let mouse_y = self.mouse_ndc[1];
                let smelting_recipes = self.recipe_manager.get_smelting_recipes();
                let mut line_y = 0.34;
                for r in smelting_recipes {
                    if mouse_y >= line_y - 0.05 && mouse_y <= line_y + 0.02 {
                        if let Some(pos) = self.container_target {
                            let block = self.chunk_manager.get_block(pos.0, pos.1, pos.2);
                            if matches!(block, BlockType::Furnace | BlockType::FurnaceLit) {
                                if let Some((inv_slot_idx, stack)) =
                                    self.inventory.find_item(r.input)
                                {
                                    let (cx, cz) = (pos.0.div_euclid(16), pos.2.div_euclid(16));
                                    let (bx, by, bz) = (
                                        pos.0.rem_euclid(16) as u8,
                                        pos.1 as i16,
                                        pos.2.rem_euclid(16) as u8,
                                    );
                                    if let Some(chunk) =
                                        self.chunk_manager.chunks.get_mut(&(cx, cz))
                                    {
                                        if let Some(crate::block_entity::BlockEntity::Furnace(
                                            ref mut f,
                                        )) = chunk.get_block_entity_mut(bx, by, bz)
                                        {
                                            if f.slots[0].is_none() {
                                                f.slots[0] = Some(stack);
                                                self.inventory.remove_at_slot(inv_slot_idx);
                                                self.audio_manager
                                                    .play_sound(crate::audio::SoundId::UiClick);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        break;
                    }
                    line_y -= 0.07;
                    if line_y < -0.40 {
                        break;
                    }
                }
            }
            InventoryHit::Enchant { option_index } => {
                if self
                    .presentation_topology()
                    .inventory_decision(PresentationInventoryTarget::Workstation)
                    != PresentationInventoryAction::LocalMutate
                {
                    return;
                }
                self.perform_enchantment(option_index);
            }
            InventoryHit::Empty => {
                if !self.presentation_topology().should_mutate_world() {
                    return;
                }
                if let Some(dragged) = self.inventory.dragged {
                    self.throw_dropped_item(dragged.item, dragged.count);
                    self.inventory.dragged = None;
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::UiClick);
                }
            }
            InventoryHit::Slot(slot_type) => {
                let slot_item = self.get_item_at_slot(slot_type);
                let creative_catalog = self.is_creative_catalog_open();

                if self.is_creative_catalog_open() {
                    let aspect = self.size.width as f32 / self.size.height as f32;
                    let track = creative_scroll_track_rect(aspect);
                    if is_left && track.contains(self.mouse_ndc[0], self.mouse_ndc[1]) {
                        return;
                    }
                }

                match slot_type {
                    SlotType::Creative(item) => {
                        self.audio_manager
                            .play_sound(crate::audio::SoundId::UiClick);
                        self.inventory.creative_supply(item, is_left);
                        return;
                    }
                    SlotType::Hotbar(index) if creative_catalog => {
                        self.audio_manager
                            .play_sound(crate::audio::SoundId::UiClick);
                        self.inventory.click_creative_hotbar(index, is_left);
                        return;
                    }
                    _ => {}
                }

                if let Some(dragged) = self.inventory.dragged {
                    if !self.slot_accepts(slot_type, dragged) {
                        return;
                    }
                }

                match slot_type {
                    SlotType::CraftOutput => {
                        if let Some(output) = slot_item {
                            self.trigger_advancement(
                                crate::advancements::AdvancementTrigger::CraftItem(output.item),
                            );
                            let max_stack = output.item.properties().max_stack;
                            if self.inventory.dragged.is_none() {
                                self.inventory.dragged = Some(output);
                                for slot in self.inventory.craft_input.iter_mut() {
                                    if let Some(stack) = slot {
                                        if stack.count > 1 {
                                            stack.count -= 1;
                                        } else {
                                            *slot = None;
                                        }
                                    }
                                }
                                let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                                self.inventory.craft_output = self
                                    .recipe_manager
                                    .match_recipe(&self.inventory.craft_input, grid_size);
                                self.audio_manager
                                    .play_sound(crate::audio::SoundId::UiClick);
                            } else if let Some(ref mut dragged) = self.inventory.dragged {
                                if dragged.can_merge_with(&output)
                                    && dragged.count + output.count <= max_stack
                                {
                                    dragged.count += output.count;
                                    for slot in self.inventory.craft_input.iter_mut() {
                                        if let Some(stack) = slot {
                                            if stack.count > 1 {
                                                stack.count -= 1;
                                            } else {
                                                *slot = None;
                                            }
                                        }
                                    }
                                    let grid_size =
                                        if self.inventory.is_table_open { 3 } else { 2 };
                                    self.inventory.craft_output = self
                                        .recipe_manager
                                        .match_recipe(&self.inventory.craft_input, grid_size);
                                    self.audio_manager
                                        .play_sound(crate::audio::SoundId::UiClick);
                                }
                            }
                        }
                    }
                    SlotType::AnvilOutput => {
                        if self
                            .presentation_topology()
                            .inventory_decision(PresentationInventoryTarget::Workstation)
                            != PresentationInventoryAction::LocalMutate
                        {
                            return;
                        }
                        if let Some(output) = self.anvil.output {
                            let affordable = self.game_mode == GameMode::Creative
                                || self.player_state.experience_level >= self.anvil.cost as u32;
                            if affordable && self.inventory.dragged.is_none() {
                                if self.game_mode == GameMode::Survival {
                                    self.player_state.spend_levels(self.anvil.cost as u32);
                                }
                                self.inventory.dragged = Some(output);
                                self.anvil.left = None;
                                self.anvil.right = None;
                                self.anvil.output = None;
                                self.anvil.rename.clear();
                                self.anvil.refresh();
                                self.audio_manager
                                    .play_sound(crate::audio::SoundId::UiClick);
                            }
                        }
                    }
                    SlotType::ContainerSlot(slot)
                        if !self.presentation_topology().is_legacy_owner() =>
                    {
                        self.submit_local_authority_container_action(
                            self.container_target.unwrap_or((0, 0, 0)),
                            crate::network::protocol::ContainerAction::Click,
                            slot as u16,
                            is_left,
                        );
                    }
                    _ => {
                        let max_stack = slot_item
                            .map(|s| s.item.properties().max_stack)
                            .unwrap_or(64);

                        if is_left {
                            if let Some(dragged) = self.inventory.dragged {
                                if let Some(slot) = slot_item {
                                    if slot.can_merge_with(&dragged) {
                                        let space = max_stack.saturating_sub(slot.count);
                                        let transfer = space.min(dragged.count);
                                        let new_slot_count = slot.count + transfer;
                                        let new_drag_count = dragged.count - transfer;

                                        self.set_item_at_slot(
                                            slot_type,
                                            Some(ItemStack {
                                                count: new_slot_count,
                                                ..slot
                                            }),
                                        );
                                        if new_drag_count > 0 {
                                            self.inventory.dragged = Some(ItemStack {
                                                count: new_drag_count,
                                                ..dragged
                                            });
                                        } else {
                                            self.inventory.dragged = None;
                                        }
                                    } else {
                                        self.set_item_at_slot(slot_type, Some(dragged));
                                        self.inventory.dragged = Some(slot);
                                    }
                                } else {
                                    self.set_item_at_slot(slot_type, Some(dragged));
                                    self.inventory.dragged = None;
                                }
                                self.audio_manager
                                    .play_sound(crate::audio::SoundId::UiClick);
                            } else if let Some(slot) = slot_item {
                                self.inventory.dragged = Some(slot);
                                self.set_item_at_slot(slot_type, None);
                                self.audio_manager
                                    .play_sound(crate::audio::SoundId::UiClick);
                            }
                        } else {
                            if let Some(dragged) = self.inventory.dragged {
                                if let Some(slot) = slot_item {
                                    if slot.can_merge_with(&dragged) && slot.count < max_stack {
                                        self.set_item_at_slot(
                                            slot_type,
                                            Some(ItemStack {
                                                count: slot.count + 1,
                                                ..slot
                                            }),
                                        );
                                        if dragged.count > 1 {
                                            self.inventory.dragged = Some(ItemStack {
                                                count: dragged.count - 1,
                                                ..dragged
                                            });
                                        } else {
                                            self.inventory.dragged = None;
                                        }
                                        self.audio_manager
                                            .play_sound(crate::audio::SoundId::UiClick);
                                    } else if !slot.can_merge_with(&dragged) {
                                        self.set_item_at_slot(slot_type, Some(dragged));
                                        self.inventory.dragged = Some(slot);
                                        self.audio_manager
                                            .play_sound(crate::audio::SoundId::UiClick);
                                    }
                                } else {
                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: 1,
                                            ..dragged
                                        }),
                                    );
                                    if dragged.count > 1 {
                                        self.inventory.dragged = Some(ItemStack {
                                            count: dragged.count - 1,
                                            ..dragged
                                        });
                                    } else {
                                        self.inventory.dragged = None;
                                    }
                                    self.audio_manager
                                        .play_sound(crate::audio::SoundId::UiClick);
                                }
                            } else if let Some(slot) = slot_item {
                                let take = (slot.count + 1) / 2;
                                let keep = slot.count - take;
                                self.inventory.dragged = Some(ItemStack {
                                    count: take,
                                    ..slot
                                });
                                if keep > 0 {
                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: keep,
                                            ..slot
                                        }),
                                    );
                                } else {
                                    self.set_item_at_slot(slot_type, None);
                                }
                                self.audio_manager
                                    .play_sound(crate::audio::SoundId::UiClick);
                            }
                        }

                        if let SlotType::ContainerSlot(2) = slot_type {
                            self.check_claim_furnace_xp(slot_type);
                        }

                        if let SlotType::CraftInput(_) = slot_type {
                            let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                            self.inventory.craft_output = self
                                .recipe_manager
                                .match_recipe(&self.inventory.craft_input, grid_size);
                        }
                        self.refresh_workstations();
                    }
                }
            }
        }
    }

    pub(super) fn legacy_open_chest(&mut self, pos: (i32, i32, i32)) {
        let (cx, cz) = (
            pos.0.div_euclid(crate::world::CHUNK_WIDTH as i32),
            pos.2.div_euclid(crate::world::CHUNK_DEPTH as i32),
        );
        let (bx, by, bz) = (
            pos.0.rem_euclid(crate::world::CHUNK_WIDTH as i32) as u8,
            pos.1 as i16,
            pos.2.rem_euclid(crate::world::CHUNK_DEPTH as i32) as u8,
        );
        let Some(chunk) = self.chunk_manager.chunks.get(&(cx, cz)) else {
            return;
        };
        let Some(entity) = chunk.get_block_entity(bx, by, bz) else {
            return;
        };
        if !matches!(
            entity,
            crate::block_entity::BlockEntity::Chest(_)
                | crate::block_entity::BlockEntity::Furnace(_)
                | crate::block_entity::BlockEntity::Hopper(_)
                | crate::block_entity::BlockEntity::Dispenser(_)
                | crate::block_entity::BlockEntity::Dropper(_)
        ) {
            return;
        }
        self.container_target = Some(pos);
        let slot_count = self.chunk_manager.container_slot_count(pos.0, pos.1, pos.2);
        self.container_is_double = slot_count > 27;
        self.set_local_chest_open_state(pos, true);
        self.open_inventory();
    }

    pub(super) fn legacy_chest_viewer_count(
        &self,
        dimension: u8,
        position: (i32, i32, i32),
    ) -> usize {
        let mut count = self
            .container_sessions
            .viewer_count(dimension, position.0, position.1, position.2);
        let block = self
            .chunk_manager
            .get_block(position.0, position.1, position.2);
        if matches!(block, BlockType::Chest | BlockType::EndCityChest) {
            if let Some(partner) =
                crate::block_entity::double_chest_partner(&self.chunk_manager, position)
            {
                count += self
                    .container_sessions
                    .viewer_count(dimension, partner.0, partner.1, partner.2);
            }
        }
        count
    }

    pub(super) fn set_local_chest_open_state(&mut self, position: (i32, i32, i32), open: bool) {
        let block = self
            .chunk_manager
            .get_block(position.0, position.1, position.2);
        if !matches!(block, BlockType::Chest | BlockType::EndCityChest) {
            return;
        }
        let current_state = self
            .chunk_manager
            .get_block_state(position.0, position.1, position.2);
        let mut state = crate::world::BlockState::decode(current_state);
        if state.is_open == open {
            return;
        }
        state.is_open = open;
        self.chunk_manager
            .set_block_state(position.0, position.1, position.2, state.encode());
        self.audio_manager.play_sound(if open {
            crate::audio::SoundId::ChestOpen
        } else {
            crate::audio::SoundId::ChestClose
        });
        self.broadcast_block_change(position.0, position.1, position.2, block);
        if let Some(partner) =
            crate::block_entity::double_chest_partner(&self.chunk_manager, position)
        {
            let mut partner_state = crate::world::BlockState::decode(
                self.chunk_manager
                    .get_block_state(partner.0, partner.1, partner.2),
            );
            if partner_state.is_open != open {
                partner_state.is_open = open;
                self.chunk_manager.set_block_state(
                    partner.0,
                    partner.1,
                    partner.2,
                    partner_state.encode(),
                );
                self.broadcast_block_change(partner.0, partner.1, partner.2, block);
            }
        }
    }

    pub(super) fn legacy_execute_active_merchant_trade(
        &mut self,
        villager_id: u64,
        offer_index: usize,
    ) -> bool {
        let discount = if self.player_state.hero_of_the_village_timer > 0.0 {
            0.3
        } else {
            0.0
        };
        let offer = &self.active_merchant_offers[offer_index];
        if offer.is_out_of_stock() {
            return false;
        }

        let required_a_count = offer.effective_cost_a(discount);
        let mut count_a = 0;
        let mut count_b = 0;

        for slot in self
            .inventory
            .hotbar
            .iter()
            .chain(self.inventory.main.iter())
            .flatten()
        {
            if slot.item == offer.buy_a.item {
                count_a += slot.count;
            }
            if let Some(buy_b) = &offer.buy_b {
                if slot.item == buy_b.item {
                    count_b += slot.count;
                }
            }
        }

        if count_a < required_a_count {
            return false;
        }
        if let Some(buy_b) = &offer.buy_b {
            if count_b < buy_b.count {
                return false;
            }
        }

        // Deduct item A
        let mut needed_a = required_a_count;
        for slot in self
            .inventory
            .hotbar
            .iter_mut()
            .chain(self.inventory.main.iter_mut())
        {
            if needed_a == 0 {
                break;
            }
            if let Some(stack) = slot {
                if stack.item == offer.buy_a.item {
                    let take = stack.count.min(needed_a);
                    stack.count -= take;
                    needed_a -= take;
                    if stack.count == 0 {
                        *slot = None;
                    }
                }
            }
        }

        // Deduct item B if present
        if let Some(buy_b) = &offer.buy_b {
            let mut needed_b = buy_b.count;
            for slot in self
                .inventory
                .hotbar
                .iter_mut()
                .chain(self.inventory.main.iter_mut())
            {
                if needed_b == 0 {
                    break;
                }
                if let Some(stack) = slot {
                    if stack.item == buy_b.item {
                        let take = stack.count.min(needed_b);
                        stack.count -= take;
                        needed_b -= take;
                        if stack.count == 0 {
                            *slot = None;
                        }
                    }
                }
            }
        }

        // Grant sell item
        let _ = self.inventory.add_stack(offer.sell);

        // Update trade offer stock and XP
        let mut new_xp = self.active_merchant_xp;
        let mut new_level = self.active_merchant_level;

        if let Some(offer_mut) = self.active_merchant_offers.get_mut(offer_index) {
            offer_mut.uses += 1;
            new_xp += offer_mut.xp_reward;
        }

        if let Some(next) = new_level.next_level() {
            if new_xp >= next.xp_threshold() {
                new_level = next;
                let new_offers = crate::village::trade::generate_offers_for_level(
                    self.active_merchant_profession,
                    new_level,
                );
                for no in new_offers {
                    if !self
                        .active_merchant_offers
                        .iter()
                        .any(|o| o.buy_a == no.buy_a && o.sell == no.sell)
                    {
                        self.active_merchant_offers.push(no);
                    }
                }
            }
        }

        self.active_merchant_xp = new_xp;
        self.active_merchant_level = new_level;

        // Persist to villager entity
        if let Some(index) = self.entity_manager.id_to_index.get(&villager_id).copied() {
            let entity = &mut self.entity_manager.entities[index];
            entity.villager_xp = new_xp;
            entity.villager_level = new_level;
            entity.offers = self.active_merchant_offers.clone();
        }

        self.trigger_advancement(crate::advancements::AdvancementTrigger::VillagerTrade);
        self.audio_manager
            .play_sound(crate::audio::SoundId::UiClick);
        true
    }

    pub(super) fn legacy_close_inventory(&mut self) -> bool {
        if !matches!(self.role, MultiplayerRole::Client { .. }) {
            if let Some(position) = self.container_target {
                if self.legacy_chest_viewer_count(self.current_dimension as u8, position) == 0 {
                    self.set_local_chest_open_state(position, false);
                }
            }
        }
        if matches!(
            self.role,
            crate::presentation_inventory_policy::MultiplayerRole::Client { .. }
        ) {
            if let Some(pos) = self.container_target {
                self.network.request_gameplay(crate::network::protocol::GameplayRequest {
                    request_id: 0,
                    client_sequence: 0,
                    session_id: 0,
                    dimension: self.current_dimension as u8,
                    client_revision: 0,
                    operation: crate::network::protocol::GameplayOperation::Container {
                        action: 2,
                        x: pos.0,
                        y: pos.1,
                        z: pos.2,
                        slot: 0,
                    },
                });
            }
        }
        let mut returning_items: Vec<ItemStack> = self
            .inventory
            .craft_input
            .iter()
            .flatten()
            .copied()
            .collect();
        returning_items.extend(match self.active_station {
            Some(StationKind::Enchanting) => [self.enchanting.input, self.enchanting.lapis]
                .into_iter()
                .flatten()
                .collect(),
            Some(StationKind::Brewing) => self
                .brewing
                .bottles
                .iter()
                .copied()
                .chain(std::iter::once(self.brewing.ingredient))
                .flatten()
                .collect(),
            Some(StationKind::Anvil) => [self.anvil.left, self.anvil.right]
                .into_iter()
                .flatten()
                .collect(),
            None | Some(StationKind::Furnace) | Some(StationKind::Merchant) => Vec::new(),
        });

        for stack in returning_items {
            if let Some(remainder) = self.inventory.add_stack(stack) {
                self.throw_dropped_item(remainder.item, remainder.count);
            }
        }

        if self.inventory.creative_drag_origin
            == Some(crate::inventory::CreativeDragOrigin::Catalog)
        {
            self.inventory.dragged = None;
            self.inventory.creative_drag_origin = None;
        } else if let Some(dragged) = self.inventory.dragged {
            if let Some(remainder) = self.inventory.add_stack(dragged) {
                self.throw_dropped_item(remainder.item, remainder.count);
            }
            self.inventory.dragged = None;
            self.inventory.creative_drag_origin = None;
        }

        self.inventory.craft_input.fill(None);
        match self.active_station {
            Some(StationKind::Enchanting) => {
                self.enchanting.input = None;
                self.enchanting.lapis = None;
            }
            Some(StationKind::Brewing) => {
                self.brewing.bottles.fill(None);
                self.brewing.ingredient = None;
            }
            Some(StationKind::Anvil) => {
                self.anvil.left = None;
                self.anvil.right = None;
            }
            None | Some(StationKind::Furnace) | Some(StationKind::Merchant) => {}
        }

        self.inventory.is_open = false;
        self.inventory.is_table_open = false;
        self.inventory.craft_input = vec![None; 4];
        self.inventory.craft_output = None;
        if self.active_station == Some(StationKind::Merchant) {
            if let Some(vid) = self.active_merchant_villager_id {
                self.merchant_sessions.close_sessions_for_villager(vid);
            }
            self.active_merchant_villager_id = None;
            self.active_merchant_offers.clear();
        }
        self.active_station = None;
        self.container_target = None;
        self.container_is_double = false;
        self.anvil.rename.clear();

        self.sync_cursor_mode();
        true
    }
}

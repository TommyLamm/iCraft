use crate::chunk_manager::{mark_block_mesh_dependencies, ChunkManager};
use crate::entity::{Entity, EntityManager, EntityType};
use crate::inventory::{GameMode, Item};
use crate::physics::PlayerPhysics;
use glam::Vec3;

fn check_cliff_ahead(entity: &Entity, chunk_manager: &ChunkManager) -> bool {
    // Calculate walking direction unit vector
    let dir_x = -entity.yaw.sin();
    let dir_z = -entity.yaw.cos();

    let check_x = (entity.position.x + dir_x * 1.0).floor() as i32;
    let check_z = (entity.position.z + dir_z * 1.0).floor() as i32;
    let feet_y = entity.position.y.floor() as i32;

    // If the block at feet-1 and feet-2 is Air, it's a cliff
    let below1 = chunk_manager.get_block(check_x, feet_y - 1, check_z);
    let below2 = chunk_manager.get_block(check_x, feet_y - 2, check_z);

    !below1.properties().is_solid && !below2.properties().is_solid
}

pub fn update_passive_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &mut ChunkManager,
    chunk_meshes: &mut std::collections::HashMap<(i32, i32), crate::state::ChunkMesh>,
    player_physics: &PlayerPhysics,
    inventory: &mut crate::inventory::Inventory,
    game_mode: GameMode,
    dt: f32,
    time: f32,
) {
    let player_pos = player_physics.position;
    let mut hearts_to_spawn = Vec::new();
    let mut baby_mobs_to_spawn = Vec::new();

    // Collect entities and process their individual AI
    let entity_len = entity_manager.entities.len();
    for i in 0..entity_len {
        let (entity_type, pos, invuln, age, breed_timer, breed_cd, _has_wool) = {
            let e = &entity_manager.entities[i];
            (
                e.entity_type,
                e.position,
                e.invulnerable_time,
                e.age,
                e.breeding_timer,
                e.breed_cooldown,
                e.has_wool,
            )
        };

        if !entity_type.is_passive() {
            continue;
        }

        // Handle age increments
        if age < 0.0 {
            entity_manager.entities[i].age += dt;
        }

        // Handle breeding timers & cooldowns
        if breed_timer > 0.0 {
            entity_manager.entities[i].breeding_timer = (breed_timer - dt).max(0.0);

            // Spawn heart particles periodically
            if (time * 2.0) as u32 % 4 == 0 && (time * 10.0) as u32 % 5 == 0 {
                hearts_to_spawn.push(pos + Vec3::new(0.0, 1.0, 0.0));
            }
        }
        if breed_cd > 0.0 {
            entity_manager.entities[i].breed_cooldown = (breed_cd - dt).max(0.0);
        }

        // Chicken Egg Laying timer
        if entity_type == EntityType::Chicken {
            let lay_timer = entity_manager.entities[i].egg_lay_timer - dt;
            if lay_timer <= 0.0 {
                // Lay egg
                entity_manager.entities[i].egg_lay_timer = 300.0 + (pos.x + pos.z) % 300.0;
                if pos.distance(player_pos) <= 16.0 && game_mode == GameMode::Survival {
                    println!("[Debug] Chicken laid an egg in your pocket!");
                    inventory.add_item(Item::Egg);
                }
            } else {
                entity_manager.entities[i].egg_lay_timer = lay_timer;
            }
        }

        // Sheep Grazing state
        if entity_type == EntityType::Sheep {
            let eat_t = entity_manager.entities[i].grass_eat_timer;
            if eat_t > 0.0 {
                entity_manager.entities[i].grass_eat_timer = (eat_t - dt).max(0.0);
                if eat_t - dt <= 0.0 {
                    // Grazing action finishes
                    let sx = pos.x.floor() as i32;
                    let sy = (pos.y - 0.5).floor() as i32;
                    let sz = pos.z.floor() as i32;
                    if chunk_manager.get_block(sx, sy, sz) == crate::world::BlockType::Grass {
                        chunk_manager.set_block(sx, sy, sz, crate::world::BlockType::Dirt);

                        let mut dirty_chunks = std::collections::HashSet::new();
                        mark_block_mesh_dependencies(&mut dirty_chunks, sx, sz);
                        for chunk_position in dirty_chunks {
                            if let Some(mesh) = chunk_meshes.get_mut(&chunk_position) {
                                mesh.dirty = true;
                            }
                        }
                    }
                    entity_manager.entities[i].has_wool = true; // wool grows back!
                }
                entity_manager.entities[i].velocity = Vec3::ZERO;
                continue; // skip other movement during grazing
            } else if (time as u32 % 20 == 0) && (pos.x + pos.z) as u32 % 5 == 0 {
                // 1% chance to graze if on ground
                if entity_manager.entities[i].on_ground {
                    entity_manager.entities[i].grass_eat_timer = 1.5;
                    continue;
                }
            }
        }

        // Movement speed & direction selection
        let mut speed = 1.0;
        let is_panicking = invuln > 0.0;

        if is_panicking {
            speed = 4.0;
            // Run away from player
            let away_dir = (pos - player_pos).normalize_or_zero();
            entity_manager.entities[i].yaw = f32::atan2(-away_dir.x, -away_dir.z);
        } else if breed_timer > 0.0 {
            // Seeking mating partner
            let mut nearest_partner = None;
            let mut nearest_dist = 999.0;
            for j in 0..entity_len {
                if i == j {
                    continue;
                }
                let partner = &entity_manager.entities[j];
                if partner.entity_type == entity_type && partner.breeding_timer > 0.0 {
                    let dist = pos.distance(partner.position);
                    if dist < nearest_dist {
                        nearest_dist = dist;
                        nearest_partner = Some(partner.position);
                    }
                }
            }

            if let Some(partner_pos) = nearest_partner {
                let mate_dir = (partner_pos - pos).normalize_or_zero();
                entity_manager.entities[i].yaw = f32::atan2(-mate_dir.x, -mate_dir.z);
                speed = 1.5;

                // If touching, spawn offspring
                if nearest_dist <= 1.2 && breed_cd <= 0.0 {
                    // Trigger mating
                    entity_manager.entities[i].breeding_timer = 0.0;
                    entity_manager.entities[i].breed_cooldown = 300.0;

                    // Find and update partner
                    for j in 0..entity_len {
                        if entity_manager.entities[j].entity_type == entity_type
                            && entity_manager.entities[j].breeding_timer > 0.0
                        {
                            if entity_manager.entities[j].position.distance(pos) <= 1.5 {
                                entity_manager.entities[j].breeding_timer = 0.0;
                                entity_manager.entities[j].breed_cooldown = 300.0;
                                break;
                            }
                        }
                    }

                    baby_mobs_to_spawn.push((entity_type, (pos + partner_pos) * 0.5));
                    for _ in 0..5 {
                        hearts_to_spawn.push((pos + partner_pos) * 0.5 + Vec3::new(0.0, 0.5, 0.0));
                    }
                    println!("[Debug] Spawned baby {:?}", entity_type);
                }
            }
        } else if age < 0.0 {
            // Follow nearest adult parent
            let mut nearest_adult = None;
            let mut nearest_dist = 999.0;
            for j in 0..entity_len {
                let adult = &entity_manager.entities[j];
                if adult.entity_type == entity_type && adult.age >= 0.0 {
                    let dist = pos.distance(adult.position);
                    if dist < nearest_dist {
                        nearest_dist = dist;
                        nearest_adult = Some(adult.position);
                    }
                }
            }

            if let Some(adult_pos) = nearest_adult {
                if nearest_dist > 2.0 {
                    let follow_dir = (adult_pos - pos).normalize_or_zero();
                    entity_manager.entities[i].yaw = f32::atan2(-follow_dir.x, -follow_dir.z);
                    speed = 1.5;
                } else {
                    speed = 0.0;
                }
            }
        } else {
            // Standard wandering AI: choose random direction occasionally
            let is_moving = Vec3::new(
                entity_manager.entities[i].velocity.x,
                0.0,
                entity_manager.entities[i].velocity.z,
            )
            .length()
                > 0.1;
            let seed = (pos.x.to_bits() ^ pos.z.to_bits()) as u32;
            if !is_moving && (time * 100.0) as u32 % 500 == 0 {
                // Turn to a random angle
                let rng = seed.wrapping_add((time * 1000.0) as u32);
                let rand_val = (rng.wrapping_mul(1103515245).wrapping_add(12345) / 65536) % 360;
                entity_manager.entities[i].yaw = (rand_val as f32) * std::f32::consts::PI / 180.0;
            }
            if !is_moving {
                speed = 0.0;
            }
        }

        // Cliff Avoidance check
        if speed > 0.0 && check_cliff_ahead(&entity_manager.entities[i], chunk_manager) {
            speed = 0.0;
            // Pivot away from cliff
            entity_manager.entities[i].yaw += std::f32::consts::FRAC_PI_2;
        }

        // Set horizontal velocity based on current yaw and speed
        if speed > 0.0 {
            let dir_x = -entity_manager.entities[i].yaw.sin();
            let dir_z = -entity_manager.entities[i].yaw.cos();
            entity_manager.entities[i].velocity.x = dir_x * speed;
            entity_manager.entities[i].velocity.z = dir_z * speed;

            // Jump if blocked
            let check_pos = pos + Vec3::new(dir_x * 0.45, 0.0, dir_z * 0.45);
            let bx = check_pos.x.floor() as i32;
            let bz = check_pos.z.floor() as i32;
            let by = pos.y.floor() as i32;
            if entity_manager.entities[i].on_ground
                && chunk_manager.get_block(bx, by, bz).properties().is_solid
            {
                entity_manager.entities[i].velocity.y = 7.0; // Jump height
            }
        } else {
            entity_manager.entities[i].velocity.x = 0.0;
            entity_manager.entities[i].velocity.z = 0.0;
        }
    }

    // Spawn new offspring baby mobs
    for (et, baby_pos) in baby_mobs_to_spawn {
        let baby_id = entity_manager.spawn(et, baby_pos);
        if let Some(baby) = entity_manager.entities.iter_mut().find(|e| e.id == baby_id) {
            baby.age = -120.0; // Start as baby
        }
    }

    // Spawn heart particles
    for h_pos in hearts_to_spawn {
        let id = entity_manager.spawn(EntityType::HeartParticle, h_pos);
        if let Some(p) = entity_manager.entities.iter_mut().find(|e| e.id == id) {
            let time_seed = (h_pos.x.to_bits() ^ h_pos.z.to_bits()) as u32;
            let rand_x = ((time_seed % 100) as f32 - 50.0) / 100.0;
            let rand_z = (((time_seed / 100) % 100) as f32 - 50.0) / 100.0;
            p.velocity = Vec3::new(rand_x * 0.5, 1.5, rand_z * 0.5);
            p.life_time = 1.5;

            // Task 6 Step 2: Set heart particle rotation
            let to_player = (player_pos - h_pos).normalize_or_zero();
            p.yaw = f32::atan2(-to_player.x, -to_player.z);
            p.pitch = f32::asin(to_player.y);
        }
    }

    // Clean up dead/expired particles
    entity_manager.entities.retain(|entity| {
        if entity.entity_type == EntityType::HeartParticle {
            entity.life_time > 0.0
        } else {
            true
        }
    });

    // Update particle lifetimes
    for entity in &mut entity_manager.entities {
        if entity.entity_type == EntityType::HeartParticle {
            entity.life_time -= dt;
        }
    }
}

pub fn spawn_passive_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &ChunkManager,
    player_pos: Vec3,
    sky_light_level: u8,
    time: f32,
) {
    // Limit total entities to prevent lag
    if entity_manager.entities.len() >= 35 {
        return;
    }

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

    // ~1% chance to attempt a spawn in daytime
    if sky_light_level < 10 || next_rand() % 100 != 0 {
        return;
    }

    let angle = (next_rand() % 360) as f32 * std::f32::consts::PI / 180.0;
    let dist = (24 + (next_rand() % 56)) as f32;
    let spawn_x = (player_pos.x + angle.cos() * dist) as i32;
    let spawn_z = (player_pos.z + angle.sin() * dist) as i32;

    // Find highest solid block
    let mut highest_y = None;
    for y in (0..crate::world::CHUNK_HEIGHT as i32).rev() {
        if chunk_manager
            .get_block(spawn_x, y, spawn_z)
            .properties()
            .is_solid
        {
            highest_y = Some(y);
            break;
        }
    }

    if let Some(solid_y) = highest_y {
        let spawn_y = solid_y + 1;
        if spawn_y > 0 && spawn_y < (crate::world::CHUNK_HEIGHT as i32 - 2) {
            let block_below = chunk_manager.get_block(spawn_x, solid_y, spawn_z);
            let block_feet = chunk_manager.get_block(spawn_x, spawn_y, spawn_z);
            let block_head = chunk_manager.get_block(spawn_x, spawn_y + 1, spawn_z);

            // Passive mobs spawn on Grass Blocks under daylight
            if block_below == crate::world::BlockType::Grass
                && block_feet == crate::world::BlockType::Air
                && block_head == crate::world::BlockType::Air
            {
                let r = next_rand() % 4;
                let et = match r {
                    0 => EntityType::Pig,
                    1 => EntityType::Cow,
                    2 => EntityType::Sheep,
                    _ => EntityType::Chicken,
                };
                entity_manager.spawn(
                    et,
                    Vec3::new(spawn_x as f32 + 0.5, spawn_y as f32, spawn_z as f32 + 0.5),
                );
                println!(
                    "[Debug] Spawned passive {:?} at ({}, {}, {})",
                    et, spawn_x, spawn_y, spawn_z
                );
            }
        }
    }
}

//! Render prepare / encode helpers extracted from State::render.
//! Pass order is unchanged: sky -> opaque -> entities -> translucent ->
//! particles -> mining -> hand -> UI -> crosshair -> text.
//! Timestamp query and frame-slot acquire/wait stay in the orchestrator.

use super::*;
use glam::Mat4;
use std::time::{Duration, Instant};

impl State {
    pub(super) fn prepare_terrain_draw_plan(&mut self) {
        let terrain_prepare_started = Instant::now();
        let view_projection = Mat4::from_cols_array_2d(&self.camera_uniform.view_proj);
        let frustum = Frustum::from_view_projection(view_projection);

        let cam_pos = self.camera.position;
        let render_blocks = self.chunk_manager.render_distance as f32 * CHUNK_WIDTH as f32;
        let render_distance_sq = render_blocks * render_blocks;
        let r_i32 = self.chunk_manager.render_distance as i32;

        let cam_sec_x = (cam_pos.x / 16.0).floor() as i32;
        let cam_sec_y_raw = (cam_pos.y / 16.0).floor() as i32;
        let cam_sec_z = (cam_pos.z / 16.0).floor() as i32;

        let height = self.chunk_manager.dimension.height();
        let fail_open_section_vis = cam_sec_y_raw < height.min_section_y() as i32
            || cam_sec_y_raw >= height.max_section_y_exclusive() as i32
            || !self.chunk_meshes.contains_key(&(cam_sec_x, cam_sec_z));

        if !fail_open_section_vis {
            crate::culling::traverse_section_visibility_with_scratch(
                cam_sec_x,
                cam_sec_y_raw as i8,
                cam_sec_z,
                r_i32,
                height,
                &frustum,
                |x, sy, z| {
                    self.chunk_meshes
                        .get(&(x, z))
                        .and_then(|mesh| mesh.section(sy))
                        .map(|section| section.connectivity.fail_open())
                },
                &mut self.visible_sections_scratch,
                &mut self.section_visibility_scratch,
            );
        }

        let lod_thresholds = LodThresholds::new(render_blocks * 0.5, render_blocks * 0.75);
        self.terrain_candidates_scratch.clear();
        let mut occluded_sections = 0u64;
        let mut lod_fills = Vec::new();

        for (&coord, mesh) in &self.chunk_meshes {
            for (sec_idx, section) in mesh.sections.iter().enumerate() {
                let section_y = mesh.section_y_at_index(sec_idx);
                let Some(bounds) = section.finest_bounds() else {
                    continue;
                };

                let distance_sq = bounds.center_distance_squared(cam_pos);
                if distance_sq > render_distance_sq || !frustum.intersects_aabb(&bounds) {
                    continue;
                }

                if !fail_open_section_vis
                    && !self
                        .visible_sections_scratch
                        .contains(&(coord.0, section_y, coord.1))
                {
                    occluded_sections += 1;
                    continue;
                }

                let lod = select_lod_for_bounds(cam_pos, bounds, lod_thresholds);
                if !section.lod_is_built(lod) {
                    lod_fills.push(SectionKey::new(coord.0, section_y, coord.1));
                }
                let Some((draw_lod, level)) = section.level_for_draw(lod) else {
                    continue;
                };
                let key = SectionKey::new(coord.0, section_y, coord.1);

                if let Some(bounds) = level.opaque.bounds {
                    self.terrain_candidates_scratch
                        .push(DrawCandidate::for_section(
                            key,
                            bounds,
                            level.opaque.num_indices(),
                            DrawLayer::Opaque,
                            draw_lod,
                            distance_sq,
                        ));
                }
                if let Some(bounds) = level.transparent.bounds {
                    self.terrain_candidates_scratch
                        .push(DrawCandidate::for_section(
                            key,
                            bounds,
                            level.transparent.num_indices(),
                            DrawLayer::Transparent,
                            draw_lod,
                            distance_sq,
                        ));
                }
            }
        }

        let player_chunk = (
            (cam_pos.x / CHUNK_WIDTH as f32).floor() as i32,
            (cam_pos.z / CHUNK_DEPTH as f32).floor() as i32,
        );
        for key in lod_fills {
            if self.section_scheduler.is_in_flight(key) {
                continue;
            }
            if let Some(identity) = self.current_section_identity(key) {
                self.section_scheduler
                    .enqueue(identity, DependencyReason::ChunkLoad, player_chunk);
            }
        }

        let terrain_candidate_count = self.terrain_candidates_scratch.len();
        self.terrain_draw_plan_scratch
            .build_into(self.terrain_candidates_scratch.iter().copied(), &frustum);
        let draw_plan = &self.terrain_draw_plan_scratch;
        self.submitted_terrain_triangles = draw_plan.submitted_triangle_count();
        self.submitted_terrain_draw_calls = draw_plan.draw_call_count();
        self.visible_chunk_count = draw_plan.visible_chunk_count();
        self.perf_counters.loaded_chunks = self.chunk_manager.chunks.len() as u64;
        self.perf_counters.visible_chunks = self.visible_chunk_count as u64;
        self.perf_counters.occluded_chunks = occluded_sections;
        self.perf_counters.terrain_candidates = terrain_candidate_count as u64;
        self.perf_counters.terrain_triangles = self.submitted_terrain_triangles;
        self.perf_counters.in_flight =
            (self.chunk_load_in_flight.len() + self.section_scheduler.in_flight.len()) as u64;
        let total_committed: usize = self
            .render_regions
            .values()
            .map(|r| r.committed_bytes())
            .sum();
        let total_used: usize = self.render_regions.values().map(|r| r.used_bytes()).sum();
        self.perf_counters.gpu_mesh_bytes = total_committed as u64;
        self.perf_counters.gpu_arena_used_bytes = total_used as u64;
        self.perf_counters.gpu_arena_wasted_bytes =
            total_committed.saturating_sub(total_used) as u64;
        self.perf_counters.gpu_arena_regions = self.render_regions.len() as u64;
        self.perf_counters.gpu_buffer_objects = self
            .render_regions
            .values()
            .map(|region| region.buffer_object_count() as u64)
            .sum();
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareTerrain,
            terrain_prepare_started.elapsed(),
        );
    }

    pub(super) fn prepare_entities(&mut self, _gpu_upload_elapsed: &mut Duration) {
        let cam_pos = self.camera.position;
        let render_blocks = self.chunk_manager.render_distance as f32 * CHUNK_WIDTH as f32;
        let render_distance_sq = render_blocks * render_blocks;
        let view_projection = Mat4::from_cols_array_2d(&self.camera_uniform.view_proj);
        let frustum = Frustum::from_view_projection(view_projection);
        let cam_sec_x = (cam_pos.x / 16.0).floor() as i32;
        let cam_sec_y_raw = (cam_pos.y / 16.0).floor() as i32;
        let cam_sec_z = (cam_pos.z / 16.0).floor() as i32;
        let height = self.chunk_manager.dimension.height();
        let fail_open_section_vis = cam_sec_y_raw < height.min_section_y() as i32
            || cam_sec_y_raw >= height.max_section_y_exclusive() as i32
            || !self.chunk_meshes.contains_key(&(cam_sec_x, cam_sec_z));
        self.entity_los_manager.counters = crate::culling::CullingCounters::default();
        self.entity_los_manager
            .set_current_identity(crate::culling::LosIdentity {
                dimension: self.current_dimension,
                generation: self.terrain_generation,
                world_revision: self.los_world_revision,
            });
        // Poll entity LOS async results
        self.entity_los_manager.poll_results();

        // Compile mob instance data with culling hierarchy
        let entity_prepare_started = Instant::now();
        self.mob_cuboid_instances_scratch.clear();
        self.mob_quad_instances_scratch.clear();

        let mut entities_rendered = 0u64;
        let mut entities_frustum_culled = 0u64;
        let mut entities_occlusion_culled = 0u64;

        let cam_cell = (
            cam_pos.x.floor() as i32,
            cam_pos.y.floor() as i32,
            cam_pos.z.floor() as i32,
        );

        for entity in self
            .entity_manager
            .query_radius(cam_pos, render_distance_sq.sqrt())
        {
            // 1. Distance check
            let entity_render_dist_sq = render_distance_sq
                * (self.settings.entity_distance_scale * self.settings.entity_distance_scale);
            let dist_sq = entity.position.distance_squared(cam_pos);
            if dist_sq > entity_render_dist_sq {
                self.entity_los_manager.counters.distance += 1;
                continue;
            }

            // 2. Frustum check
            let aabb = entity.get_aabb();
            let bounds = crate::chunk_render::MeshBounds::new(aabb.min, aabb.max);
            if !frustum.intersects_aabb(&bounds) {
                entities_frustum_culled += 1;
                self.entity_los_manager.counters.frustum += 1;
                continue;
            }

            // 3. Section visibility check
            let sec_x = (entity.position.x / 16.0).floor() as i32;
            let sec_y = (entity.position.y / 16.0).floor() as i32;
            let sec_z = (entity.position.z / 16.0).floor() as i32;

            if !fail_open_section_vis {
                let valid_y = sec_y.clamp(
                    height.min_section_y() as i32,
                    height.max_section_y_exclusive() as i32 - 1,
                ) as i8;
                if !self
                    .visible_sections_scratch
                    .contains(&(sec_x, valid_y, sec_z))
                {
                    entities_occlusion_culled += 1;
                    self.entity_los_manager.counters.section += 1;
                    continue;
                }
            }

            // 4. Asynchronous Entity LOS check
            if !self.entity_los_manager.is_entity_visible(
                entity,
                cam_pos,
                cam_cell,
                &self.chunk_manager,
            ) {
                entities_occlusion_culled += 1;
                continue;
            }

            entities_rendered += 1;
            crate::mob_renderer::render_mobs(
                std::iter::once(entity),
                &self.chunk_manager,
                &mut self.mob_cuboid_instances_scratch,
                &mut self.mob_quad_instances_scratch,
                self.total_time,
            );
        }

        self.perf_counters.rendered_entities = entities_rendered;
        self.perf_counters.frustum_culled_entities = entities_frustum_culled;
        self.perf_counters.occlusion_culled_entities = entities_occlusion_culled;

        if self.camera_perspective.is_third_person() {
            let held_item = self.inventory.hotbar[self.inventory.selected]
                .map(|stack| stack.item)
                .unwrap_or(Item::Air);
            crate::mob_renderer::render_local_player(
                self.player_physics.position,
                std::f32::consts::FRAC_PI_2 - self.camera.yaw,
                -self.camera.pitch,
                &self.chunk_manager,
                &mut self.mob_cuboid_instances_scratch,
                &mut self.mob_quad_instances_scratch,
                held_item,
                self.total_time,
                self.player_physics.velocity,
            );
        }

        self.mob_cuboid_num_instances = self.mob_cuboid_instances_scratch.len() as u32;
        self.mob_quad_num_instances = self.mob_quad_instances_scratch.len() as u32;

        if self.mob_cuboid_num_instances > 0 {
            let limit = (self.mob_cuboid_num_instances as usize).min(16384);
            self.mob_cuboid_num_instances = limit as u32;
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.mob_cuboid_instance_buffers[self.frame_ring_index],
                0,
                bytemuck::cast_slice(&self.mob_cuboid_instances_scratch[..limit]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Entity as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    (limit * std::mem::size_of::<crate::mob_renderer::MobInstance>()) as u64,
                );
        }

        if self.mob_quad_num_instances > 0 {
            let limit = (self.mob_quad_num_instances as usize).min(4096);
            self.mob_quad_num_instances = limit as u32;
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.mob_quad_instance_buffers[self.frame_ring_index],
                0,
                bytemuck::cast_slice(&self.mob_quad_instances_scratch[..limit]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Entity as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    (limit * std::mem::size_of::<crate::mob_renderer::MobInstance>()) as u64,
                );
        }
        let entity_prepare_elapsed = entity_prepare_started.elapsed();

        // Compile particle instance data
        let particle_prepare_started = Instant::now();
        self.particle_instances_scratch.clear();
        self.particle_num_indices = self
            .particles
            .compile_instances(&mut self.particle_instances_scratch);
        let particle_count = self.particle_instances_scratch.len();
        if particle_count > 0 {
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.particle_instance_buffers[self.frame_ring_index],
                0,
                bytemuck::cast_slice(&self.particle_instances_scratch),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Particle as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    (particle_count * std::mem::size_of::<crate::particles::ParticleInstance>())
                        as u64,
                );
        }
        let particle_prepare_elapsed = particle_prepare_started.elapsed();
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareParticles,
            particle_prepare_elapsed,
        );
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareEntities,
            entity_prepare_elapsed,
        );
    }

    pub(super) fn prepare_hand(&mut self, gpu_upload_elapsed: &mut Duration) {
        // Compile first-person hand mesh in view space. Hidden in third-person.
        let hand_prepare_started = Instant::now();
        if !self.camera_perspective.is_third_person() {
            let speed_2d = Vec3::new(
                self.player_physics.velocity.x,
                0.0,
                self.player_physics.velocity.z,
            )
            .length();
            let walking = speed_2d > 0.1;
            let walk_swing = if walking && self.settings.accessibility.camera_bobbing {
                (self.total_time * 8.0).sin() * 0.6
            } else {
                0.0
            };
            let swing_active = self.left_mouse_pressed || self.total_time < self.hand_swing_until;
            let attack_swing = crate::hand_renderer::hand_swing_progress(
                (self.total_time - self.hand_swing_started_at).max(0.0),
                swing_active,
            );
            let mesh_key = crate::hand_renderer::hand_mesh_key(&self.inventory);
            if crate::hand_renderer::should_rebuild_hand_mesh(self.last_hand_mesh_key, mesh_key) {
                self.last_hand_mesh_key = Some(mesh_key);
                crate::hand_renderer::build_first_person_hand_base_mesh(
                    mesh_key,
                    &mut self.hand_vertices_scratch,
                    &mut self.hand_indices_scratch,
                );
                let hand_vertices_len = self.hand_vertices_scratch.len();
                let hand_indices_len = self.hand_indices_scratch.len();
                let mesh_fits_buffers = hand_vertices_len
                    <= crate::hand_renderer::HAND_VERTEX_CAPACITY
                    && hand_indices_len <= crate::hand_renderer::HAND_INDEX_CAPACITY
                    && self
                        .hand_indices_scratch
                        .iter()
                        .all(|index| (*index as usize) < hand_vertices_len);
                self.hand_num_indices = 0;
                if hand_indices_len > 0 && mesh_fits_buffers {
                    self.hand_num_indices = hand_indices_len as u32;
                    let upload_started = Instant::now();
                    self.queue.write_buffer(
                        &self.hand_vertex_buffer,
                        0,
                        bytemuck::cast_slice(&self.hand_vertices_scratch),
                    );
                    self.queue.write_buffer(
                        &self.hand_index_buffer,
                        0,
                        bytemuck::cast_slice(&self.hand_indices_scratch),
                    );
                    let hand_upload_elapsed = upload_started.elapsed();
                    *gpu_upload_elapsed += hand_upload_elapsed;
                    self.gpu_upload_scopes_frame.record(
                        crate::perf::UploadSource::Entity as usize,
                        hand_upload_elapsed,
                    );
                    self.perf_counters.upload_bytes_frame =
                        self.perf_counters.upload_bytes_frame.saturating_add(
                            (hand_vertices_len * std::mem::size_of::<Vertex>()
                                + hand_indices_len * std::mem::size_of::<u32>())
                                as u64,
                        );
                } else if hand_indices_len > 0 {
                    eprintln!(
                        "Skipping invalid first-person hand mesh: {hand_vertices_len} vertices, \
                         {hand_indices_len} indices"
                    );
                }
            }

            // Animation is a per-frame uniform transform over the cached base
            // mesh; walking and attacking never regenerate or upload vertices.
            let animation =
                crate::hand_renderer::animation_for_hand_mesh(mesh_key, walk_swing, attack_swing);
            let aspect = self.size.width.max(1) as f32 / self.size.height.max(1) as f32;
            let hand_proj = Mat4::perspective_lh(f32::to_radians(70.0), aspect, 0.01, 10.0);
            let combined = hand_proj * animation.matrix();
            let mut hand_uniform = crate::camera::CameraUniform::new();
            hand_uniform.view_proj = combined.to_cols_array_2d();
            hand_uniform.inv_view_proj = combined.inverse().to_cols_array_2d();
            hand_uniform.camera_pos = [0.0, 0.0, 0.0, 0.0];
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.hand_camera_buffer,
                0,
                bytemuck::bytes_of(&hand_uniform),
            );
            let elapsed = upload_started.elapsed();
            *gpu_upload_elapsed += elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Entity as usize, elapsed);
            self.perf_counters.upload_bytes_frame = self
                .perf_counters
                .upload_bytes_frame
                .saturating_add(std::mem::size_of::<crate::camera::CameraUniform>() as u64);
        }
        let _ = hand_prepare_started;
    }

    pub(super) fn build_hud(&mut self, gpu_upload_elapsed: &mut Duration) {
        let font_source = &self.font_source;
        let add_string_lines = |s: &str,
                                start_x: f32,
                                y: f32,
                                char_w: f32,
                                char_h: f32,
                                spacing: f32,
                                color: [f32; 4],
                                vertices: &mut Vec<UiVertex>| {
            add_string_lines_with_source(
                font_source,
                s,
                start_x,
                y,
                char_w,
                char_h,
                spacing,
                color,
                vertices,
            );
        };
        let ui_prepare_started = Instant::now();
        let mut ui_vertices = std::mem::take(&mut self.ui_vertices_scratch);
        let mut ui_line_vertices = std::mem::take(&mut self.ui_line_vertices_scratch);
        ui_vertices.clear();
        ui_line_vertices.clear();
        if self.is_saving || self.save_error.is_some() {
            let bg_color = [0.1, 0.1, 0.1, 0.75];
            add_ui_quad(&mut ui_vertices, -1.0, 1.0, -1.0, 1.0, bg_color);

            if self.save_error.is_some() {
                let [mouse_x, mouse_y] = self.mouse_ndc;
                for (y0, y1) in [(0.02, 0.12), (-0.16, -0.06)] {
                    let hovered = (-0.3..=0.3).contains(&mouse_x) && (y0..=y1).contains(&mouse_y);
                    add_ui_quad(
                        &mut ui_vertices,
                        -0.3,
                        0.3,
                        y0,
                        y1,
                        if hovered {
                            [0.45, 0.18, 0.14, 1.0]
                        } else {
                            [0.22, 0.08, 0.07, 1.0]
                        },
                    );
                    add_ui_border(
                        &mut ui_line_vertices,
                        -0.3,
                        0.3,
                        y0,
                        y1,
                        [0.9, 0.55, 0.45, 1.0],
                    );
                }
            }

            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            if let Some(error) = &self.save_error {
                let save_failed = self.translate("hud.save_failed");
                draw_centered_text(
                    &save_failed,
                    0.38,
                    0.03,
                    0.06,
                    0.012,
                    [1.0, 0.35, 0.28, 1.0],
                    &mut ui_line_vertices,
                );
                let reason: String = error.chars().take(56).collect();
                draw_centered_text(
                    &reason,
                    0.25,
                    0.015,
                    0.03,
                    0.006,
                    [1.0, 0.8, 0.7, 1.0],
                    &mut ui_line_vertices,
                );
                let retry = self.translate("hud.retry");
                draw_centered_text(
                    &retry,
                    0.05,
                    0.025,
                    0.05,
                    0.01,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
                let quit_without_saving = self.translate("hud.quit_without_saving");
                draw_centered_text(
                    &quit_without_saving,
                    -0.13,
                    0.018,
                    0.036,
                    0.007,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            } else {
                let saving_world = self.translate("hud.saving_world");
                draw_centered_text(
                    &saving_world,
                    0.0,
                    0.03,
                    0.06,
                    0.012,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            self.apply_ui_accessibility(&mut ui_vertices, &mut ui_line_vertices, &mut []);
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len * std::mem::size_of::<UiVertex>())
                        + (ui_line_vert_len * std::mem::size_of::<UiVertex>()))
                        as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.connection_lost {
            let [mouse_x, mouse_y] = self.mouse_ndc;
            let button_hover = (-0.3..=0.3).contains(&mouse_x) && (-0.10..=0.00).contains(&mouse_y);

            add_ui_quad(
                &mut ui_vertices,
                -1.0,
                1.0,
                -1.0,
                1.0,
                [0.04, 0.02, 0.02, 0.82],
            );
            add_ui_quad(
                &mut ui_vertices,
                -0.3,
                0.3,
                -0.10,
                0.00,
                if button_hover {
                    [0.45, 0.18, 0.14, 1.0]
                } else {
                    [0.22, 0.08, 0.07, 1.0]
                },
            );
            add_ui_border(
                &mut ui_line_vertices,
                -0.3,
                0.3,
                -0.10,
                0.00,
                if button_hover {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.75, 0.35, 0.3, 1.0]
                },
            );

            let mut draw_centered =
                |text: &str, y: f32, char_w: f32, char_h: f32, spacing: f32, color: [f32; 4]| {
                    let text = text.to_uppercase();
                    let width = text.chars().count() as f32 * (char_w + spacing) - spacing;
                    add_string_lines(
                        &text,
                        -width / 2.0,
                        y,
                        char_w,
                        char_h,
                        spacing,
                        color,
                        &mut ui_line_vertices,
                    );
                };
            let connection_lost = self.translate("hud.connection_lost");
            draw_centered(
                &connection_lost,
                0.26,
                0.030,
                0.060,
                0.010,
                [1.0, 0.35, 0.28, 1.0],
            );
            if let Some(status) = &self.network_status {
                let reason: String = status
                    // `network_status` is an internal protocol/status string;
                    // keep its stable prefix independent from the localized
                    // heading rendered above.
                    .strip_prefix("CONNECTION LOST: ")
                    .unwrap_or(status)
                    .chars()
                    .take(64)
                    .collect();
                draw_centered(&reason, 0.12, 0.012, 0.024, 0.005, [0.92, 0.92, 0.92, 1.0]);
            }
            let return_to_menu = self.translate("hud.return_to_menu");
            draw_centered(
                &return_to_menu,
                -0.07,
                0.020,
                0.040,
                0.008,
                [1.0, 1.0, 1.0, 1.0],
            );

            self.apply_ui_accessibility(&mut ui_vertices, &mut ui_line_vertices, &mut []);
            let ui_vert_len = ui_vertices.len().min(UI_VERTEX_CAPACITY);
            let ui_line_vert_len = ui_line_vertices.len().min(UI_LINE_VERTEX_CAPACITY);
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len) * std::mem::size_of::<UiVertex>()) as u64,
                );
            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.player_state.is_dead {
            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Respawn button hover (X: [-0.3, 0.3], Y: [-0.1, 0.0])
            let respawn_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.1 && mouse_y <= 0.0;

            // Reddish overlay
            let bg_color = [0.4, 0.0, 0.0, 0.6];
            add_ui_quad(&mut ui_vertices, -1.0, 1.0, -1.0, 1.0, bg_color);

            // Button background
            let btn_bg = if respawn_hover {
                [0.4, 0.1, 0.1, 1.0]
            } else {
                [0.2, 0.0, 0.0, 1.0]
            };
            let btn_border = if respawn_hover {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.6, 0.2, 0.2, 1.0]
            };
            let btn_y_min = -0.10;
            let btn_y_max = 0.00;

            add_ui_quad(&mut ui_vertices, -0.3, 0.3, btn_y_min, btn_y_max, btn_bg);

            // Button border
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_border,
            });

            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            let you_died = self.translate("hud.you_died");
            draw_centered_text(
                &you_died,
                0.30,
                0.04,
                0.08,
                0.015,
                [1.0, 0.2, 0.2, 1.0],
                &mut ui_line_vertices,
            );

            let death_key = match self.player_state.death_reason {
                Some(DamageSource::Fall) => "death.fall",
                Some(DamageSource::Void) => "death.void",
                Some(DamageSource::Hunger) => "death.starved",
                Some(DamageSource::Mob) => "death.mob",
                Some(DamageSource::Explosion) => "death.explosion",
                Some(DamageSource::Drowning) => "death.drowned",
                Some(DamageSource::Lightning) => "death.lightning",
                None => "death.generic",
            };
            let msg = self.translate(death_key);
            draw_centered_text(
                &msg,
                0.15,
                0.015,
                0.03,
                0.006,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );
            let respawn = self.translate("hud.respawn");
            draw_centered_text(
                &respawn,
                -0.06,
                0.02,
                0.04,
                0.008,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );

            self.apply_ui_accessibility(&mut ui_vertices, &mut ui_line_vertices, &mut []);
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len) * std::mem::size_of::<UiVertex>()) as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.is_paused {
            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Hover states
            let resume_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.24 && mouse_y <= 0.34;
            let fov_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.10 && mouse_y <= 0.20;
            let sens_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.04 && mouse_y <= 0.06;
            let rd_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.18 && mouse_y <= -0.08;
            let vol_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.32 && mouse_y <= -0.22;
            let weather_vol_hover = point_in_bounds(mouse_x, mouse_y, PAUSE_WEATHER_VOLUME_BOUNDS);
            let quit_hover = point_in_bounds(mouse_x, mouse_y, PAUSE_QUIT_BOUNDS);

            // 1. Dark overlay (screen covers from -1.0 to 1.0)
            let bg_color = [0.1, 0.1, 0.1, 0.7];
            add_ui_quad(&mut ui_vertices, -1.0, 1.0, -1.0, 1.0, bg_color);

            // Button drawing helper
            let draw_button = |hover: bool,
                               y_min: f32,
                               y_max: f32,
                               ui_verts: &mut Vec<UiVertex>,
                               ui_line_verts: &mut Vec<UiVertex>| {
                let bg = if hover {
                    [0.4, 0.4, 0.4, 1.0]
                } else {
                    [0.2, 0.2, 0.2, 1.0]
                };
                let border = if hover {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.6, 0.6, 0.6, 1.0]
                };

                // Background (two triangles)
                ui_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: bg,
                });

                // Border (line loop)
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: border,
                });
            };

            // Draw Button backgrounds and borders
            draw_button(
                resume_hover,
                0.24,
                0.34,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                fov_hover,
                0.10,
                0.20,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                sens_hover,
                -0.04,
                0.06,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                rd_hover,
                -0.18,
                -0.08,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                vol_hover,
                -0.32,
                -0.22,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                weather_vol_hover,
                PAUSE_WEATHER_VOLUME_BOUNDS[2],
                PAUSE_WEATHER_VOLUME_BOUNDS[3],
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                quit_hover,
                PAUSE_QUIT_BOUNDS[2],
                PAUSE_QUIT_BOUNDS[3],
                &mut ui_vertices,
                &mut ui_line_vertices,
            );

            // Centered text drawing helper
            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            // Render Text Labels
            let text_color = [1.0, 1.0, 1.0, 1.0];
            // "GAME PAUSED"
            let game_paused = self.translate("hud.game_paused");
            draw_centered_text(
                &game_paused,
                0.40,
                0.03,
                0.06,
                0.012,
                text_color,
                &mut ui_line_vertices,
            );
            if let Some(status) = &self.network_status {
                draw_centered_text(
                    status,
                    0.52,
                    0.014,
                    0.028,
                    0.006,
                    [1.0, 0.45, 0.35, 1.0],
                    &mut ui_line_vertices,
                );
            }
            // "RESUME"
            let resume = self.translate("hud.resume");
            draw_centered_text(
                &resume,
                0.28,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "FOV < value >"
            let fov_value = format!("{:.0}", self.base_fov);
            let fov_text = self
                .translation_catalog
                .format_lookup("hud.fov", &[("value", &fov_value)]);
            draw_centered_text(
                &fov_text,
                0.14,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "SENS < value >"
            let sens_val = (self.sensitivity / 0.002 * 100.0).round();
            let sens_value = format!("{sens_val:.0}");
            let sens_text = self
                .translation_catalog
                .format_lookup("hud.sensitivity", &[("value", &sens_value)]);
            draw_centered_text(
                &sens_text,
                0.00,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "RENDER DISTANCE < value >"
            let rd_value = self.chunk_manager.render_distance.to_string();
            let rd_text = self
                .translation_catalog
                .format_lookup("hud.render_distance", &[("value", &rd_value)]);
            draw_centered_text(
                &rd_text,
                -0.14,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "MASTER VOLUME < value >"
            let vol_value = format!("{:.0}", self.settings.master_volume * 100.0);
            let vol_text = self
                .translation_catalog
                .format_lookup("hud.master_volume", &[("value", &vol_value)]);
            draw_centered_text(
                &vol_text,
                -0.28,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "WEATHER VOLUME < value >"
            let weather_value = format!("{:.0}", self.settings.weather_volume * 100.0);
            let weather_vol_text = self
                .translation_catalog
                .format_lookup("hud.weather_volume", &[("value", &weather_value)]);
            draw_centered_text(
                &weather_vol_text,
                -0.42,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "SAVE AND QUIT"
            let save_and_quit = self.translate("hud.save_and_quit");
            draw_centered_text(
                &save_and_quit,
                -0.56,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // Cap the sizes to the preallocated buffers (4096 vertices)
            self.apply_ui_accessibility(&mut ui_vertices, &mut ui_line_vertices, &mut []);
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );

            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len) * std::mem::size_of::<UiVertex>()) as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
        } else {
            let mut ui_textured_vertices = Vec::new();

            let aspect = self.size.width as f32 / self.size.height as f32;
            let slot_w = 0.08;
            let slot_h = 0.08 * aspect;
            let gap = 0.01;
            let start_x = -0.40;

            let draw_durability_bar =
                |stack: &ItemStack,
                 x0: f32,
                 x1: f32,
                 y0: f32,
                 y1: f32,
                 _aspect: f32,
                 ui_vertices: &mut Vec<UiVertex>| {
                    if let Some(tool_prop) = stack.item.tool_properties() {
                        let max_dur = tool_prop.durability;
                        if stack.durability < max_dur {
                            let ratio = (stack.durability as f32 / max_dur as f32).clamp(0.0, 1.0);

                            // Define bar bounds relative to slot size
                            let slot_w = x1 - x0;
                            let slot_h = y1 - y0;

                            let bar_x0 = x0 + slot_w * 0.15;
                            let bar_x1 = x1 - slot_w * 0.15;
                            let bar_y0 = y0 + slot_h * 0.10;
                            let bar_y1 = y0 + slot_h * 0.16;

                            // 1. Black background bar
                            let bg_color = [0.0, 0.0, 0.0, 1.0];
                            add_ui_quad(ui_vertices, bar_x0, bar_x1, bar_y0, bar_y1, bg_color);

                            // 2. Colored foreground bar
                            let fg_x1 = bar_x0 + (bar_x1 - bar_x0) * ratio;
                            let (r, g) = if ratio > 0.5 {
                                ((1.0 - ratio) * 2.0, 1.0)
                            } else {
                                (1.0, ratio * 2.0)
                            };
                            let fg_color = [r, g, 0.0, 1.0];

                            add_ui_quad(ui_vertices, bar_x0, fg_x1, bar_y0, bar_y1, fg_color);
                        }
                    }
                };

            if self.inventory.is_open {
                let creative_catalog = self.is_creative_catalog_open();
                // 1. Dark overlay (screen covers from -1.0 to 1.0)
                let bg_color = [0.08, 0.08, 0.08, 0.6];
                add_ui_quad(&mut ui_vertices, -1.0, 1.0, -1.0, 1.0, bg_color);

                if creative_catalog {
                    add_ui_quad(
                        &mut ui_vertices,
                        -0.49,
                        0.51,
                        -0.92,
                        0.92,
                        [0.10, 0.10, 0.10, 0.96],
                    );
                    add_ui_border(
                        &mut ui_line_vertices,
                        -0.49,
                        0.51,
                        -0.92,
                        0.92,
                        [0.52, 0.52, 0.52, 1.0],
                    );

                    for (index, tab) in CreativeTab::TABS.into_iter().enumerate() {
                        let rect = creative_tab_rect(index);
                        let hovered = rect.contains(self.mouse_ndc[0], self.mouse_ndc[1]);
                        let selected = tab == self.inventory.creative_tab;
                        add_ui_quad(
                            &mut ui_vertices,
                            rect.x0,
                            rect.x1,
                            rect.y0,
                            rect.y1,
                            if selected {
                                [0.30, 0.42, 0.22, 1.0]
                            } else if hovered {
                                [0.34, 0.34, 0.34, 1.0]
                            } else {
                                [0.18, 0.18, 0.18, 1.0]
                            },
                        );
                        add_ui_border(
                            &mut ui_line_vertices,
                            rect.x0,
                            rect.x1,
                            rect.y0,
                            rect.y1,
                            if selected || hovered {
                                [0.95, 0.95, 0.95, 1.0]
                            } else {
                                [0.42, 0.42, 0.42, 1.0]
                            },
                        );
                        let label = tab.label();
                        let char_w = 0.005;
                        let spacing = 0.0015;
                        let label_w = label.chars().count() as f32 * (char_w + spacing) - spacing;
                        add_string_lines(
                            label,
                            (rect.x0 + rect.x1 - label_w) * 0.5,
                            rect.y0 + 0.035,
                            char_w,
                            0.020,
                            spacing,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }

                    let track = creative_scroll_track_rect(aspect);
                    add_ui_quad(
                        &mut ui_vertices,
                        track.x0,
                        track.x1,
                        track.y0,
                        track.y1,
                        [0.035, 0.035, 0.035, 1.0],
                    );
                    add_ui_border(
                        &mut ui_line_vertices,
                        track.x0,
                        track.x1,
                        track.y0,
                        track.y1,
                        [0.34, 0.34, 0.34, 1.0],
                    );
                    let max_scroll = self.inventory.creative_max_scroll();
                    let total_rows = max_scroll + CREATIVE_ROWS;
                    let track_height = track.y1 - track.y0;
                    let thumb_height = if max_scroll == 0 {
                        track_height
                    } else {
                        (track_height * CREATIVE_ROWS as f32 / total_rows as f32).max(0.06)
                    };
                    let progress = if max_scroll == 0 {
                        0.0
                    } else {
                        self.inventory.creative_scroll_row as f32 / max_scroll as f32
                    };
                    let thumb_y1 = track.y1 - progress * (track_height - thumb_height);
                    add_ui_quad(
                        &mut ui_vertices,
                        track.x0 + 0.004,
                        track.x1 - 0.004,
                        thumb_y1 - thumb_height,
                        thumb_y1,
                        [0.68, 0.68, 0.68, 1.0],
                    );
                }

                // 2. Draw slots
                let slots = self.get_inventory_slots();
                let mouse_x = self.mouse_ndc[0];
                let mouse_y = self.mouse_ndc[1];
                let mut hovered_slot = None;

                for &(slot_type, x0, x1, y0, y1) in &slots {
                    let is_hovered =
                        mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1;
                    if is_hovered {
                        hovered_slot = Some((slot_type, x0, x1, y0, y1));
                    }

                    // Background Quad
                    let slot_bg_color = if is_hovered {
                        [0.35, 0.35, 0.35, 0.8]
                    } else {
                        [0.15, 0.15, 0.15, 0.8]
                    };
                    add_ui_quad(&mut ui_vertices, x0, x1, y0, y1, slot_bg_color);

                    // Borders
                    let border_color = match slot_type {
                        SlotType::Hotbar(idx) if idx == self.inventory.selected => {
                            [1.0, 1.0, 1.0, 1.0]
                        }
                        _ => [0.3, 0.3, 0.3, 0.8],
                    };
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });

                    // Slot Item
                    if let Some(stack) = self.get_item_at_slot(slot_type) {
                        let (col, row) = stack.item.properties().tex_coords;
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let margin_x = 0.015;
                        let margin_y = 0.015 * aspect;
                        let tx0 = x0 + margin_x;
                        let tx1 = x1 - margin_x;
                        let ty0 = y0 + margin_y;
                        let ty1 = y1 - margin_y;

                        let c = if stack.enchantments.is_empty() {
                            [1.0, 1.0, 1.0, 1.0]
                        } else {
                            let pulse = 0.72 + (self.total_time * 3.0).sin() * 0.18;
                            [0.82, pulse, 1.0, 1.0]
                        };
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.008;
                            let count_y = y0 + 0.01 * aspect;
                            add_string_lines(
                                &count_str,
                                count_x,
                                count_y,
                                cw,
                                ch,
                                cs,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }

                        // Draw durability bar
                        draw_durability_bar(&stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
                }

                // 3. Draw crafting arrow symbol
                if !creative_catalog && self.active_station.is_none() {
                    let arrow_y = if self.inventory.is_table_open {
                        -0.10 + 1.0 * (slot_h + gap) + slot_h / 2.0
                    } else {
                        -0.05 + 0.5 * (slot_h + gap) + slot_h / 2.0
                    };
                    let arrow_x = if self.inventory.is_table_open {
                        -0.05 + 3.0 * (slot_w + gap) + 0.015
                    } else {
                        0.05 + 2.0 * (slot_w + gap) + 0.015
                    };
                    let ac = [0.8, 0.8, 0.8, 1.0];
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.02, arrow_y + 0.01 * aspect, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.02, arrow_y - 0.01 * aspect, 0.0],
                        color: ac,
                    });
                }

                // 4. Draw texts (Labels)
                if creative_catalog {
                    let creative_inventory = self.translate("inventory.creative");
                    add_string_lines(
                        &creative_inventory,
                        -0.45,
                        0.70,
                        0.010,
                        0.020,
                        0.003,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                    let hotbar = self.translate("inventory.hotbar");
                    add_string_lines(
                        &hotbar,
                        -0.45,
                        -0.67,
                        0.008,
                        0.016,
                        0.003,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                } else {
                    let inventory = self.translate("inventory.inventory");
                    add_string_lines(
                        &inventory,
                        -0.40,
                        -0.70 + 3.0 * (slot_h + gap) + 0.02,
                        0.008,
                        0.016,
                        0.003,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                    if self.active_station.is_none() {
                        let craft_lbl_x = if self.inventory.is_table_open {
                            -0.05
                        } else {
                            0.05
                        };
                        let craft_lbl_y = if self.inventory.is_table_open {
                            -0.10 + 3.0 * (slot_h + gap) + 0.02
                        } else {
                            -0.05 + 2.0 * (slot_h + gap) + 0.02
                        };
                        let crafting = self.translate("inventory.crafting");
                        add_string_lines(
                            &crafting,
                            craft_lbl_x,
                            craft_lbl_y,
                            0.008,
                            0.016,
                            0.003,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                    if let Some(pos) = self.container_target {
                        let block = self.chunk_manager.get_block(pos.0, pos.1, pos.2);
                        if matches!(block, BlockType::Furnace | BlockType::FurnaceLit) {
                            let furnace = self.translate("inventory.furnace");
                            add_string_lines(
                                &furnace,
                                -0.15,
                                0.26,
                                0.010,
                                0.020,
                                0.003,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );

                            let (burn_time, burn_total, cook_progress, cook_total) = {
                                let (cx, cz) = (pos.0.div_euclid(16), pos.2.div_euclid(16));
                                let (bx, by, bz) = (
                                    pos.0.rem_euclid(16) as u8,
                                    pos.1 as i16,
                                    pos.2.rem_euclid(16) as u8,
                                );
                                self.chunk_manager
                                    .chunks
                                    .get(&(cx, cz))
                                    .and_then(|c| {
                                        if let Some(crate::block_entity::BlockEntity::Furnace(f)) =
                                            c.get_block_entity(bx, by, bz)
                                        {
                                            Some((
                                                f.burn_time,
                                                f.burn_total,
                                                f.cook_progress,
                                                f.cook_total,
                                            ))
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or((0, 0, 0, 200))
                            };

                            // Flame Fuel Progress Bar (vertical)
                            add_ui_quad(
                                &mut ui_vertices,
                                -0.14,
                                -0.09,
                                -0.01,
                                0.08,
                                [0.1, 0.1, 0.1, 0.8],
                            );
                            if burn_time > 0 && burn_total > 0 {
                                let fuel_ratio =
                                    (burn_time as f32 / burn_total as f32).clamp(0.0, 1.0);
                                let fill_h = 0.09 * fuel_ratio;
                                add_ui_quad(
                                    &mut ui_vertices,
                                    -0.14,
                                    -0.09,
                                    -0.01,
                                    -0.01 + fill_h,
                                    [1.0, 0.45, 0.0, 1.0],
                                );
                            }

                            // Cook Progress Bar (horizontal arrow)
                            add_ui_quad(
                                &mut ui_vertices,
                                -0.04,
                                0.12,
                                0.02,
                                0.06,
                                [0.1, 0.1, 0.1, 0.8],
                            );
                            if cook_progress > 0 && cook_total > 0 {
                                let cook_ratio =
                                    (cook_progress as f32 / cook_total as f32).clamp(0.0, 1.0);
                                let fill_w = 0.16 * cook_ratio;
                                add_ui_quad(
                                    &mut ui_vertices,
                                    -0.04,
                                    -0.04 + fill_w,
                                    0.02,
                                    0.06,
                                    [0.9, 0.75, 0.2, 1.0],
                                );
                            }
                        }
                    }
                }

                // Draw Recipe Book Toggle Button
                let book_btn_hover =
                    mouse_x >= -0.45 && mouse_x <= -0.37 && mouse_y >= 0.35 && mouse_y <= 0.43;
                add_ui_quad(
                    &mut ui_vertices,
                    -0.45,
                    -0.37,
                    0.35,
                    0.43,
                    if book_btn_hover {
                        [0.2, 0.6, 0.3, 1.0]
                    } else {
                        [0.15, 0.45, 0.2, 0.9]
                    },
                );
                let book = self.translate("inventory.book");
                add_string_lines(
                    &book,
                    -0.44,
                    0.40,
                    0.007,
                    0.014,
                    0.002,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );

                // Draw Recipe Book Side Panel if open
                if self.recipe_book_open {
                    add_ui_quad(
                        &mut ui_vertices,
                        -0.85,
                        -0.48,
                        -0.45,
                        0.45,
                        [0.12, 0.12, 0.12, 0.95],
                    );
                    let recipes = self.translate("inventory.recipes");
                    add_string_lines(
                        &recipes,
                        -0.82,
                        0.40,
                        0.008,
                        0.016,
                        0.003,
                        [0.4, 0.9, 0.4, 1.0],
                        &mut ui_line_vertices,
                    );

                    let smelting_recipes = self.recipe_manager.get_smelting_recipes();
                    let mut line_y = 0.34;
                    for r in smelting_recipes {
                        let entry_hover = mouse_x >= -0.83
                            && mouse_x <= -0.50
                            && mouse_y >= line_y - 0.05
                            && mouse_y <= line_y + 0.02;
                        if entry_hover {
                            add_ui_quad(
                                &mut ui_vertices,
                                -0.83,
                                -0.50,
                                line_y - 0.05,
                                line_y + 0.02,
                                [0.25, 0.35, 0.25, 0.8],
                            );
                        }
                        let text = format!(
                            "{} -> {}",
                            self.localized_item_name(r.input),
                            self.localized_item_name(r.output.item)
                        );
                        add_string_lines(
                            &text,
                            -0.82,
                            line_y,
                            0.006,
                            0.012,
                            0.002,
                            [0.9, 0.9, 0.9, 1.0],
                            &mut ui_line_vertices,
                        );
                        line_y -= 0.07;
                        if line_y < -0.40 {
                            break;
                        }
                    }
                }

                match self.active_station {
                    Some(StationKind::Enchanting) => {
                        let enchanting = self.translate("station.enchanting");
                        add_string_lines(
                            &enchanting,
                            -0.18,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.75, 0.45, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                        let level = self.player_state.experience_level.to_string();
                        let bookshelves = self.enchanting.bookshelves.to_string();
                        let level_text = self.translation_catalog.format_lookup(
                            "station.level_bookshelves",
                            &[("level", &level), ("bookshelves", &bookshelves)],
                        );
                        add_string_lines(
                            &level_text,
                            -0.18,
                            0.31,
                            0.008,
                            0.016,
                            0.003,
                            [0.5, 1.0, 0.5, 1.0],
                            &mut ui_line_vertices,
                        );
                        for (index, option) in self.enchanting.options.iter().enumerate() {
                            let y1 = 0.28 - index as f32 * 0.12;
                            let y0 = y1 - 0.09;
                            let hovered = mouse_x >= 0.02
                                && mouse_x <= 0.62
                                && mouse_y >= y0
                                && mouse_y <= y1;
                            add_ui_quad(
                                &mut ui_vertices,
                                0.02,
                                0.62,
                                y0,
                                y1,
                                if hovered {
                                    [0.30, 0.16, 0.42, 0.95]
                                } else {
                                    [0.14, 0.07, 0.20, 0.95]
                                },
                            );
                            let enchantment =
                                option.enchantments.entries.iter().flatten().next().copied();
                            let label = enchantment
                                .map(|e| {
                                    let enchantment = format!("{} {}", e.short_name(), e.level());
                                    let cost = option.cost.to_string();
                                    let lapis = option.lapis_cost.to_string();
                                    self.translation_catalog.format_lookup(
                                        "station.cost_lapis",
                                        &[
                                            ("enchantment", &enchantment),
                                            ("cost", &cost),
                                            ("lapis", &lapis),
                                        ],
                                    )
                                })
                                .unwrap_or_else(|| self.translate("station.no_enchantment"));
                            add_string_lines(
                                &label,
                                0.04,
                                y0 + 0.032,
                                0.007,
                                0.014,
                                0.002,
                                [0.8, 0.65, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }
                    }
                    Some(StationKind::Brewing) => {
                        let brewing_stand = self.translate("station.brewing_stand");
                        add_string_lines(
                            &brewing_stand,
                            -0.18,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.8, 0.6, 0.3, 1.0],
                            &mut ui_line_vertices,
                        );
                        let progress = (self.brewing.progress / 10.0).clamp(0.0, 1.0);
                        add_ui_quad(
                            &mut ui_vertices,
                            0.04,
                            0.54,
                            0.20,
                            0.24,
                            [0.05, 0.05, 0.05, 1.0],
                        );
                        add_ui_quad(
                            &mut ui_vertices,
                            0.04,
                            0.04 + 0.5 * progress,
                            0.20,
                            0.24,
                            [0.85, 0.45, 0.1, 1.0],
                        );
                        let status = if self.brewing.can_brew() {
                            let value = format!("{:.0}", progress * 100.0);
                            self.translation_catalog
                                .format_lookup("station.brewing_progress", &[("value", &value)])
                        } else {
                            self.translate("station.add_bottles_ingredient")
                        };
                        add_string_lines(
                            &status,
                            0.04,
                            0.28,
                            0.008,
                            0.016,
                            0.003,
                            [1.0, 0.85, 0.55, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                    Some(StationKind::Anvil) => {
                        let anvil = self.translate("station.anvil");
                        add_string_lines(
                            &anvil,
                            -0.20,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.8, 0.8, 0.8, 1.0],
                            &mut ui_line_vertices,
                        );
                        add_ui_quad(
                            &mut ui_vertices,
                            -0.20,
                            0.45,
                            0.25,
                            0.31,
                            [0.04, 0.04, 0.04, 0.95],
                        );
                        let rename = if self.anvil.rename.is_empty() {
                            self.translate("station.type_a_name")
                        } else {
                            self.anvil.rename.clone()
                        };
                        add_string_lines(
                            &rename,
                            -0.18,
                            0.27,
                            0.009,
                            0.018,
                            0.003,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                        let cost_value = self.anvil.cost.to_string();
                        let cost = self
                            .translation_catalog
                            .format_lookup("station.cost_levels", &[("cost", &cost_value)]);
                        add_string_lines(
                            &cost,
                            0.20,
                            0.05,
                            0.008,
                            0.016,
                            0.003,
                            [0.5, 1.0, 0.5, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                    Some(StationKind::Merchant) => {
                        let profession = self.active_merchant_profession.display_name().to_string();
                        let level = (self.active_merchant_level as u8).to_string();
                        let title = self.translation_catalog.format_lookup(
                            "station.villager_trading",
                            &[("profession", &profession), ("level", &level)],
                        );
                        add_string_lines(
                            &title,
                            -0.35,
                            0.38,
                            0.010,
                            0.020,
                            0.003,
                            [0.3, 0.9, 0.4, 1.0],
                            &mut ui_line_vertices,
                        );

                        let mut offer_y = 0.28;
                        let discount = if self.player_state.hero_of_the_village_timer > 0.0 {
                            0.3
                        } else {
                            0.0
                        };
                        for (idx, offer) in self.active_merchant_offers.iter().enumerate() {
                            let btn_hover = mouse_x >= -0.35
                                && mouse_x <= 0.35
                                && mouse_y >= offer_y - 0.04
                                && mouse_y <= offer_y + 0.03;
                            let cost_a = offer.effective_cost_a(discount);
                            let index = (idx + 1).to_string();
                            let buy_count = cost_a.to_string();
                            let buy_item = format!("{:?}", offer.buy_a.item);
                            let sell_count = offer.sell.count.to_string();
                            let sell_item = format!("{:?}", offer.sell.item);
                            let text = self.translation_catalog.format_lookup(
                                "station.trade",
                                &[
                                    ("index", &index),
                                    ("buy_count", &buy_count),
                                    ("buy_item", &buy_item),
                                    ("sell_count", &sell_count),
                                    ("sell_item", &sell_item),
                                ],
                            );

                            add_ui_quad(
                                &mut ui_vertices,
                                -0.36,
                                0.36,
                                offer_y - 0.04,
                                offer_y + 0.03,
                                if offer.is_out_of_stock() {
                                    [0.2, 0.1, 0.1, 0.7]
                                } else if btn_hover {
                                    [0.2, 0.5, 0.3, 0.9]
                                } else {
                                    [0.15, 0.25, 0.18, 0.85]
                                },
                            );

                            add_string_lines(
                                &text,
                                -0.34,
                                offer_y,
                                0.007,
                                0.014,
                                0.002,
                                if offer.is_out_of_stock() {
                                    [0.6, 0.6, 0.6, 1.0]
                                } else {
                                    [1.0, 1.0, 1.0, 1.0]
                                },
                                &mut ui_line_vertices,
                            );

                            offer_y -= 0.09;
                            if offer_y < -0.30 {
                                break;
                            }
                        }
                    }
                    None => {}
                }

                // 5. Draw dragged item at cursor position
                if let Some(dragged) = self.inventory.dragged {
                    let (cursor_slot_w, cursor_slot_h) = if creative_catalog {
                        let (width, height, _, _) = creative_slot_metrics(aspect);
                        (width, height)
                    } else {
                        (slot_w, slot_h)
                    };
                    let (col, row) = dragged.item.properties().tex_coords;
                    let u0 = col as f32 * 0.0625;
                    let u1 = (col + 1) as f32 * 0.0625;
                    let v0 = row as f32 * 0.0625;
                    let v1 = (row + 1) as f32 * 0.0625;

                    let dx0 = mouse_x - cursor_slot_w / 2.0 + 0.015;
                    let dx1 = mouse_x + cursor_slot_w / 2.0 - 0.015;
                    let dy0 = mouse_y - cursor_slot_h / 2.0 + 0.015 * aspect;
                    let dy1 = mouse_y + cursor_slot_h / 2.0 - 0.015 * aspect;

                    let c = if dragged.enchantments.is_empty() {
                        [1.0, 1.0, 1.0, 1.0]
                    } else {
                        [0.82, 0.65 + (self.total_time * 3.0).sin() * 0.18, 1.0, 1.0]
                    };
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy1, 0.0],
                        tex_coords: [u0, v0],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy0, 0.0],
                        tex_coords: [u0, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy0, 0.0],
                        tex_coords: [u1, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy1, 0.0],
                        tex_coords: [u0, v0],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy0, 0.0],
                        tex_coords: [u1, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy1, 0.0],
                        tex_coords: [u1, v0],
                        color: c,
                    });

                    if dragged.count > 1 {
                        let count_str = format!("{}", dragged.count);
                        let cw = 0.008;
                        let ch = 0.016;
                        let cs = 0.003;
                        let n_chars = count_str.len() as f32;
                        let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                        let count_x = mouse_x + cursor_slot_w / 2.0 - count_w - 0.008;
                        let count_y = mouse_y - cursor_slot_h / 2.0 + 0.01 * aspect;
                        add_string_lines(
                            &count_str,
                            count_x,
                            count_y,
                            cw,
                            ch,
                            cs,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                }

                // 6. Draw tooltip for hovered slot
                if self.inventory.dragged.is_none() {
                    if let Some((slot_type, _, _, _, _)) = hovered_slot {
                        if let Some(stack) = self.get_item_at_slot(slot_type) {
                            let name = if !stack.custom_name.is_empty() {
                                stack.custom_name.as_str().to_string()
                            } else if let Some(potion) = stack.potion {
                                potion.display_name().to_string()
                            } else {
                                self.localized_item_name(stack.item)
                            };
                            let tw = name.len() as f32 * 0.014 + 0.02;
                            let th = 0.035 * aspect;
                            let tx = mouse_x + 0.02;
                            let ty = mouse_y + 0.02;

                            // The tooltip background must sit above the slot
                            // icons but below the text. The colored-UI pass
                            // now runs before the icons, so this quad goes
                            // through the textured pass instead, using the
                            // pure white atlas tile (col 15, row 8) tinted to
                            // the tooltip color.
                            let tt_bg = [0.05, 0.05, 0.1, 0.95];
                            let (w0u, w0v) = (15.0 * 0.0625, 8.0 * 0.0625);
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [tx, ty + th, 0.0],
                                tex_coords: [w0u, w0v],
                                color: tt_bg,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [tx, ty, 0.0],
                                tex_coords: [w0u, w0v + 0.0625],
                                color: tt_bg,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [tx + tw, ty, 0.0],
                                tex_coords: [w0u + 0.0625, w0v + 0.0625],
                                color: tt_bg,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [tx, ty + th, 0.0],
                                tex_coords: [w0u, w0v],
                                color: tt_bg,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [tx + tw, ty, 0.0],
                                tex_coords: [w0u + 0.0625, w0v + 0.0625],
                                color: tt_bg,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                tex_coords: [w0u + 0.0625, w0v],
                                color: tt_bg,
                            });

                            let tt_border = [0.3, 0.3, 0.7, 1.0];
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_border,
                            });

                            add_string_lines(
                                &name,
                                tx + 0.01,
                                ty + 0.01 * aspect,
                                0.008,
                                0.016,
                                0.003,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }
                    }
                }
            } else {
                // Background Bar
                let bg_color = [0.05, 0.05, 0.05, 0.6];
                let bg_x0 = -0.415;
                let bg_x1 = 0.415;
                let bg_y0 = -0.96;
                let bg_y1 = -0.94 + slot_h;
                add_ui_quad(&mut ui_vertices, bg_x0, bg_x1, bg_y0, bg_y1, bg_color);

                // Slots
                for i in 0..9 {
                    let x0 = start_x + i as f32 * (slot_w + gap);
                    let x1 = x0 + slot_w;
                    let y0 = -0.95;
                    let y1 = y0 + slot_h;

                    let border_color = if i == self.inventory.selected {
                        [1.0, 1.0, 1.0, 1.0] // White for active
                    } else {
                        [0.3, 0.3, 0.3, 0.8] // Gray for inactive
                    };

                    // Push lines to ui_line_vertices (forms border box)
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });

                    if let Some(stack) = &self.inventory.hotbar[i] {
                        let (col, row) = stack.item.properties().tex_coords;
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let margin_x = 0.015;
                        let margin_y = 0.015 * aspect;
                        let tx0 = x0 + margin_x;
                        let tx1 = x1 - margin_x;
                        let ty0 = y0 + margin_y;
                        let ty1 = y1 - margin_y;

                        let c = if stack.enchantments.is_empty() {
                            [1.0, 1.0, 1.0, 1.0]
                        } else {
                            [0.82, 0.65 + (self.total_time * 3.0).sin() * 0.18, 1.0, 1.0]
                        };
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.01;
                            let count_y = y0 + 0.012 * aspect;
                            add_string_lines(
                                &count_str,
                                count_x,
                                count_y,
                                cw,
                                ch,
                                cs,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }

                        // Draw durability bar
                        draw_durability_bar(stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
                }

                if self.game_mode_policy().can_take_damage {
                    // Draw Health HUD
                    let hud_w = 0.03;
                    let hud_h = 0.03 * aspect;
                    let hud_gap = 0.005;
                    let x_hearts_start = -0.38;
                    let y_hud = -0.76;

                    for i in 0..10 {
                        let h_val = self.player_state.health;
                        let (col, row) = if h_val >= 2.0 * (i + 1) as f32 {
                            (0, 8) // Full
                        } else if h_val >= 2.0 * i as f32 + 1.0 {
                            (1, 8) // Half
                        } else {
                            (2, 8) // Empty
                        };

                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let hx0 = x_hearts_start + i as f32 * (hud_w + hud_gap);
                        let hx1 = hx0 + hud_w;
                        let hy0 = y_hud;
                        let hy1 = hy0 + hud_h;

                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });
                    }

                    // Draw Hunger HUD
                    let x_hunger_start = 0.38 - 10.0 * hud_w - 9.0 * hud_gap;
                    for i in 0..10 {
                        let hung_val = self.player_state.hunger;
                        let (col, row) = if hung_val >= 2.0 * (i + 1) as f32 {
                            (3, 8) // Full
                        } else if hung_val >= 2.0 * i as f32 + 1.0 {
                            (4, 8) // Half
                        } else {
                            (5, 8) // Empty
                        };

                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let hx0 = x_hunger_start + i as f32 * (hud_w + hud_gap);
                        let hx1 = hx0 + hud_w;
                        let hy0 = y_hud;
                        let hy1 = hy0 + hud_h;

                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });
                    }

                    // Draw Oxygen HUD
                    if self.player_state.oxygen < 300.0 {
                        let oxygen = self.player_state.oxygen;
                        let bubble_count = (oxygen / 30.0).ceil() as i32;
                        let y_bubbles = y_hud + hud_h + 0.005;

                        for i in 0..bubble_count {
                            let col = 15;
                            let row = 3;
                            let u0 = col as f32 * 0.0625;
                            let u1 = (col + 1) as f32 * 0.0625;
                            let v0 = row as f32 * 0.0625;
                            let v1 = (row + 1) as f32 * 0.0625;

                            let slot_idx = 9 - i;
                            let hx0 = x_hunger_start + slot_idx as f32 * (hud_w + hud_gap);
                            let hx1 = hx0 + hud_w;
                            let hy0 = y_bubbles;
                            let hy1 = hy0 + hud_h;

                            let c = [1.0, 1.0, 1.0, 1.0];
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy1, 0.0],
                                tex_coords: [u0, v0],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy0, 0.0],
                                tex_coords: [u0, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy0, 0.0],
                                tex_coords: [u1, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy1, 0.0],
                                tex_coords: [u0, v0],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy0, 0.0],
                                tex_coords: [u1, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy1, 0.0],
                                tex_coords: [u1, v0],
                                color: c,
                            });
                        }
                    }
                }

                // Selected Block/Item Text
                let selected_item = self.inventory.hotbar[self.inventory.selected]
                    .map(|s| s.item)
                    .unwrap_or(crate::inventory::Item::Air);
                let selected_text = if let Some(target) = self.mining_target {
                    let block = self.chunk_manager.get_block(
                        target.x as i32,
                        target.y as i32,
                        target.z as i32,
                    );
                    format!(
                        "{} / {}",
                        self.localized_block_name(block),
                        self.localized_item_name(selected_item)
                    )
                } else {
                    self.localized_item_name(selected_item)
                }
                .to_uppercase();
                let char_w = 0.010;
                let char_h = 0.020;
                let spacing = 0.004;
                let n = selected_text.len() as f32;
                let width = n * char_w + (n - 1.0) * spacing;
                let text_x = -width / 2.0;
                add_string_lines(
                    &selected_text,
                    text_x,
                    -0.78,
                    char_w,
                    char_h,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );

                // Game Mode Status Text
                let mode_text = match (self.game_mode, self.player_physics.is_flying()) {
                    (GameMode::Creative, true) => "CREATIVE MODE - FLYING",
                    (GameMode::Creative, false) => "CREATIVE MODE",
                    (GameMode::Survival, _) => "SURVIVAL MODE",
                    (GameMode::Adventure, _) => "ADVENTURE MODE",
                    (GameMode::Spectator, _) => "SPECTATOR MODE",
                };
                let mode_w = 0.009;
                let mode_h = 0.018;
                let mode_s = 0.003;
                let n_mode = mode_text.len() as f32;
                let width_mode = n_mode * mode_w + (n_mode - 1.0) * mode_s;
                let mode_x = -width_mode / 2.0;
                add_string_lines(
                    mode_text,
                    mode_x,
                    -0.71,
                    mode_w,
                    mode_h,
                    mode_s,
                    [1.0, 0.9, 0.4, 1.0],
                    &mut ui_line_vertices,
                );

                if self.game_mode_policy().can_take_damage {
                    let xp_text = format!("LEVEL {}", self.player_state.experience_level);
                    let width = xp_text.len() as f32 * 0.009;
                    add_string_lines(
                        &xp_text,
                        -width / 2.0,
                        -0.66,
                        0.009,
                        0.018,
                        0.003,
                        [0.35, 1.0, 0.25, 1.0],
                        &mut ui_line_vertices,
                    );
                }

                for (index, effect) in self.potion_effects.active.iter().enumerate() {
                    let seconds = effect.remaining().ceil() as u32;
                    let text = format!("{} {}:{:02}", effect.name(), seconds / 60, seconds % 60);
                    add_string_lines(
                        &text,
                        0.54,
                        0.86 - index as f32 * 0.05,
                        0.007,
                        0.014,
                        0.002,
                        [0.75, 0.55, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                }

                // Damaged screen red flash overlay
                if self.player_state.damaged_flash_time > 0.0 {
                    let base = (self.player_state.damaged_flash_time / 0.5).min(1.0) * 0.25;
                    let alpha = crate::accessibility::damage_overlay_alpha(
                        base,
                        self.settings.accessibility.damage_tilt,
                        self.settings.accessibility.reduce_flashing,
                    );
                    let flash_color = [1.0, 0.0, 0.0, alpha];
                    let tilt = crate::accessibility::damage_tilt_angle(
                        base / 0.25,
                        self.settings.accessibility.damage_tilt,
                        self.settings.accessibility.reduce_flashing,
                    );
                    for position in [
                        [-1.0, 1.0],
                        [-1.0, -1.0],
                        [1.0, -1.0],
                        [-1.0, 1.0],
                        [1.0, -1.0],
                        [1.0, 1.0],
                    ] {
                        let rotated = crate::accessibility::rotate_ndc(position, tilt);
                        ui_vertices.push(UiVertex {
                            position: [rotated[0], rotated[1], 0.0],
                            color: flash_color,
                        });
                    }
                }

                let lightning_flash = self.weather.flash_intensity();
                if lightning_flash > 0.0 {
                    let flash_alpha = crate::accessibility::reduced_flash_alpha(
                        lightning_flash * 0.82,
                        self.settings.accessibility.reduce_flashing,
                    );
                    let flash_color = [1.0, 1.0, 1.0, flash_alpha];
                    for position in [
                        [-1.0, 1.0, 0.0],
                        [-1.0, -1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [-1.0, 1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [1.0, 1.0, 0.0],
                    ] {
                        ui_vertices.push(UiVertex {
                            position,
                            color: flash_color,
                        });
                    }
                }

                // Dragon completion has a short presentation flash analogous
                // to the End portal effect.  It is driven by a transient
                // State timer only; no tick, damage or authority value uses
                // this field.
                if self.end_flash_time > 0.0 {
                    let alpha = crate::accessibility::reduced_flash_alpha(
                        (self.end_flash_time / 0.45).clamp(0.0, 1.0) * 0.55,
                        self.settings.accessibility.reduce_flashing,
                    );
                    let flash_color = [0.72, 0.52, 1.0, alpha];
                    for position in [
                        [-1.0, 1.0, 0.0],
                        [-1.0, -1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [-1.0, 1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [1.0, 1.0, 0.0],
                    ] {
                        ui_vertices.push(UiVertex {
                            position,
                            color: flash_color,
                        });
                    }
                }

                // F3 Debug Screen
                if self.show_debug {
                    use std::fmt::Write;

                    let char_w = 0.007;
                    let char_h = 0.014;
                    let spacing = 0.002;
                    let start_x = -0.98;
                    let mut line_y = 0.95;
                    let line_gap = 0.025;

                    let mut render_line = |s: &str, color: [f32; 4], verts: &mut Vec<UiVertex>| {
                        add_string_lines(s, start_x, line_y, char_w, char_h, spacing, color, verts);
                        line_y -= line_gap;
                    };

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "FPS: {:.1} / FRAME: {:.2} MS",
                        self.debug_fps, self.debug_frame_ms
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let pos = self.player_physics.position;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "XYZ: {:.3} / {:.3} / {:.3}",
                        pos.x, pos.y, pos.z
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "AUTOMATION: MOVES {} / CHECKS {} / PULSES {} / BUDGET {} / REDSTONE Q {} / OBSERVER {}",
                        self.perf_counters.hopper_transfers,
                        self.perf_counters.hopper_container_checks,
                        self.perf_counters.observer_pulses,
                        self.perf_counters.hopper_budget_exhausted,
                        self.perf_counters.redstone_scheduled_backlog,
                        self.perf_counters.observer_pending_pulses
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "FACING: YAW {:.2} / PITCH {:.2}",
                        self.camera.yaw.to_degrees().rem_euclid(360.0),
                        self.camera.pitch.to_degrees()
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let chunk_x = debug_chunk_coordinate(pos.x, CHUNK_WIDTH);
                    let chunk_z = debug_chunk_coordinate(pos.z, CHUNK_DEPTH);
                    self.debug_str_scratch.clear();
                    let _ = write!(self.debug_str_scratch, "CHUNK: {} / {}", chunk_x, chunk_z);
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "BIOME: (presentation has no climate)"
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "WEATHER: {:?}",
                        self.weather.current
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "CHUNKS: {} VISIBLE / {} OCCLUDED / {} LOADED / {} DRAWS",
                        self.visible_chunk_count,
                        self.perf_counters.occluded_chunks,
                        self.chunk_manager.chunks.len(),
                        self.submitted_terrain_draw_calls
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "ENTITIES: {} ({} RENDERED, {} FRUSTUM, {} OCCLUSION) / PARTICLES: {}",
                        self.entity_manager.entities.len(),
                        self.perf_counters.rendered_entities,
                        self.perf_counters.frustum_culled_entities,
                        self.perf_counters.occlusion_culled_entities,
                        self.particles.particles.len()
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let culling = self.entity_los_manager.counters;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "CULL: DIST {} / FRUST {} / SEC {} / LOS {} / FAIL-OPEN {} / STALE {} / TIMEOUT {} / OVERFLOW {}",
                        culling.distance,
                        culling.frustum,
                        culling.section,
                        culling.los,
                        culling.fail_open,
                        culling.stale,
                        culling.timeouts,
                        culling.overflow
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let terrain_indices = self.submitted_terrain_triangles.saturating_mul(3);
                    let rendered_indices = terrain_indices
                        + u64::from(self.mob_num_indices)
                        + u64::from(self.particle_num_indices);
                    let rendered_triangles = rendered_indices / 3;
                    let rendered_vertices = rendered_indices * 2 / 3;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "RENDER: {} VERTICES / {} TRIANGLES / {} DRAWS",
                        rendered_vertices, rendered_triangles, self.perf_counters.draw_calls
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "FRAME ALLOCS: {}",
                        self.perf_counters.frame_allocations
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "MEMORY TRACKED: {:.1} MB",
                        self.estimated_debug_memory_bytes() as f64 / (1024.0 * 1024.0)
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "GPU MESH: {:.1} MB / {} BUFFERS / UPLOAD: {:.1} KB",
                        self.perf_counters.gpu_mesh_bytes as f64 / (1024.0 * 1024.0),
                        self.perf_counters.gpu_buffer_objects,
                        self.perf_counters.upload_bytes_frame as f64 / 1024.0
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "WORKERS: {} IN FLIGHT / {} STALE / {} CANCELLED",
                        self.perf_counters.in_flight,
                        self.perf_counters.stale_results,
                        self.perf_counters.cancelled
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "NET Q: {} | NET FULL: {}",
                        self.perf_counters.network_queue_depth,
                        self.perf_counters.network_catchup_mailbox_full
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    if self.perf_counters.gpu_timestamps_supported
                        && self.perf_counters.gpu_timestamps_inside_passes
                        && self.gpu_pass_timings_valid
                    {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: SKY {:.2}MS | OPAQUE {:.2}MS | MOBS {:.2}MS | TRANS {:.2}MS | PART {:.2}MS | CRACK {:.2}MS | UI {:.2}MS",
                            self.perf_counters.gpu_sky_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_opaque_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_mobs_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_translucent_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_particles_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_crack_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_ui_ns as f64 / 1_000_000.0,
                        );
                    } else if self.perf_counters.gpu_timestamps_supported
                        && self.perf_counters.gpu_timestamps_inside_passes
                    {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: N/A (WAITING FOR FIRST VALID TIMESTAMP SAMPLE)"
                        );
                    } else if self.perf_counters.gpu_timestamps_supported {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: N/A (TIMESTAMP_QUERY_INSIDE_PASSES UNSUPPORTED)"
                        );
                    } else {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: TIMESTAMP QUERY NOT SUPPORTED"
                        );
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let time_of_day = self.world_time.time_of_day_smooth();
                    let hour = ((time_of_day * 24.0 + 6.0) % 24.0).floor() as u32;
                    let minute = (((time_of_day * 24.0 + 6.0) % 1.0) * 60.0).floor() as u32;
                    let day = self.world_time.ticks / self.world_time.day_length;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "TIME: {:02}:{:02} / DAY: {} / TICKS: {}",
                        hour, minute, day, self.world_time.ticks
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    match &self.role {
                        MultiplayerRole::Host { port } => {
                            let _ = write!(
                                self.debug_str_scratch,
                                "NET: HOST ON PORT {} | CLIENTS: {}",
                                port,
                                self.remote_players.len()
                            );
                        }
                        MultiplayerRole::Client {
                            server_addr, port, ..
                        } => {
                            let _ = write!(
                                self.debug_str_scratch,
                                "NET: CLIENT @ {}:{} | LOCAL ID: {} | PLAYERS: {}",
                                server_addr,
                                port,
                                self.local_player_id
                                    .map(|id| id.to_string())
                                    .unwrap_or_else(|| "?".to_string()),
                                self.remote_players.len() + 1
                            );
                        }
                        MultiplayerRole::Singleplayer => {
                            let _ = write!(self.debug_str_scratch, "NET: SINGLEPLAYER");
                        }
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    for summary in self.perf_summaries.iter() {
                        let name_label = if summary.name == "lighting" {
                            "LIGHTING (LOAD+MUTATION)"
                        } else {
                            summary.name
                        };
                        self.debug_str_scratch.clear();
                        let _ = write!(
                            self.debug_str_scratch,
                            "CPU {}: AVG {:.3} / P95 {:.3} / P99 {:.3} MS / N {}",
                            name_label,
                            summary.average() as f64 / 1_000_000.0,
                            summary.p95() as f64 / 1_000_000.0,
                            summary.p99() as f64 / 1_000_000.0,
                            summary.sample_count(),
                        );
                        render_line(
                            &self.debug_str_scratch,
                            [0.82, 0.94, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }

                    // Queue telemetry is sampled once per frame and retained in the
                    // bounded 240-frame ring; show every queue family plus p95/p99.
                    if let Some(latest) = self.frame_perf_samples.back() {
                        let categories = latest.queues.categories.clone();
                        for category in crate::perf::QueueCategory::ALL {
                            let name = match category {
                                crate::perf::QueueCategory::Inbound => "IN",
                                crate::perf::QueueCategory::Outbound => "OUT",
                                crate::perf::QueueCategory::Reliable => "REL",
                                crate::perf::QueueCategory::CatchUp => "CATCH",
                                crate::perf::QueueCategory::SaveProducer => "SAVE-P",
                                crate::perf::QueueCategory::SaveWorker => "SAVE-W",
                            };
                            let sample = categories.get(&category).cloned().unwrap_or_default();
                            let p95 = crate::perf::frame_percentile(
                                &self.frame_perf_samples,
                                95,
                                |frame| {
                                    frame
                                        .queues
                                        .categories
                                        .get(&category)
                                        .map_or(0, |queue| queue.depth)
                                },
                            );
                            let p99 = crate::perf::frame_percentile(
                                &self.frame_perf_samples,
                                99,
                                |frame| {
                                    frame
                                        .queues
                                        .categories
                                        .get(&category)
                                        .map_or(0, |queue| queue.depth)
                                },
                            );
                            self.debug_str_scratch.clear();
                            let _ = write!(
                                self.debug_str_scratch,
                                "Q {} D:{} B:{} DROP:{} RETRY:{} CANCEL:{} AGE:{}ms P95:{} P99:{}",
                                name,
                                sample.depth,
                                sample.bytes,
                                sample.drops,
                                sample.retries,
                                sample.cancels,
                                sample.oldest_age_ms,
                                p95,
                                p99
                            );
                            render_line(
                                &self.debug_str_scratch,
                                [0.82, 0.94, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }
                    }

                    self.debug_str_scratch.clear();
                    let _ = write!(self.debug_str_scratch, "LIGHT SRC:");
                    for source in crate::perf::LightingSource::ALL {
                        let ms = self.lighting_scopes_frame.get(source as usize).unwrap_or(0)
                            as f64
                            / 1_000_000.0;
                        let _ = write!(self.debug_str_scratch, " {} {:.3}ms", source.name(), ms);
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [0.82, 0.94, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(self.debug_str_scratch, "UPLOAD SRC:");
                    for source in crate::perf::UploadSource::ALL {
                        let ms = self
                            .gpu_upload_scopes_frame
                            .get(source as usize)
                            .unwrap_or(0) as f64
                            / 1_000_000.0;
                        let _ = write!(self.debug_str_scratch, " {} {:.3}ms", source.name(), ms);
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [0.82, 0.94, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                }
            }

            // Remote-player name tags use the same vector-line UI as the rest
            // of the HUD. Project the point above each avatar into NDC, then
            // keep the label readable at the horizontal screen edge.
            let view_proj = self.camera.build_view_projection_matrix(
                aspect,
                crate::camera::render_far_plane(
                    self.chunk_manager.render_distance as u32,
                    self.chunk_manager.dimension.height().height(),
                ),
            );
            for remote in self.remote_players.values() {
                if remote.username.trim().is_empty() {
                    continue;
                }
                let Some(entity) = self.entity_manager.get_by_id(remote.entity_id) else {
                    continue;
                };
                if entity.position.distance_squared(self.camera.position) > 96.0 * 96.0 {
                    continue;
                }
                let Some(projected) =
                    project_name_tag(entity.position + Vec3::new(0.0, 2.05, 0.0), view_proj)
                else {
                    continue;
                };
                let label: String = remote.username.to_uppercase().chars().take(24).collect();
                let char_w = 0.009;
                let char_h = 0.018;
                let spacing = 0.003;
                let width = label.chars().count() as f32 * (char_w + spacing) - spacing;
                let center_x = projected.x.clamp(-0.98 + width / 2.0, 0.98 - width / 2.0);
                let y = (projected.y + 0.025).clamp(-0.94, 0.94);
                add_ui_quad(
                    &mut ui_vertices,
                    center_x - width / 2.0 - 0.012,
                    center_x + width / 2.0 + 0.012,
                    y - 0.007,
                    y + char_h + 0.007,
                    [0.02, 0.02, 0.02, 0.68],
                );
                add_string_lines(
                    &label,
                    center_x - width / 2.0,
                    y,
                    char_w,
                    char_h,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            // Chat history is deliberately a compact ring buffer. The newest
            // line sits closest to the input box at the lower-left.
            let visible_messages: Vec<_> = self
                .chat_messages
                .iter()
                .rev()
                .take(CHAT_VISIBLE_LINES)
                .collect();
            let chat_scale = self.settings.accessibility.chat_scale.clamp(0.5, 2.0);
            let chat_opacity = self.settings.accessibility.chat_opacity.clamp(0.0, 1.0);
            for (line_index, (sender, message)) in visible_messages.iter().enumerate() {
                let line: String = format!("<{sender}> {message}")
                    .to_uppercase()
                    .chars()
                    .take(96)
                    .collect();
                let y = -0.80 + line_index as f32 * 0.050 * chat_scale;
                let char_w = 0.008 * chat_scale;
                let char_h = 0.018 * chat_scale;
                let spacing = 0.002 * chat_scale;
                let width = line.chars().count() as f32 * (char_w + spacing) - spacing;
                let alpha = (1.0 - line_index as f32 * 0.07) * chat_opacity;
                add_ui_quad(
                    &mut ui_vertices,
                    -0.985,
                    (-0.955 + width).min(0.985),
                    y - 0.007,
                    y + char_h + 0.007,
                    [0.01, 0.01, 0.01, 0.52 * alpha],
                );
                add_string_lines(
                    &line,
                    -0.97,
                    y,
                    char_w,
                    char_h,
                    spacing,
                    [1.0, 1.0, 1.0, alpha],
                    &mut ui_line_vertices,
                );
            }

            if self.is_chat_open {
                add_ui_quad(
                    &mut ui_vertices,
                    -0.99,
                    0.99,
                    -0.97,
                    -0.875,
                    [0.01, 0.01, 0.01, 0.78],
                );
                add_ui_border(
                    &mut ui_line_vertices,
                    -0.99,
                    0.99,
                    -0.97,
                    -0.875,
                    [0.65, 0.65, 0.65, 0.9],
                );
                let mut visible_input: Vec<char> = self.chat_input.chars().rev().take(92).collect();
                visible_input.reverse();
                let mut input = String::from("> ");
                input.extend(visible_input);
                if (self.total_time * 2.0) as u32 % 2 == 0 {
                    input.push('_');
                }
                add_string_lines(
                    &input.to_uppercase(),
                    -0.97,
                    -0.935,
                    0.008 * chat_scale,
                    0.024 * chat_scale,
                    0.002 * chat_scale,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            if self.settings.accessibility.subtitles {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_millis() as u64)
                    .unwrap_or_default();
                for (index, event) in self
                    .audio_manager
                    .drain_subtitles(now_ms)
                    .into_iter()
                    .rev()
                    .take(4)
                    .enumerate()
                {
                    let direction_key = match event.direction {
                        crate::accessibility::SubtitleDirection::Left => {
                            "hud.subtitle.direction_left"
                        }
                        crate::accessibility::SubtitleDirection::Right => {
                            "hud.subtitle.direction_right"
                        }
                        crate::accessibility::SubtitleDirection::Front => {
                            "hud.subtitle.direction_front"
                        }
                        crate::accessibility::SubtitleDirection::Back => {
                            "hud.subtitle.direction_back"
                        }
                        crate::accessibility::SubtitleDirection::Center => "hud.subtitle.center",
                    };
                    let direction = self.translate(direction_key);
                    let sound = self.translate(event.key);
                    let text = if direction.is_empty() {
                        format!("[{}]", sound)
                    } else {
                        format!("[{}] {}", direction, sound)
                    };
                    let subtitle_scale = self.settings.accessibility.chat_scale.clamp(0.5, 2.0);
                    let y = 0.58 - index as f32 * 0.055 * subtitle_scale;
                    let char_w = 0.006 * subtitle_scale;
                    let char_h = 0.014 * subtitle_scale;
                    let spacing = 0.002 * subtitle_scale;
                    let width = text.chars().count() as f32 * (char_w + spacing) - spacing;
                    let x = (0.97 - width).max(0.02);
                    add_ui_quad(
                        &mut ui_vertices,
                        x - 0.012,
                        (0.98f32).min(x + width + 0.012),
                        y - 0.006,
                        y + char_h + 0.006,
                        [0.01, 0.01, 0.01, 0.82 * chat_opacity],
                    );
                    add_string_lines(
                        &text,
                        x,
                        y,
                        char_w,
                        char_h,
                        spacing,
                        [1.0, 1.0, 1.0, chat_opacity],
                        &mut ui_line_vertices,
                    );
                }
            }

            if let Some(boss) = crate::boss::active_boss_hud(&self.entity_manager) {
                let x0 = -0.42;
                let x1 = 0.42;
                let y0 = 0.82;
                let y1 = 0.875;
                add_ui_quad(&mut ui_vertices, x0, x1, y0, y1, [0.05, 0.01, 0.07, 0.92]);
                add_ui_quad(
                    &mut ui_vertices,
                    x0 + 0.008,
                    x0 + 0.008 + (x1 - x0 - 0.016) * boss.progress,
                    y0 + 0.009,
                    y1 - 0.009,
                    [0.55, 0.05, 0.65, 1.0],
                );
                let char_w = 0.010;
                let spacing = 0.003;
                let boss_title = if boss.title.eq_ignore_ascii_case("ENDER DRAGON") {
                    self.localized_entity_name(crate::entity::EntityType::EnderDragon)
                } else if boss.title.eq_ignore_ascii_case("WITHER") {
                    self.localized_entity_name(crate::entity::EntityType::Wither)
                } else {
                    boss.title.to_string()
                };
                let width = boss_title.chars().count() as f32 * (char_w + spacing) - spacing;
                add_string_lines(
                    &boss_title,
                    -width / 2.0,
                    0.895,
                    char_w,
                    0.02,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            self.render_advancement_ui_and_toasts(
                &mut ui_vertices,
                &mut ui_line_vertices,
                &mut ui_textured_vertices,
            );

            // Apply presentation settings after composing every HUD branch so
            // chat, subtitles, death, advancement and save/disconnect screens
            // share the same scale/contrast contract.  The fitted scale keeps
            // the full layout inside NDC instead of clamping individual glyph
            // vertices (which used to clip text at high DPI/UI scale).
            let requested_scale = self.settings.accessibility.ui_scale;
            // Use a stable full-screen extent.  Deriving this value from the
            // current vertices made the entire HUD subtly resize whenever a
            // chat/permission message appeared (notably after pressing G).
            let layout_scale = crate::accessibility::fit_ui_scale(requested_scale, 1.0);
            for vertex in ui_vertices.iter_mut() {
                vertex.position[0] *= layout_scale;
                vertex.position[1] *= layout_scale;
                if self.settings.accessibility.high_contrast {
                    vertex.color = crate::accessibility::high_contrast_color(vertex.color);
                }
            }
            for vertex in ui_line_vertices.iter_mut() {
                vertex.position[0] *= layout_scale;
                vertex.position[1] *= layout_scale;
                if self.settings.accessibility.high_contrast {
                    vertex.color = crate::accessibility::high_contrast_color(vertex.color);
                }
            }
            for vertex in ui_textured_vertices.iter_mut() {
                vertex.position[0] *= layout_scale;
                vertex.position[1] *= layout_scale;
                if self.settings.accessibility.high_contrast {
                    vertex.color = crate::accessibility::high_contrast_color(vertex.color);
                }
            }

            // Write Buffers
            let ui_vert_len = ui_vertices.len().min(UI_VERTEX_CAPACITY);
            let ui_line_vert_len = ui_line_vertices.len().min(UI_LINE_VERTEX_CAPACITY);
            let ui_textured_vert_len = ui_textured_vertices.len().min(UI_VERTEX_CAPACITY);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_textured_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_textured_vertices[..ui_textured_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len + ui_textured_vert_len)
                        * std::mem::size_of::<UiVertex>()) as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = ui_textured_vert_len as u32;
        }

        self.ui_vertices_scratch = ui_vertices;
        self.ui_line_vertices_scratch = ui_line_vertices;

        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareUi,
            ui_prepare_started.elapsed(),
        );
        self.gpu_upload_time_frame += *gpu_upload_elapsed;
        let mut total_draw_calls = 1 + self.submitted_terrain_draw_calls as u64;
        total_draw_calls += u64::from(self.mob_cuboid_num_instances > 0)
            + u64::from(self.mob_quad_num_instances > 0);
        total_draw_calls += u64::from(!self.particle_instances_scratch.is_empty());
        total_draw_calls += u64::from(self.mining_target.is_some() && self.mining_progress > 0.0);
        total_draw_calls += u64::from(
            self.hand_num_indices > 0
                && !self.camera_perspective.is_third_person()
                && !self.is_paused,
        );
        if self.is_paused {
            total_draw_calls += 2;
        } else {
            total_draw_calls += u64::from(self.num_ui_vertices > 0);
            total_draw_calls += u64::from(self.num_ui_textured_vertices > 0);
            total_draw_calls += 1; // Crosshair.
            total_draw_calls += u64::from(self.num_ui_line_vertices > 0);
        }
        self.perf_counters.draw_calls = total_draw_calls;
    }

    pub(super) fn encode_frame(
        &mut self,
        output: wgpu::SurfaceTexture,
        view: wgpu::TextureView,
        frame_submission_id: u64,
        allocs_before: u64,
    ) -> Result<(), wgpu::SurfaceError> {
        let render_encode_started = Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let mut crack_metrics: Option<(u64, u64)> = None;
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.camera_uniform.sky_color_horizon[0] as f64,
                            g: self.camera_uniform.sky_color_horizon[1] as f64,
                            b: self.camera_uniform.sky_color_horizon[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            // Draw Skybox first
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 0);
                }
            }
            render_pass.set_pipeline(&self.sky_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 1);
                }
            }

            // Pass 1: Opaque & Cutout
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 2);
                }
            }
            render_pass.set_pipeline(&self.terrain_render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            let mut bound_region: Option<(i32, i32)> = None;
            for candidate in &self.terrain_draw_plan_scratch.opaque {
                let lod = candidate.lod;
                let Some(layer) = self
                    .chunk_meshes
                    .get(&candidate.chunk_coord)
                    .and_then(|mesh| {
                        candidate
                            .section_y
                            .and_then(|section_y| mesh.section(section_y))
                    })
                    .and_then(|section| section.level(lod))
                    .map(|level| &level.opaque)
                else {
                    continue;
                };
                let Some(handle) = layer.handle else {
                    continue;
                };
                let region_coord = crate::chunk_render::chunk_to_region_coord(
                    candidate.chunk_coord.0,
                    candidate.chunk_coord.1,
                );
                let Some(region) = self.render_regions.get(&region_coord) else {
                    continue;
                };
                if !region.handle_is_live(&handle) {
                    continue;
                }
                if bound_region != Some(region_coord) {
                    render_pass.set_vertex_buffer(0, region.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(region.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.set_bind_group(1, &region.bind_group, &[]);
                    bound_region = Some(region_coord);
                }
                let Some(index_end) = handle.index_offset.checked_add(handle.num_indices) else {
                    continue;
                };
                let Ok(base_vertex) = i32::try_from(handle.vertex_offset) else {
                    continue;
                };
                render_pass.draw_indexed(handle.index_offset..index_end, base_vertex, 0..1);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 3);
                }
            }

            // Draw Mobs
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 4);
                }
            }
            if self.mob_cuboid_num_instances > 0 {
                render_pass.set_pipeline(&self.mob_instanced_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.mob_cuboid_proto_vbuf.slice(..));
                render_pass.set_vertex_buffer(
                    1,
                    self.mob_cuboid_instance_buffers[self.frame_ring_index].slice(..),
                );
                render_pass.set_index_buffer(
                    self.mob_cuboid_proto_ibuf.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..36, 0, 0..self.mob_cuboid_num_instances);
            }
            if self.mob_quad_num_instances > 0 {
                render_pass.set_pipeline(&self.mob_instanced_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.mob_quad_proto_vbuf.slice(..));
                render_pass.set_vertex_buffer(
                    1,
                    self.mob_quad_instance_buffers[self.frame_ring_index].slice(..),
                );
                render_pass.set_index_buffer(
                    self.mob_quad_proto_ibuf.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..12, 0, 0..self.mob_quad_num_instances);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 5);
                }
            }

            // Pass 2: Translucent (Water/Ice)
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 6);
                }
            }
            render_pass.set_pipeline(&self.terrain_trans_pipeline);
            let mut bound_region: Option<(i32, i32)> = None;
            for candidate in &self.terrain_draw_plan_scratch.transparent {
                let lod = candidate.lod;
                let Some(layer) = self
                    .chunk_meshes
                    .get(&candidate.chunk_coord)
                    .and_then(|mesh| {
                        candidate
                            .section_y
                            .and_then(|section_y| mesh.section(section_y))
                    })
                    .and_then(|section| section.level(lod))
                    .map(|level| &level.transparent)
                else {
                    continue;
                };
                let Some(handle) = layer.handle else {
                    continue;
                };
                let region_coord = crate::chunk_render::chunk_to_region_coord(
                    candidate.chunk_coord.0,
                    candidate.chunk_coord.1,
                );
                let Some(region) = self.render_regions.get(&region_coord) else {
                    continue;
                };
                if !region.handle_is_live(&handle) {
                    continue;
                }
                if bound_region != Some(region_coord) {
                    render_pass.set_vertex_buffer(0, region.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(region.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.set_bind_group(1, &region.bind_group, &[]);
                    bound_region = Some(region_coord);
                }
                let Some(index_end) = handle.index_offset.checked_add(handle.num_indices) else {
                    continue;
                };
                let Ok(base_vertex) = i32::try_from(handle.vertex_offset) else {
                    continue;
                };
                render_pass.draw_indexed(handle.index_offset..index_end, base_vertex, 0..1);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 7);
                }
            }

            // Draw billboard particles using instanced particle pipeline.
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 8);
                }
            }
            if !self.particle_instances_scratch.is_empty() {
                let num_particles = self.particle_instances_scratch.len() as u32;
                render_pass.set_pipeline(&self.particle_instanced_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.particle_proto_vbuf.slice(..));
                render_pass.set_vertex_buffer(
                    1,
                    self.particle_instance_buffers[self.frame_ring_index].slice(..),
                );
                render_pass.set_index_buffer(
                    self.particle_proto_ibuf.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..6, 0, 0..num_particles);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 9);
                }
            }

            // Draw Block cracking animation overlay (multiply blend)
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 10);
                }
            }
            if let Some(target) = self.mining_target {
                if self.mining_progress > 0.0 {
                    if let Some((_num_vertices, num_indices, upload_ns, upload_bytes)) =
                        self.update_crack_buffers(target, self.mining_progress)
                    {
                        crack_metrics = Some((upload_ns, upload_bytes));
                        render_pass.set_pipeline(&self.crack_pipeline);
                        render_pass.set_vertex_buffer(0, self.crack_vertex_buffer.slice(..));
                        render_pass.set_index_buffer(
                            self.crack_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        render_pass.draw_indexed(0..num_indices, 0, 0..1);
                    }
                }
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 11);
                }
            }

            // Draw first-person right hand and held item. Uses a dedicated
            // camera with a very near plane so the view-space model never
            // clips into world geometry. Hidden in third-person mode and when
            // the game is paused.
            if self.hand_num_indices > 0
                && !self.camera_perspective.is_third_person()
                && !self.is_paused
            {
                render_pass.set_pipeline(&self.hand_pipeline);
                render_pass.set_bind_group(0, &self.hand_camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.hand_vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(self.hand_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..self.hand_num_indices, 0, 0..1);
            }

            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 12);
                }
            }
            if !self.is_paused {
                // 1. Draw Colored UI (slot/panel backgrounds). Backgrounds go
                // first so the item icons drawn next stay fully visible;
                // previously the semi-transparent slot quads were drawn over
                // the icons and washed them out.
                if self.num_ui_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_pipeline);
                    render_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_vertices, 0..1);
                }

                // 2. Draw Textured UI (block thumbnails, hearts, dragged item)
                if self.num_ui_textured_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_textured_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.ui_textured_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_textured_vertices, 0..1);
                }

                // 3. Draw Crosshair (shared UI line pipeline)
                render_pass.set_pipeline(&self.ui_line_pipeline);
                render_pass.set_vertex_buffer(0, self.crosshair_buffer.slice(..));
                render_pass.draw(0..4, 0..1);

                // 4. Draw Line/Text UI (slot borders & texts)
                if self.num_ui_line_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_line_pipeline);
                    render_pass.set_vertex_buffer(0, self.ui_line_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_line_vertices, 0..1);
                }
            } else {
                // 3. Draw Pause Menu
                // Background overlay & buttons
                render_pass.set_pipeline(&self.ui_pipeline);
                render_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
                render_pass.draw(0..self.num_ui_vertices, 0..1);

                // Borders & Text
                render_pass.set_pipeline(&self.ui_line_pipeline);
                render_pass.set_vertex_buffer(0, self.ui_line_vertex_buffer.slice(..));
                render_pass.draw(0..self.num_ui_line_vertices, 0..1);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 13);
                }
            }
        }

        if let Some((upload_ns, upload_bytes)) = crack_metrics {
            self.gpu_upload_time_frame += Duration::from_nanos(upload_ns);
            self.gpu_upload_scopes_frame
                .record_nanos(crate::perf::UploadSource::Crack as usize, upload_ns);
            self.perf_counters.upload_bytes_frame = self
                .perf_counters
                .upload_bytes_frame
                .saturating_add(upload_bytes);
        }
        self.perf_recorder
            .record(crate::perf::ScopeId::GpuUpload, self.gpu_upload_time_frame);

        self.poll_gpu_timestamp_readbacks();
        let mut timestamp_readback_slot = None;
        if let (Some(query_set), Some(resolve_buffer)) = (
            &self.gpu_timestamp_query_set,
            &self.gpu_timestamp_resolve_buffer,
        ) {
            for (slot_index, slot) in self.gpu_timestamp_readback_slots.iter().enumerate() {
                if !slot
                    .status
                    .lock()
                    .unwrap()
                    .reserve_copy(frame_submission_id)
                {
                    continue;
                }
                encoder.resolve_query_set(
                    query_set,
                    0..GPU_TIMESTAMP_QUERY_COUNT,
                    resolve_buffer,
                    0,
                );
                encoder.copy_buffer_to_buffer(
                    resolve_buffer,
                    0,
                    &slot.buffer,
                    0,
                    GPU_TIMESTAMP_READBACK_BYTES,
                );
                timestamp_readback_slot = Some(slot_index);
                break;
            }
        }
        self.perf_counters.gpu_sky_ns = self.gpu_pass_timings_ns[0];
        self.perf_counters.gpu_opaque_ns = self.gpu_pass_timings_ns[1];
        self.perf_counters.gpu_mobs_ns = self.gpu_pass_timings_ns[2];
        self.perf_counters.gpu_translucent_ns = self.gpu_pass_timings_ns[3];
        self.perf_counters.gpu_particles_ns = self.gpu_pass_timings_ns[4];
        self.perf_counters.gpu_crack_ns = self.gpu_pass_timings_ns[5];
        self.perf_counters.gpu_ui_ns = self.gpu_pass_timings_ns[6];
        self.perf_counters.gpu_timestamps_supported = self.gpu_timestamps_supported;
        self.perf_counters.gpu_timestamps_inside_passes = self.gpu_timestamps_inside_passes;

        let command_buffer = encoder.finish();
        self.queue.submit(std::iter::once(command_buffer));
        let completion_tx = self.gpu_completion_tx.clone();
        self.queue.on_submitted_work_done(move || {
            let _ = completion_tx.send(frame_submission_id);
        });
        if let Some(slot_index) = timestamp_readback_slot {
            let slot = &self.gpu_timestamp_readback_slots[slot_index];
            if slot
                .status
                .lock()
                .unwrap()
                .begin_mapping(frame_submission_id)
            {
                let status = std::sync::Arc::clone(&slot.status);
                slot.buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        status
                            .lock()
                            .unwrap()
                            .map_completed(frame_submission_id, result.is_ok());
                    });
            }
        }
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderEncode,
            render_encode_started.elapsed(),
        );
        let present_started = Instant::now();
        output.present();
        self.perf_recorder
            .record(crate::perf::ScopeId::Present, present_started.elapsed());
        let allocs_after = crate::perf::thread_alloc_count();
        self.perf_counters.frame_allocations = allocs_after.saturating_sub(allocs_before);
        self.record_frame_perf_sample();
        Ok(())
    }
}

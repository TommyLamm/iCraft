# Skeleton Bow Model and Shooting Animation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a 3D Voxel Bow model held in the Skeleton's left hand and a pitch-aligned dynamic shooting/drawing animation during targeting.

**Architecture:** Add a dedicated Bow texture tile in `src/texture.rs` (Row 9 Col 9). Update `src/mob.rs` to track Skeleton pitch during target acquisition. In `src/mob_renderer.rs`, construct a multi-part 3D bow attached to the left hand, and animate arm pitch and string pull-back based on `entity.pitch` and `entity.action_cooldown`.

**Tech Stack:** Rust, glam (Vec3), wgpu (Vertex layout)

---

### Task 1: Add Bow Texture Tile to Texture Atlas

**Files:**
- Modify: `src/texture.rs`

- [ ] **Step 1: Add Row 9 Col 9 Bow texture generation in `src/texture.rs`**

Add the pixel art drawing block for Row 9 Col 9 in `src/texture.rs`:

```rust
        // Col 9: Bow (Row 9)
        {
            let ox = 9 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let is_bow_wood = (x >= 4 && x <= 11 && y >= 4 && y <= 11)
                        && ((x == y) || (x == y + 1) || (x + 1 == y));
                    let is_string = x == 12 && y >= 2 && y <= 13;
                    let c = if is_bow_wood {
                        let var = ((x * 7 + y * 13) % 20) as u8;
                        Rgba([130 + var, 80 + var / 2, 40 + var / 2, 255])
                    } else if is_string {
                        Rgba([230, 230, 235, 255])
                    } else {
                        Rgba([0, 0, 0, 0])
                    };
                    img.put_pixel(ox + x, oy + y, c);
                }
            }
        }
```

- [ ] **Step 2: Run `cargo check` to verify texture atlas updates compile**

Run: `cargo check --release`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/texture.rs
git commit -m "feat(texture): add bow texture tile at row 9 col 9"
```

---

### Task 2: Ensure Skeleton Pitch Tracking in `src/mob.rs`

**Files:**
- Modify: `src/mob.rs`

- [ ] **Step 1: Update Skeleton entity.pitch when aiming at player in `src/mob.rs`**

In `src/mob.rs`, inside `EntityType::Skeleton` logic, set `entity.pitch` based on `shoot_dir.y`:

```rust
                        let spawn_pos = entity.position + Vec3::new(0.0, 1.4, 0.0);
                        let mut shoot_dir =
                            (player_pos + Vec3::new(0.0, 1.0, 0.0) - spawn_pos).normalize_or_zero();
                        // Add slight gravity correction
                        shoot_dir.y += 0.08;
                        entity.pitch = shoot_dir.y.asin();
```

- [ ] **Step 2: Run unit tests to ensure no mob AI regressions**

Run: `cargo test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/mob.rs
git commit -m "feat(mob): update skeleton pitch aiming angle towards player target"
```

---

### Task 3: Implement Skeleton 3D Bow Model and Shooting Animation in `src/mob_renderer.rs`

**Files:**
- Modify: `src/mob_renderer.rs`

- [ ] **Step 1: Update Skeleton arm posing and add 3D Bow assembly in `src/mob_renderer.rs`**

Update `EntityType::Skeleton` in `src/mob_renderer.rs`:

```rust
            EntityType::Skeleton => {
                // Head (Col 4 front face, Col 5 others)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::new(0.0, 0.25, 0.0),
                    to_world(Vec3::new(0.0, 1.4, 0.0)),
                    entity.yaw,
                    entity.pitch,
                    [4, 5, 5, 5, 5, 5],
                    9,
                    light_val,
                );

                // Torso (Col 5)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.4, 0.75, 0.2),
                    Vec3::new(0.0, 0.375, 0.0),
                    to_world(Vec3::new(0.0, 0.65, 0.0)),
                    entity.yaw,
                    0.0,
                    [5; 6],
                    9,
                    light_val,
                );

                // Aiming calculation
                let target = entity.target_player;
                let aim_pitch = if target { entity.pitch } else { 0.0 };

                let left_arm_pitch = if target {
                    -std::f32::consts::FRAC_PI_2 + aim_pitch
                } else {
                    -swing
                };

                // Draw animation progress: action_cooldown from 2.0 to 0.0
                let draw_progress = if target {
                    ((2.0 - entity.action_cooldown) / 2.0).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                let right_arm_pitch = if target {
                    -std::f32::consts::FRAC_PI_2 + aim_pitch + 0.2 * (1.0 - draw_progress)
                } else {
                    swing
                };

                // Left Arm (holding bow)
                let left_shoulder = Vec3::new(-0.275, 1.3, 0.0);
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.325, 0.0),
                    to_world(left_shoulder),
                    entity.yaw,
                    left_arm_pitch,
                    [5; 6],
                    9,
                    light_val,
                );

                // Right Arm (drawing string)
                let right_shoulder = Vec3::new(0.275, 1.3, 0.0);
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.325, 0.0),
                    to_world(right_shoulder),
                    entity.yaw,
                    right_arm_pitch,
                    [5; 6],
                    9,
                    light_val,
                );

                // 3D Bow Model attached to Left Hand
                // Calculate left hand position in world space
                let left_hand_local = left_shoulder + Vec3::new(0.0, -0.6, 0.3);
                let bow_pivot = to_world(left_hand_local);

                // Bow Grip (Center)
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.08, 0.3, 0.08),
                    Vec3::ZERO,
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [9; 6], // Bow texture at Col 9 Row 9
                    9,
                    light_val,
                );

                // Bow Upper Limb
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.06, 0.4, 0.06),
                    Vec3::new(0.0, 0.3, -0.05),
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [9; 6],
                    9,
                    light_val,
                );

                // Bow Lower Limb
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.06, 0.4, 0.06),
                    Vec3::new(0.0, -0.3, -0.05),
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [9; 6],
                    9,
                    light_val,
                );

                // Bow String (Center pull-back driven by draw_progress)
                let string_offset_z = -0.1 - 0.25 * draw_progress;
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.02, 0.9, 0.02),
                    Vec3::new(0.0, 0.0, string_offset_z),
                    bow_pivot,
                    entity.yaw,
                    aim_pitch,
                    [9; 6],
                    9,
                    light_val,
                );

                // Legs (Col 5)
                // Left Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.375, 0.0),
                    to_world(Vec3::new(-0.1, 0.75, 0.0)),
                    entity.yaw,
                    swing,
                    [5; 6],
                    9,
                    light_val,
                );
                // Right Leg
                add_cuboid(
                    vertices,
                    indices,
                    Vec3::new(0.15, 0.75, 0.15),
                    Vec3::new(0.0, -0.375, 0.0),
                    to_world(Vec3::new(0.1, 0.75, 0.0)),
                    entity.yaw,
                    -swing,
                    [5; 6],
                    9,
                    light_val,
                );
            }
```

- [ ] **Step 2: Run `cargo check` to ensure syntax & types are valid**

Run: `cargo check --release`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/mob_renderer.rs
git commit -m "feat(mob_renderer): add 3D bow model and shooting animation for skeleton"
```

---

### Task 4: Full Test Suite and Verification

**Files:**
- Test: `cargo test`

- [ ] **Step 1: Execute all unit tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 2: Verify binary compilation**

Run: `cargo check --release`
Expected: PASS

- [ ] **Step 3: Commit plan completion**

```bash
git add docs/superpowers/plans/2026-07-21-skeleton-bow-model-and-animation.md
git commit -m "docs: complete skeleton bow model and shooting animation implementation plan"
```

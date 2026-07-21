# Skeleton Bow Model and Shooting Animation Design Document

## 1. Overview

The Skeleton mob currently lacks a 3D weapon model in hand and a dynamic shooting/drawing animation. This spec details adding a multi-part 3D Voxel Bow model attached to the Skeleton's left hand, alongside a dynamic archery pose that tilts up/down with pitch and smoothly draws the string back during the Skeleton's attack cooldown.

## 2. Requirements & Behavior

### 2.1 Texture Atlas (`src/texture.rs`)
- Add a dedicated Bow texture tile at Row 9, Col 9 in `texture_atlas.png`.
- Pixel art layout for Col 9 Row 9:
  - Dark/medium oak wood texture for bow limbs and grip.
  - Light gray/white texture for string tips and string line.

### 2.2 Skeleton Posing & Animation (`src/mob_renderer.rs`)
- **Pitch-Aligned Aiming**:
  - When `entity.target_player` is `true`, both arms tilt according to `entity.pitch` (aiming angle towards player).
- **Left Arm (Holding Bow)**:
  - When targeting: Extends forward towards the target angle holding the bow vertically.
  - When idle (`target_player == false`): Lowered at the skeleton's side, swinging with walking rhythm (`-swing`). The bow remains in hand, pointing downwards.
- **Right Arm (Drawing String)**:
  - When targeting: Raises towards chest/shoulder level.
  - **Draw Progress Calculation**: `draw_progress = ((2.0 - entity.action_cooldown) / 2.0).clamp(0.0, 1.0)`.
  - As `action_cooldown` ticks down from 2.0 to 0.0 seconds, the right hand pulls back towards the shoulder/chin.
  - When idle: Lowered at side, swinging with walking rhythm (`swing`).

### 2.3 3D Bow Model Assembly (`src/mob_renderer.rs`)
- A multi-cuboid assembly anchored to the left hand transform:
  - **Grip**: `0.08 x 0.3 x 0.08` wood cuboid centered on left hand.
  - **Upper Limb**: `0.06 x 0.45 x 0.06` angled cuboid extending up and slightly backward.
  - **Lower Limb**: `0.06 x 0.45 x 0.06` angled cuboid extending down and slightly backward.
  - **Bow String**: String segments connecting top/bottom limb tips, with the center vertex pulled back to the right hand position during draw state.

### 2.4 AI Pitch Tracking (`src/mob.rs`)
- Ensure `entity.pitch` is updated in `mob.rs` while aiming at player target position so the arm and bow pitch angles track height differences naturally.

## 3. Verification & Testing

- Run `cargo check --release` to verify standard build passes.
- Run `cargo test` to ensure existing mob and entity unit tests pass without regressions.

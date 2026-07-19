# Sprint and Sneak System Design

This document details the design for implementing the Sprint (Ctrl key or double-click W) and Sneak (Shift key) mechanics. This includes input binding, dynamic movement speed modification, camera view changes, entity height/hitbox alterations, and the edge fall-prevention physics algorithm.

---

## 1. Overview
The goal is to enhance player mobility with two core states:
- **Sprint**: Increases forward movement speed, dynamically expands field of view (FOV), drains hunger faster, and cancels upon collision or if the player stops moving.
- **Sneak**: Reduces movement speed, lowers the camera height, shrinks the player's physical hitbox (to traverse low openings), and prevents the player from falling off block edges.

---

## 2. Key Bindings and Input State Updates

Input processing will be updated in `src/app.rs` and `src/state.rs` to track the Left Ctrl and Left Shift keys.

### 2.1. KeyState Extension (`src/state.rs`)
We extend the `KeyState` structure to keep track of the Ctrl and Shift key states:
```rust
#[derive(Default)]
pub struct KeyState {
    pub w: bool,
    pub a: bool,
    pub s: bool,
    pub d: bool,
    pub space: bool,
    pub t: bool,
    pub ctrl: bool,  // Tracks Left Ctrl key
    pub shift: bool, // Tracks Left Shift key
}
```

### 2.2. Event Routing (`src/app.rs`)
In `App::window_event`, we route the keyboard events for Left Ctrl and Left Shift to the active state:
```rust
winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::ControlLeft) => {
    state.keys.ctrl = pressed;
}
winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::ShiftLeft) => {
    state.keys.shift = pressed;
}
```

---

## 3. Sprinting Logic (Sprint)

### 3.1. Entering Sprinting State
Sprinting can be triggered in two ways:
1. **Holding Ctrl**: Pressing Left Ctrl while moving forward.
2. **Double-tapping W** (Optional but recommended): Pressing W, releasing it, and pressing it again within `0.3` seconds.

In `State::update`, we track:
- `is_sprinting: bool`: True if the player is currently sprinting.
- `w_press_timer: f32`: Tracks time elapsed since W was last released.

Trigger conditions in `State::update`:
```rust
// Sprint state check
if self.keys.ctrl && self.keys.w && self.player_state.hunger > 6.0 {
    self.is_sprinting = true;
}
```

### 3.2. Cancelling Sprinting State
Sprinting cancels immediately if:
- The player releases W (no forward input).
- The player begins sneaking (Shift is pressed).
- The player's hunger drops to 6.0 or below.
- The player collides with a block along the horizontal movement direction (velocity is blocked by a wall).

```rust
if !self.keys.w || self.keys.shift || self.player_state.hunger <= 6.0 || (self.player_physics.velocity.x.abs() < 0.01 && self.player_physics.velocity.z.abs() < 0.01 && (self.keys.w || self.keys.a || self.keys.s || self.keys.d)) {
    self.is_sprinting = false;
}
```

### 3.3. Dynamic FOV Transition
When sprinting, the FOV expands by 10% to 15% (e.g., `base_fov * 1.12`). We smoothly transition the camera FOV using linear interpolation (lerp) inside the frame update:
```rust
let target_fov = if self.is_sprinting {
    self.base_fov * 1.12
} else {
    self.base_fov
};
// Smoothly interpolate the FOV value
self.camera.fov = self.camera.fov + (target_fov - self.camera.fov) * dt * 10.0;
```

---

## 4. Sneaking Logic (Sneak)

### 4.1. Movement Speed and Hitbox Adjustment
When the Shift key is pressed (`keys.shift` is true):
- The player enters the sneak state.
- Movement speed is multiplied by `0.3`.
- The physical size of the player is reduced to allow entering 1.5-block high gaps.

```rust
// Adjust physical hitbox size
if is_sneaking {
    self.player_physics.size.y = 1.5;
} else {
    // Restore default height
    self.player_physics.size.y = 1.8;
}
```

### 4.2. Camera Eye Height Adjustment
The camera eye height is offset by `-0.2` blocks when sneaking:
```rust
let eye_height = if self.keys.shift {
    1.42 // Normal eye height (1.62) - 0.20
} else {
    1.62
};
self.camera.position = self.player_physics.position + glam::Vec3::new(0.0, eye_height, 0.0);
```

### 4.3. Fall Prevention (Edge Sneaking)
To prevent the player from falling off edges when sneaking, the physics update detects if a movement step would result in no solid block supporting the player.

Inside `PlayerPhysics::update`, when `is_sneaking` and `self.on_ground` are both true, we perform a test movement:
- If moving along the X-axis would result in the player having no solid block underneath their AABB, we revert the X translation and zero the X velocity.
- If moving along the Z-axis would result in the player having no solid block underneath their AABB, we revert the Z translation and zero the Z velocity.

```rust
// Step 3: Shift X and resolve collision.
let old_x = self.position.x;
self.position.x += self.velocity.x * dt;
self.resolve_collisions(chunk_manager, 0);
if is_sneaking && self.on_ground {
    if !self.is_block_below(chunk_manager) {
        self.position.x = old_x;
        self.velocity.x = 0.0;
    }
}

// Step 4: Shift Z and resolve collision.
let old_z = self.position.z;
self.position.z += self.velocity.z * dt;
self.resolve_collisions(chunk_manager, 2);
if is_sneaking && self.on_ground {
    if !self.is_block_below(chunk_manager) {
        self.position.z = old_z;
        self.velocity.z = 0.0;
    }
}
```

The supporting block check `is_block_below` queries the block types underneath the player's current boundaries:
```rust
fn is_block_below(&self, chunk_manager: &ChunkManager) -> bool {
    let mut check_aabb = self.get_aabb();
    // Offset testing AABB downward slightly
    check_aabb.min.y -= 0.05;
    check_aabb.max.y = self.position.y; // Keep only feet level and below

    let min_x = check_aabb.min.x.floor() as i32;
    let max_x = check_aabb.max.x.floor() as i32;
    let min_y = (check_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
    let max_y = (check_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
    let min_z = check_aabb.min.z.floor() as i32;
    let max_z = check_aabb.max.z.floor() as i32;

    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let block = chunk_manager.get_block(x, y, z);
                if block.properties().is_solid {
                    let block_aabb = AABB::new(
                        Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                        Vec3::ONE,
                    );
                    if check_aabb.intersects(&block_aabb) {
                        return true;
                    }
                }
            }
        }
    }
    false
}
```

---

## 5. Hunger Depletion
Sprinting drains exhaustion faster due to higher kinetic activity. Inside `State::update`:
```rust
if self.is_sprinting && (self.keys.w || self.keys.a || self.keys.s || self.keys.d) {
    self.player_state.add_exhaustion(dt * 0.15); // Dynamic additional exhaustion
}
```

---

## 6. Verification Plan

### 6.1. Automated Unit Tests
We will add unit tests to verify:
- Edge sneaking detection: Assert that `is_block_below` returns false at a cliff edge and true on a flat platform.
- Speed modifiers: Verify that the physics speed factor behaves properly in normal, sprint, and crouch scenarios.

### 6.2. Manual Testing
1. **Sprint Movement**: Verify that holding Ctrl while pressing W accelerates the player. Confirm that FOV enlarges smoothly. Let go of W and confirm FOV returns to normal.
2. **Sneak Movement**: Hold Shift. Confirm the camera drops by 0.2 blocks and movement slows down. Walk to a cliff edge and verify that the player is blocked from falling.
3. **Collision Interruption**: Sprint into a wall. Verify that the player stops sprinting immediately.
4. **Hunger Interaction**: Confirm that sprinting for an extended duration depletes the hunger bar. Confirm that sprinting is blocked when hunger drops below 6.0.

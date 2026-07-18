# Day/Night Cycle Design Specification

## Overview
This specification details the implementation of a smooth, performant, and visual-rich Day/Night Cycle for the Rust/wgpu-based Minecraft clone. 

To achieve smooth transitions without CPU bottlenecking, the system uses a **packed vertex light** approach combined with **dynamic GPU scaling**. It also introduces a rotating celestial sphere, stars in the night sky, and keyboard controls for debugging.

---

## 1. Time System Model (`WorldTime`)

The global game time is modeled using Minecraft-standard ticks (20 ticks per second).

*   **Total cycle length**: 24,000 ticks (20 minutes of real time).
    *   `0`: Sunrise starts (6:00 AM)
    *   `6000`: Noon (12:00 PM) - Sun is directly overhead
    *   `12000`: Sunset starts (6:00 PM)
    *   `18000`: Midnight (12:00 AM) - Moon is directly overhead
*   **Time Progression**:
    *   Normal speed: `20 ticks/sec` (accumulated via delta time `dt`).
    *   Accelerated speed: If the `T` key is held, time moves at `4000 ticks/sec` (200x speed) to allow full day-night cycle review in ~6 seconds.

---

## 2. Vertex Light Packing Scheme

To avoid re-building the chunk mesh whenever the time of day changes, light levels are packed into the single `light_level: f32` vertex attribute during mesh generation:

$$packed = sky\_light + block\_light \times 16.0 + multiplier\_code \times 256.0$$

Where:
*   `sky_light` $\in [0, 15]$: Static sky light attenuation.
*   `block_light` $\in [0, 15]$: Ambient block illumination (e.g., from torches).
*   `multiplier_code` $\in \{0.0, 1.0, 2.0\}$: Directional face multiplier:
    *   `0.0` $\rightarrow 1.0$ (Top face)
    *   `1.0` $\rightarrow 0.8$ (Sides)
    *   `2.0` $\rightarrow 0.5$ (Bottom face)

---

## 3. Shader Changes (`src/shader.wgsl`)

### Unpacking & Combining Light (in `fs_main`):
```wgsl
let packed = in.light_level;
let multiplier_code = floor(packed / 256.0);
let rest = packed - multiplier_code * 256.0;
let block_light = floor(rest / 16.0);
let sky_light = rest - block_light * 16.0;

var multiplier = 1.0;
if (multiplier_code > 1.5) {
    multiplier = 0.5;
} else if (multiplier_code > 0.5) {
    multiplier = 0.8;
}

// camera.sun_dir.w stores the global sky light intensity factor (between 4/15 and 1.0)
let sky_intensity = camera.sun_dir.w;
let adjusted_sky_light = sky_light * sky_intensity;
let max_light = max(adjusted_sky_light, block_light);
let ambient = 0.08;
let final_light = max(max_light / 15.0, ambient) * multiplier;
```

### Rotating Sky & Procedural Stars (in `fs_sky`):
1.  **Celestial Rotation**: Use `atan2` on the sun's direction vector in world space to find its current rotation angle. Rotate the sky view vector around the Z-axis by this negative angle.
2.  **Procedural Stars**: Divide the upper hemisphere into a grid. Perform 3D hashing on grid coordinates to check if a star exists in a cell. Jitter the star center slightly within the cell to break the grid pattern.
3.  **Twinkle & Fading**: Stars fade in as `sun_dir.y` decreases below `0.1` and reach maximum opacity when `sun_dir.y < -0.1`.

---

## 4. Sky Colors & Light Levels

Colors and light levels will interpolate linearly between the keyframes based on the smooth `time_of_day`:

| Time Phase | `time_of_day` | Sky Top Color | Horizon Color | Sky Light Level |
|------------|---------------|---------------|---------------|-----------------|
| **Sunrise**| `0.00`        | `[0.1, 0.15, 0.3]` | `[0.9, 0.5, 0.2]` | `15` $\rightarrow$ Transitioning |
| **Day**    | `0.25`        | `[0.1, 0.25, 0.45]`| `[0.53, 0.81, 0.92]`| `15` |
| **Sunset** | `0.50`        | `[0.05, 0.1, 0.25]`| `[0.9, 0.4, 0.15]` | `15` $\rightarrow$ Transitioning |
| **Night**  | `0.75`        | `[0.01, 0.01, 0.03]`| `[0.02, 0.02, 0.05]`| `4` |

---

## 5. UI and Controls

*   **F3 Debug Screen**: Toggled by `F3`. It will print:
    *   Game time (Format: `Day X, HH:MM (Ticks: Y)`)
    *   Player position (`XYZ: ...`)
    *   Camera orientation (`DIR: ...`)
    *   Global sky light level (`SKY LIGHT: X`)
*   **T Key Acceleration**: Accelerates game ticks to `4000 ticks/sec` while held.

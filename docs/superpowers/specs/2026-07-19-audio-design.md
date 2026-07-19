# Audio System Design Specification

## Overview
This specification details the implementation of a fully-featured, 3D spatialized Audio System for the Rust/wgpu-based Minecraft clone.

To provide a seamless developer and player experience out-of-the-box, the system implements a **hybrid audio engine** using the `rodio` library. If custom audio files are missing under the `assets/sounds/` directory, the engine programmatically synthesizes default WAV sounds in memory and writes them to the disk, ensuring audio works instantly without requiring manual asset downloads.

---

## 1. Audio Engine Architecture (`src/audio.rs`)

The system introduces a dedicated `AudioManager` responsible for managing the audio device stream, loading cached sound effects, and playing both 2D (UI/Player) and 3D spatial (Blocks/Mobs) sounds.

### Sound Material & Event Enumerations:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundMaterial {
    Grass,
    Wood,
    Sand,
    Gravel,
    Stone,
    Snow,
    Ice,
    Glass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundId {
    BlockBreak(SoundMaterial),
    BlockPlace(SoundMaterial),
    Footstep(SoundMaterial),
    Jump,
    Land(SoundMaterial),
    PlayerHurt,
    PlayerDeath,
    UiClick,
    CreeperIgnition,
    Explosion,
    ArrowShoot,
}
```

### Audio Manager Structure:
```rust
pub struct AudioManager {
    _stream: Option<rodio::OutputStream>,
    stream_handle: Option<rodio::OutputStreamHandle>,
    pub volume: f32, // 0.0 to 1.0
    // Cached WAV bytes for each sound event
    sound_cache: std::collections::HashMap<SoundId, Vec<u8>>,
    // Tracks active looping or defusable spatial sinks (e.g. Creeper hiss) by Entity ID
    active_loops: std::collections::HashMap<u64, rodio::Sink>,
}
```
* **Robustness**: If no host audio device is found (e.g., in a CI or headless system), `OutputStream::try_default()` fails gracefully by printing a warning and setting `stream_handle = None`. Playback calls will check for this and do nothing rather than crashing.

---

## 2. Programmatic Sound Synthesis & Asset Setup

At startup, the `AudioManager` initializes the `assets/sounds/` directory. For each `SoundId`, it determines a filename (e.g., `stone_break.wav`).

* **Loading Flow**:
  1. If the file exists under `assets/sounds/`, load it into the `sound_cache`.
  2. If missing, synthesize the sound in-memory as a mono `f32` buffer, convert it to 16-bit PCM WAV bytes, write the file to `assets/sounds/` (so the player can inspect or override it), and load the bytes into the cache.

### Synthesis Algorithms (Mono, 22050Hz, 16-bit PCM WAV):
1. **`UiClick`**: A short (0.05s) sine wave at 1000Hz with linear decay.
2. **`PlayerHurt`**: A pitch-sweeping triangle wave (180Hz down to 80Hz over 0.15s) with a quick exponential decay, creating an "oof" sound.
3. **`PlayerDeath`**: A longer pitch-sweeping wave (120Hz down to 40Hz over 0.4s).
4. **`ArrowShoot`**: High-frequency white noise filtered to simulate a bowstring snap (0.12s decay).
5. **`Explosion`**: High-amplitude white noise shaped with a heavy running-average low-pass filter decaying exponentially over 1.5 seconds.
6. **`CreeperIgnition`**: High-frequency white noise hiss (1.5 seconds).
7. **`Footstep(material)`**: A short (0.15s) crunch of random noise passed through material envelopes:
   * `Grass`: Soft, low-amplitude noise with a low-pass filter.
   * `Stone`: Loud transient pop followed by a very short mid-frequency noise burst.
   * `Wood`: Low-frequency wooden tap mixed with quiet noise.
   * `Sand` / `Gravel`: Higher frequency soft rustles.
8. **`BlockBreak` & `BlockPlace`**: Re-uses footsteps with slightly modified duration and amplitudes.

---

## 3. 3D Spatial Audio & Listener Coordinates

For events occurring at a specific coordinates `position: glam::Vec3` (like block actions and mobs):

1. **Ear Calculation**: Based on the player's camera position (`camera.position`) and the right vector of the camera (`listener_right = Vec3::new(-yaw_sin, 0.0, yaw_cos).normalize()`):
   $$\text{left\_ear} = \text{position} - \text{listener\_right} \times 0.15$$
   $$\text{right\_ear} = \text{position} + \text{listener\_right} \times 0.15$$
2. **Spatial Sink Setup**: Use `rodio::SpatialSink` to play the sound:
   ```rust
   let sink = rodio::SpatialSink::try_new(
       handle,
       [position.x, position.y, position.z],
       [left_ear.x, left_ear.y, left_ear.z],
       [right_ear.x, right_ear.y, right_ear.z],
   );
   ```
3. **One-shot Sounds**: Configure the volume, add the cached sound decoder, and detach the sink immediately (`sink.detach()`).
4. **Looping / Defusable Sounds**:
   * For the Creeper ignition hiss, we store the sink in `active_loops` using the Creeper's `entity.id`.
   * In `update_mobs` each frame, update its position relative to the moving player.
   * If defused, we remove the sink from `active_loops` (dropping it stops the sound). If exploded, play a spatial explosion sound and remove it.

---

## 4. State Integration & Trigger Points

### State Struct Fields (`src/state.rs`):
```rust
pub struct State {
    ...
    pub audio_manager: AudioManager,
    pub footstep_accumulator: f32,
    pub was_on_ground: bool,
}
```

### Event Trigger Points:
* **UI Interaction**:
  * In `handle_menu_click` (Pause menu button click).
  * In `handle_inventory_click` (Slot click, item swap, or crafting event).
* **Block Actions**:
  * In `handle_click` (Creative block break or block placement).
  * In `break_block` (Survival block break after cracking completion).
* **Movement Events**:
  * **Footsteps**: In `update`, if player `on_ground` is true, accumulate `horizontal_dist` in `footstep_accumulator`. When it exceeds `2.0` meters, play `Footstep(block_under)` and subtract `2.0`.
  * **Jump**: In `update`, if `jumped` is true, play `SoundId::Jump`.
  * **Landing**: In `update`, if `on_ground && !was_on_ground`, play `Land(block_under)`.
* **Combat & Damage**:
  * In `take_damage`, play `PlayerDeath` if the damage kills the player; otherwise, play `PlayerHurt` if health was actually reduced (not ignored by invulnerability frames).
  * In `update_mobs` (in `src/mob.rs`):
    * Skeleton shoot arrows $\rightarrow$ play `ArrowShoot` spatially.
    * Creeper ignition $\rightarrow$ start `CreeperIgnition` looping sound.
    * Creeper defusal $\rightarrow$ stop `CreeperIgnition` sound.
    * Explosion $\rightarrow$ play `Explosion` sound.

---

## 5. Volume Settings UI

We integrate the volume slider in `GameSettings` and the pause menu:

1. **`GameSettings` expansion**:
   ```rust
   pub struct GameSettings {
       pub fov: f32,
       pub sensitivity: f32,
       pub render_distance: i32,
       pub volume: f32, // Added: 0.0 to 1.0
   }
   ```
2. **Menu Shifting**:
   * Volume is rendered as a slider button at `Y: [-0.32, -0.22]`.
   * Quit button is shifted down to `Y: [-0.46, -0.36]`.
3. **Adjustment**:
   * Clicking the volume button adjusts `volume` (Left side click decreases by 10%, right side click increases by 10%).
   * Trigger immediate saving and update of `audio_manager.volume`.

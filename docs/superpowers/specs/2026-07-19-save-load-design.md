# World Save and Load System Design

This document details the design for implementing a world save and load system for the voxel game. It includes the save directory layout, serialization structures utilizing Serde and Bincode, Zlib compression of voxel data, region file management (32x32 chunks), thread-based non-blocking autosaving, and pause menu integration.

---

## 1. Overview
The goal is to persist the entire state of the game, including voxel modifications, player coordinates/states, inventory, and world settings. The system will:
- Add dependencies `serde`, `bincode`, and `flate2` to `Cargo.toml`.
- Derive `Serialize` and `Deserialize` on existing enums (`Item`, `GameMode`, `BlockType`, `DamageSource`).
- Structure saves under a dedicated directory: `saves/world_001/`.
- Pack chunks into region files (`saves/world_001/regions/r.X.Z.bin`), where each region file covers a 32x32 chunk grid and maps local coordinates to compressed chunk data.
- Support thread-based background autosaving (every 5 minutes) to avoid blocking the main simulation thread.
- Update the Pause menu to replace the "QUIT" button with "SAVE AND QUIT", rendering a "SAVING WORLD..." message while flushing remaining changes to disk during shutdown.

---

## 2. Directory Layout & Storage Structure

The world save files will be organized under the project working directory as follows:

```text
saves/
└── world_001/
    ├── level.dat          # World metadata (seed, game time ticks)
    ├── player.dat         # Player state (coordinates, health, hunger, inventory)
    └── regions/
        ├── r.0.0.bin      # Region file for chunk coordinates [0..31, 0..31]
        ├── r.0.-1.bin     # Region file for chunk coordinates [0..31, -32..-1]
        └── ...
```

---

## 3. Data Serialization Structures (`src/save.rs`)

We will create a new module `src/save.rs` containing the following serializable representations:

### 3.1. World Metadata (`LevelData`)
Saves basic world parameters like the procedural generation seed and current time ticks:
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LevelData {
    pub seed: u32,
    pub time: u64,
}
```

### 3.2. Player and Inventory Data (`PlayerData`)
Stores all player stats and inventory items.
```rust
use crate::inventory::{GameMode, Item};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerData {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub health: f32,
    pub hunger: f32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub oxygen: f32,
    pub game_mode: GameMode,
    pub inventory: InventoryData,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InventoryData {
    pub hotbar: Vec<Option<ItemStackData>>,
    pub main: Vec<Option<ItemStackData>>,
    pub armor: Vec<Option<ItemStackData>>,
    pub selected: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemStackData {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
}
```

### 3.3. Chunk and Region Data (`RegionData`)
To store chunks efficiently, each region file represents a `RegionData` struct. Chunks within the region are individually compressed to optimize memory usage and loading times.
```rust
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegionData {
    /// Maps local coordinate (0..32, 0..32) -> Compressed ChunkSaveData bytes
    pub chunks: HashMap<(u8, u8), Vec<u8>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChunkSaveData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: Vec<u8>,        // Zlib compressed u8 array of BlockType
    pub sky_light: Vec<u8>,     // Zlib compressed u8 array of sky light
    pub block_light: Vec<u8>,   // Zlib compressed u8 array of block light
    pub fluid_levels: Vec<u8>,  // Zlib compressed u8 array of fluid levels
}
```

---

## 4. Save & Load Lifecycle

### 4.1. SaveManager and Caching
We will implement a `SaveManager` struct that manages save folders, channel-based saving, and keeps a read-cache of `RegionData` maps to prevent constant disk access:
```rust
pub struct SaveManager {
    pub world_dir: std::path::PathBuf,
    // Caches loaded regions to speed up subsequent chunk loading
    region_cache: std::collections::HashMap<(i32, i32), RegionData>,
}
```

### 4.2. Chunk Streaming Integration
- **On Chunk Load**: In `State::update_chunks()`, when a chunk `(cx, cz)` is requested:
  - Check if region file `r.rx.rz.bin` (where `rx = cx.div_euclid(32)`, `rz = cz.div_euclid(32)`) exists and contains the chunk at `(cx.rem_euclid(32), cz.rem_euclid(32))`.
  - If it exists: Read, decompress the chunk arrays using `flate2::read::ZlibDecoder`, and build the `Chunk`. Recalculate heightmap.
  - If not: Generate chunk terrain procedurally and mark it as dirty.
- **On Chunk Unload**: When a chunk is unloaded, send its data to the background save queue.

---

## 5. Thread-Based Autosaving

To ensure that saving large numbers of chunks does not stutter the main game thread, we will introduce a background worker thread:
- At startup, `State` spawns a background thread listening on a channel: `Receiver<SaveCommand>`.
- `SaveCommand` represents saving actions:
  ```rust
  pub enum SaveCommand {
      SaveChunk(ChunkSaveData),
      SaveLevelAndPlayer(LevelData, PlayerData),
  }
  ```
- **Every 5 minutes**: The main thread clones dirty chunk data and triggers a channel send. The background thread updates the region files in-memory and flushes them to disk.

---

## 6. Shutdown and "Save and Quit" Flow

To guarantee that no block changes are lost when the game closes:
1. Replace the text `"QUIT"` in the pause menu with `"SAVE AND QUIT"`.
2. When clicked, set `State::is_saving = true` and render a `SAVING WORLD...` overlay screen.
3. Call `queue.submit(...)` and present the frame immediately.
4. Block the main thread to synchronously write all dirty chunks, player states, and level data to disk.
5. Exit the winit event loop.

---

## 7. Verification Plan

### 7.1. Automated Tests
- Implement serialization roundtrip tests in `src/save.rs` verifying that `ChunkSaveData` and `PlayerData` encode and decode correctly via Bincode/Zlib.
- Verify compilation: `cargo check --release` and `cargo test`.

### 7.2. Manual Verification
1. **World Persistence**: Start the game, place unique blocks (e.g., Diamond block or Cobblestone), move the player, and exit using the "SAVE AND QUIT" button. Restart the game and verify that the player spawns at the correct position with the modified blocks intact.
2. **Inventory Persistence**: Collect drops (e.g., Wood, Wheat), put them in the backpack, open the crafting table, craft bread, and exit. Restart and verify that the items in the inventory/hotbar are preserved.
3. **Autosave Test**: Set the autosave timer to 10 seconds. Place some blocks, wait 15 seconds, and forcefully terminate the process (e.g., via Task Manager or terminal). Restart the game and verify that the changes were persisted.

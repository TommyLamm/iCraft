# iCraft Performance Benchmark Scenes (Phase 0 Baseline)

This directory contains standard, reproducible benchmark scenarios for performance regression testing and optimization verification in iCraft.

## Benchmark Matrix

| ID | Scene Name | World Seed | Primary Bottleneck Focus |
|---|---|---|---|
| 01 | Overworld Static & Spin | `1337001` | Frustum culling, terrain draw calls, vertex count |
| 02 | Cave Occlusion Stress | `1337002` | Occlusion culling candidates, hidden chunk overhead |
| 03 | High-Speed Flight Streaming | `1337003` | Chunk generation, meshing, worker queue depth |
| 04 | High-Density Redstone Circuit | `1337004` | 20 Hz Redstone tick graph & propagation latency |
| 05 | Fluid & Lighting Stress | `1337005` | BFS sky/block light propagation & mesh invalidation |
| 06 | Entity Crowd & Wall Occlusion | `1337006` | Entity physics, mob mesh assembly, frustum/occlusion culling |
| 07 | Autosave & Continuous Dirty Save | `1337007` | Background SaveManager I/O, region cache memory |
| 08 | Multiplayer Host-Client Join | `1337008` | Packet encoding, pose interpolation, net event queue depth |

## Running Benchmarks

1. Run the target scene in release mode:
   ```bash
   cargo run --release
   ```
2. Press `F3` in-game to display the extended observability HUD.
3. Record CPU average/p95/p99 per scope, GPU pass timings, queue depths, and memory working set.

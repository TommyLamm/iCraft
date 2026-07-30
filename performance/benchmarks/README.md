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

## R9 fixed-scene artifact protocol

Use `performance/tools/Invoke-R9Matrix.ps1` for eight scenes at render distance 16; each run records warmup, sample, and repetition metadata. Validate and summarize with `Validate-R9Jsonl.ps1` and `Measure-R9Runs.ps1`. Missing metrics are fatal, never zero-filled. The stable JSONL frame schema includes timestamps, scene/repetition/phase, frame/cpu/gpu timings, working set, upload, draw calls, buffer bytes, queue depth/delay, and a correctness checksum.

Acceptance is predeclared: five repetitions per scene, exact checksum parity, and PGO inclusion only when median CPU p50 improves at least 3%, CPU p95/p99 regress no more than 1%, working set regresses no more than 5%, and correctness is unchanged. `Compare-R9Pgo.ps1` records the gate without fabricating results. Capture contemporaneous host/GPU/driver/OS/git/settings with `New-R9Manifest.ps1`; fixtures and dry-run checks are under `performance/tools/tests`.

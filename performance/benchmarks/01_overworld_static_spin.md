# Scenario 01: Overworld Static & Spin

> Seed: `1337001`
> Render Distance: 8 / 16
> Target: Static terrain rendering, frustum culling, GPU draw calls

## Description

Tests basic rendering and camera rotation overhead over dense terrain without ongoing world mutations.

## Setup & Execution

1. Start game with seed `1337001` in Survival or Creative mode.
2. Teleport / move to coordinates `(X: 100.0, Y: 80.0, Z: 100.0)`.
3. Stand stationary for 10 seconds to let initial chunk loading settle.
4. Perform 360-degree continuous camera horizontal rotation at constant speed (approx 5 seconds per full turn).
5. Record F3 statistics for RenderPrepareTerrain, GpuUpload, RenderEncode, and GPU Opaque/Translucent passes.

## Metrics to Track

- `visible_chunks` vs `loaded_chunks`
- `terrain_candidates` and `terrain_triangles`
- `RenderPrepareTerrain` p95 / p99
- GPU pass timings (Opaque & Translucent)

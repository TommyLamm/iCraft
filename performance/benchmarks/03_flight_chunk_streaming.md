# Scenario 03: High-Speed Flight Chunk Streaming

> Seed: `1337003`
> Render Distance: 12
> Target: Terrain generation, meshing worker queue, stale result disposal

## Description

Tests background chunk streaming and meshing throughput while traveling rapidly across chunk boundaries.

## Setup & Execution

1. Start Creative mode with seed `1337003`.
2. Toggle flight and fly diagonally at constant maximum speed along `(X: +20, Z: +20)` per second.
3. Fly for 30 seconds straight.
4. Record worker in-flight queue depth, stale result count, and `TerrainResultIntegrate` CPU scope.

## Metrics to Track

- `in_flight` worker queue
- `stale_results` counter
- `cancelled` worker jobs
- `TerrainResultIntegrate` average and p95

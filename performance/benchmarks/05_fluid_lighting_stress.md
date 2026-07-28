# Scenario 05: Fluid Flow & Lighting Propagation Stress

> Seed: `1337005`
> Render Distance: 8
> Target: BFS lighting update, chunk light recalculation, mesh invalidation

## Description

Tests sky and block light propagation overhead triggered by large fluid cascades and block removal.

## Setup & Execution

1. Load world with seed `1337005`.
2. Release water/lava source at high elevation `(Y: 120)` to flood a large vertical cave section.
3. Record `lighting` CPU scope (which includes chunk load propagation + mutation) during fluid flow.

## Metrics to Track

- `Lighting (LOAD+MUTATION)` CPU scope p95 and p99
- Mesh dirtying frequency and `gpu_upload` spikes

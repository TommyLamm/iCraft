# Scenario 02: Cave Occlusion Stress

> Seed: `1337002`
> Render Distance: 12
> Target: Occlusion culling candidates, hidden chunk overhead

## Description

Places the player deep underground in a cavern system surrounded by solid terrain to measure overdraw and hidden geometry draw submission.

## Setup & Execution

1. Start game with seed `1337002`.
2. Move to underground position `(X: 32.0, Y: 24.0, Z: 32.0)`.
3. Stand stationary looking towards dense surrounding rock wall.
4. Record F3 statistics for hidden vs visible chunk counts and terrain draw call count.

## Metrics to Track

- `loaded_chunks` vs `visible_chunks`
- `submitted_terrain_draw_calls`
- `RenderPrepareTerrain` latency

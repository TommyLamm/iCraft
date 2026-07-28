# Scenario 06: Entity Crowd & Wall Occlusion

> Seed: `1337006`
> Render Distance: 8
> Target: Entity physics, mob mesh generation, frustum & occlusion culling counters

## Description

Spawns 500 mobs / items behind a solid stone wall to verify entity frustum culling and CPU mob rendering efficiency.

## Setup & Execution

1. Load world with seed `1337006`.
2. Spawn 500 mobs (e.g. Zombies) behind a 3-block thick stone wall directly in front of camera.
3. Observe `entities_rendered` vs `entities_frustum_culled` on F3.
4. Turn camera 180 degrees away to verify all 500 entities move to `frustum_culled`.

## Metrics to Track

- `rendered_entities` vs `frustum_culled_entities`
- `RenderPrepareEntities` CPU scope
- GPU `MOBS` pass timing

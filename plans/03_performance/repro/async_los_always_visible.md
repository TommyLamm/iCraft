# `repro_async_los_worker_always_visible`

> Status: open structural/visual reproduction
> Input: seed `42`, an entity farther than 16 blocks behind a solid opaque wall
> Repair round: R6

## Evidence

- `src/culling.rs:347-352` — `EntityLosRequest` carries positions and camera
  cell, but no voxel snapshot, dimension generation, Chunk revisions, or entity
  generation.
- `src/culling.rs:382-394` — the worker calls `is_los_blocked` with an occluder
  closure that always returns `false`, therefore every async result has
  `is_visible: true`.
- `src/culling.rs:406-423` — polling a visible result resets hysteresis.
- `src/culling.rs:459-493` — a synchronous blocked sample increments
  hysteresis, but queues the terrain-free async request and returns visible.

## Replay

1. In a seed-42 world, build a wall of opaque full cubes.
2. Put a non-bypass entity more than 16 blocks from the camera, fully behind the
   wall.
3. Call/render `is_entity_visible`, poll async results each frame, and record the
   cache entry.
4. The synchronous DDA can report blocked once, but the worker result reports
   visible and resets `hysteresis_count` before three blocked confirmations
   accumulate.

Expected: a valid terrain snapshot produces a blocked result after hysteresis;
stale/unknown data remains conservatively visible.

Actual: the async worker cannot ever report blocked, so wall-hidden entities
remain visible and the advertised async LOS culling never takes effect.

## Closure gap

There is no golden/integration test for wall, wall removal, door/open model,
teleport, queue overflow, timeout, dimension switch, or stale Chunk revision.
R6 must supply immutable voxel identity in requests and reject stale results
while retaining fail-open behavior for uncertainty.


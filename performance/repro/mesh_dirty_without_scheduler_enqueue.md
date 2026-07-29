# `repro_mesh_dirty_without_scheduler_enqueue`

> Status: open structural/manual reproduction
> Input: seed `42`, render distance `8`, Overworld, a loaded Chunk containing
> flowing water or a breakable block
> Repair round: R1

## Evidence

- `src/state.rs:5486-5490` — `State::mark_chunk_dirty` increments
  `ChunkMesh::revision` and calls `scheduler.mark_dirty(coord)`.
- `src/state.rs:5861-5888` — mesh dispatch iterates only
  `scheduler.dirty_chunk_meshes`.
- `src/state.rs:6020-6024` and `6036-6040` — water/lava mutations call
  `mesh.mark_dirty()` directly without scheduler enqueue.
- The same split ownership exists in authoritative batch, redstone and break
  paths at `src/state.rs:1372-1375`, `7403-7406`, and `7777-7780`.

## Replay

1. Start a seed-42 world, stay inside one loaded Chunk, and let its current mesh
   reach `meshed_revision == revision`.
2. Record that Chunk coordinate and confirm it is absent from
   `scheduler.dirty_chunk_meshes`.
3. Trigger a water/lava update in that Chunk (or break a block through the
   authoritative break path).
4. Break/trace immediately after the direct `mesh.mark_dirty()` call.
5. Inspect the same `ChunkMesh` and scheduler set, then leave the player in the
   same Chunk so no target-set rebuild incidentally masks the failure.

## Reproduced invariant violation

| State | Expected | Current path |
|---|---|---|
| `mesh.revision != mesh.meshed_revision` | true | true |
| `scheduler.dirty_chunk_meshes.contains(coord)` | true | false |
| worker eventually receives new revision | true | not guaranteed |

The visible terrain mesh can therefore remain stale indefinitely even though
the authoritative block/light/fluid state changed.

## Closure gap

There is no integration test that follows
`mutation → scheduler queued → worker result → visible mesh revision`, including
Chunk boundary and diagonal AO dependencies. R1 must add that test and remove
runtime direct ownership of `ChunkMesh::mark_dirty`.


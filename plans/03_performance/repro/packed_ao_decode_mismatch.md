# `repro_packed_ao_shader_decode_mismatch`

> Status: open deterministic numeric/golden reproduction
> Input: seed `42` (seed-independent); AO codes `0`, `1`, `2`, `3`
> Repair round: R6

## Evidence

- `src/world.rs:1539-1545` — CPU AO values are
  `1.0`, `0.75`, `0.5`, `0.25` for zero through three occluders.
- `src/chunk_render.rs:52-63` — packed codes are `3`, `2`, `1`, `0`
  respectively.
- `src/chunk_render.rs:113-120` — CPU decode correctly maps
  `3/2/1/0 → 1.0/0.75/0.5/0.25`.
- `src/shader.wgsl:162-163` — WGSL decodes `ao_raw / 3.0` (with code 3 selected
  to 1.0), producing `1.0`, `0.6667`, `0.3333`, `0.0`.

## Replay

Construct or pack one vertex for each discrete AO code and compare
`TerrainVertex::ao()` with the WGSL expression:

| Code | CPU expected/decoded | Current WGSL | Error |
|---:|---:|---:|---:|
| 3 | 1.00 | 1.00 | 0.00 |
| 2 | 0.75 | 0.6667 | -0.0833 |
| 1 | 0.50 | 0.3333 | -0.1667 |
| 0 | 0.25 | 0.00 | -0.25 |

Expected: CPU packing, CPU decode and WGSL decode use the same four discrete
levels.

Actual: three of four codes are darker in the shader, including fully
occluded corners becoming zero instead of `0.25`.

## Closure gap

No CPU-packing ↔ WGSL parity test or GPU golden image exists. R6 must replace
the linear shader expression with the discrete mapping and add a parity/golden
test covering all four codes and representative Chunk/section boundaries.


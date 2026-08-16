# Performance Audit Reproductions

> Captured: 2026-07-29
> Audit baseline: `master` / `5f1ee4d`
> Default world seed: `42` (all five failures are seed-independent)
> Status: open; these are structural/manual reproductions until the owning repair
> round adds an automated regression test.

This directory preserves the five concrete failure paths that caused tasks
01–14 to be rolled back to `Partial`. Each entry gives a stable proposed test
name, exact current source evidence, deterministic input, replay steps,
expected/actual behavior, the owning repair round, and the missing closure
artifact.

| ID / proposed regression test | Failure | Repair |
|---|---|---|
| [`repro_mesh_dirty_without_scheduler_enqueue`](mesh_dirty_without_scheduler_enqueue.md) | mesh revision changes without scheduler ownership | R1 |
| [`repro_atomic_write_remove_before_replace`](save_non_atomic_replace.md) | save replacement has a missing-file crash window | R2 |
| [`repro_catchup_mailbox_full_drops_chunk`](catchup_mailbox_silent_drop.md) | full catch-up mailbox silently loses a Chunk | R3 |
| [`repro_async_los_worker_always_visible`](async_los_always_visible.md) | async LOS worker has no terrain and returns visible | R6 |
| [`repro_packed_ao_shader_decode_mismatch`](packed_ao_decode_mismatch.md) | CPU AO codes and WGSL decode disagree | R6 |

## Replay conventions

- Use a release build and a new world with seed `42` unless an entry describes a
  smaller unit harness.
- Keep render distance at 8 when comparing with
  [`../baselines/2026-07-28_windows_dx12.md`](../baselines/2026-07-28_windows_dx12.md).
  That baseline is not evidence for render distance 16 claims.
- Source line numbers refer to the captured audit baseline and were rechecked
  against the working tree on 2026-07-29. Prefer the named symbols if later
  repairs move the lines.
- A reproduction is not considered closed by compilation or an unrelated unit
  test. The owning repair document defines the required automated/golden/fault
  injection evidence.


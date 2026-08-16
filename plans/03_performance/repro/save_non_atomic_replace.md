# `repro_atomic_write_remove_before_replace`

> Status: open debugger/fault-injection reproduction
> Input: seed `42`, Windows, an existing `level.dat` or region file
> Repair round: R2

## Evidence

- `src/save.rs:668-690` — `atomic_write` writes and `sync_all`s a fixed `.tmp`
  file. If `rename(tmp, destination)` fails and the destination exists, it
  removes the destination at line 681 before retrying the rename.
- `src/save.rs:910-929` — failure to open/read/deserialize an existing region is
  ignored; an empty `RegionData` can then be inserted and written over the
  existing region.
- The save command/snapshot path has no persisted revision ACK ownership, so an
  enqueue or I/O failure cannot safely prove which dirty revision is durable.

## Replay A — missing-file crash window

1. On Windows, create and save a seed-42 world twice so `level.dat` (or one
   `r.*.*.bin`) already exists.
2. Attach a debugger or temporary fault injector and break immediately after
   `fs::remove_file(path)` at `src/save.rs:681`.
3. Trigger another save. The first rename to the existing destination fails,
   the code removes the old file, and execution reaches the breakpoint.
4. Terminate the process before the retry at line 682.
5. Restart and inspect the target path.

Expected: either the complete old file or the complete new file exists.

Actual: the destination can be absent; the old durable version was deleted
before the replacement became durable.

## Replay B — corrupt-region overwrite

1. Keep a backup, then corrupt one byte of an existing region file that contains
   at least two Chunks.
2. Load/mutate only one Chunk in that region and trigger a save.
3. The read/deserialize errors are ignored, an empty region is created, and the
   one new Chunk can replace the file.

Expected: corruption is reported and the region is not overwritten.

Actual: unrelated Chunk records in the same region can be lost.

## Closure gap

No automated test currently injects enqueue, serialize, open/read, write,
replace, worker-panic, or crash failures. R2 must add bounded revision ownership,
`Result`-bearing flush/ACK, platform-atomic replacement, corruption refusal, and
restart verification that only a complete old or complete new revision exists.


# R9 artifact schema and protocol

`Validate-R9Jsonl.ps1` enforces `icraft.r9.frame.v1`: one JSON object per line with non-negative numeric `frameIndex`, `repetition`, `frameTimeMs`, `cpuMs`, `gpuMs`, `workingSetBytes`, `uploadBytes`, `drawCalls`, `bufferBytes`, `queueDepth`, and `queueDelayMs`, plus `timestampUtc`, `sceneId`, `phase`, and non-empty `checksum`. Missing or malformed metrics fail closed.

`Measure-R9Runs.ps1` emits p50/p95/p99 timings, 1% low FPS (`1000 / frame-time p99`), working-set/upload/draw/buffer p50, queue-depth/delay p95, and exact checksum parity. `Invoke-R9Matrix.ps1` creates eight scene directories at render distance 16 with five repetitions by default. PGO is admitted only at CPU p50 improvement >=3%, p95/p99 regressions <=1%, working-set regression <=5%, and exact checksum parity.

# R9 artifact schema and protocol

`Validate-R9Jsonl.ps1` enforces the frame schema: one JSON object per line with non-negative, finite numeric `frameIndex`, `repetition`, `frameTimeMs`, `cpuMs`, `gpuMs`, `workingSetBytes`, `uploadBytes`, `drawCalls`, `bufferBytes`, aggregate `queueDepth`/`queueDelayMs`, and separate `saveQueue*`/`networkQueue*` metrics, plus `timestampUtc`, `sceneId`, `phase`, and non-empty `checksum`. Missing, malformed, duplicate, or changing checksums fail closed. It can also assert the expected scene and repetition after a matrix run.

`Measure-R9Runs.ps1` emits CPU and GPU p50/p95/p99 timings, 1% low FPS (`1000 / frame-time p99`), working-set/upload/draw/buffer percentiles, queue-depth/delay p95, and exact checksum parity. `Invoke-R9Matrix.ps1` reads `performance/benchmarks/r9-scenes.json`, creates eight scene directories at render distance 16 with five repetitions by default, and writes a run manifest before invoking the workload. PGO is admitted only at CPU p50 improvement >=3%, p95/p99 regressions <=1%, working-set regression <=5%, and exact checksum parity.

Run the local tooling checks with:

```powershell
pwsh performance/tools/Test-R9Tools.ps1
```

These checks exercise schemas and dry-run orchestration only; they do not
pretend to be a GPU/window benchmark.

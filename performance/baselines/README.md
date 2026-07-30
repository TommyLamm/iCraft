# R9 baseline evidence

The only checked-in numeric baseline is
[`2026-07-28_windows_dx12.md`](2026-07-28_windows_dx12.md). It is historical
render-distance **8** data and is intentionally not a render-distance 16
before/after artifact. It has incomplete host metadata and no replayable raw
frame stream, so it cannot support an R9 performance claim.

R9 baselines must be captured on the same host and commit as the corresponding
after run. Use `performance/tools/New-R9Manifest.ps1` at capture time and keep
the generated manifest beside the raw JSONL files. Do not hand-edit this
directory with numbers from an unrelated machine or run.

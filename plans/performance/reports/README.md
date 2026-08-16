# R9 reports and raw artifacts

This directory is the output location for the fixed-scene artifact gate. A
capture is complete only when every scene has five repetitions for each phase
(`before` and `after`, or `non-pgo` and `pgo`), a contemporaneous hardware
manifest, and raw frame JSONL. The checked-in repository currently contains no
GPU/window capture; therefore no performance improvement is claimed.

## Capture

```powershell
pwsh plans/performance/tools/New-R9Manifest.ps1 `
  -OutputPath plans/performance/reports/<capture>/manifest.json `
  -SettingsPath <settings.json> -WgpuBackend dx12 `
  -Resolution 1920x1080 -RenderDistance 16

pwsh plans/performance/tools/Invoke-R9Matrix.ps1 `
  -Command <workload-executable> -Phase before `
  -OutputRoot plans/performance/reports/<capture>/before
```

Repeat the matrix with `-Phase after` using the same seed, settings, host and
render distance. The workload, not the wrapper, is responsible for writing
`frames.jsonl` using the schema enforced by `Validate-R9Jsonl.ps1`.

## Summarize and gate

```powershell
pwsh plans/performance/tools/Measure-R9Runs.ps1 `
  -InputPath plans/performance/reports/<capture>/before `
  -OutputPath plans/performance/reports/<capture>/before-summary.json `
  -ManifestPath plans/performance/reports/<capture>/manifest.json
```

Use `Compare-R9Pgo.ps1` only after both measured summaries exist. It emits a
`pending` decision when evidence is missing and never invents timings or GPU
results. `plans/performance/reports/r9-report-template.md` lists the required
provenance and acceptance fields for a human review.

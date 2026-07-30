# R9 reports and raw artifacts

This directory is the output location for the fixed-scene artifact gate. A
capture is complete only when every scene has five repetitions for each phase
(`before` and `after`, or `non-pgo` and `pgo`), a contemporaneous hardware
manifest, and raw frame JSONL. The checked-in repository currently contains no
GPU/window capture; therefore no performance improvement is claimed.

## Capture

```powershell
pwsh performance/tools/New-R9Manifest.ps1 `
  -OutputPath performance/reports/<capture>/manifest.json `
  -SettingsPath <settings.json> -WgpuBackend dx12 `
  -Resolution 1920x1080 -RenderDistance 16

pwsh performance/tools/Invoke-R9Matrix.ps1 `
  -Command <workload-executable> -Phase before `
  -OutputRoot performance/reports/<capture>/before
```

Repeat the matrix with `-Phase after` using the same seed, settings, host and
render distance. The workload, not the wrapper, is responsible for writing
`frames.jsonl` using the schema enforced by `Validate-R9Jsonl.ps1`.

## Summarize and gate

```powershell
pwsh performance/tools/Measure-R9Runs.ps1 `
  -InputPath performance/reports/<capture>/before `
  -OutputPath performance/reports/<capture>/before-summary.json `
  -ManifestPath performance/reports/<capture>/manifest.json
```

Use `Compare-R9Pgo.ps1` only after both measured summaries exist. It emits a
`pending` decision when evidence is missing and never invents timings or GPU
results. `performance/reports/r9-report-template.md` lists the required
provenance and acceptance fields for a human review.

# R9 fixed-scene artifact report

> Status: **Pending measurement**. Replace this line only after raw artifacts,
> manifest, and the verification commands below are available.

## Provenance

| Field | Value |
|---|---|
| Capture ID | `<capture-id>` |
| Commit | `<manifest.git.commit>` |
| Host manifest | [`manifest.json`](manifest.json) |
| Raw before | [`before/`](before/) |
| Raw after | [`after/`](after/) |
| Render distance | `16` (required) |
| Resolution | `<manifest.capture.resolution>` |
| wgpu backend | `<manifest.capture.wgpuBackend>` |

The manifest is captured at run time. Do not fill CPU, GPU, driver, RAM, or
OS values from memory.

## Scene matrix

| ID | Scene | Seed | Before summary | After summary | Checksum parity | Decision |
|---|---|---:|---|---|---|---|
| 01 | overworld-static-spin | 1337001 | pending | pending | pending | pending |
| 02 | cave-occlusion-stress | 1337002 | pending | pending | pending | pending |
| 03 | high-speed-flight-streaming | 1337003 | pending | pending | pending | pending |
| 04 | high-density-redstone | 1337004 | pending | pending | pending | pending |
| 05 | fluid-lighting-stress | 1337005 | pending | pending | pending | pending |
| 06 | entity-crowd-wall-occlusion | 1337006 | pending | pending | pending | pending |
| 07 | autosave-continuous-dirty | 1337007 | pending | pending | pending | pending |
| 08 | multiplayer-host-client-join | 1337008 | pending | pending | pending | pending |

Each summary must be generated from raw JSONL by `Measure-R9Runs.ps1`; do not
paste hand-written percent changes. Required metrics are CPU/GPU p50/p95/p99,
1% low FPS, working set, upload bytes, draw calls, buffer bytes, save/network
queue depth and delay, and the complete world checksum fields.

## PGO A/B

PGO uses the same scene matrix and repetitions as non-PGO. The result is
`pending` unless `Compare-R9Pgo.ps1` has both summaries. The admission gate is
CPU p50 improvement at least 3%, CPU p95/p99 regression at most 1%, working
set regression at most 5%, and exact checksum parity for every scene. A failed
gate leaves the non-PGO release as the selected build.

## Reproduction and verification

```powershell
pwsh performance/tools/Validate-R9Jsonl.ps1 -Path <raw.jsonl>
pwsh performance/tools/Measure-R9Runs.ps1 -InputPath <raw-root> -ManifestPath <manifest.json>
git diff --check
```

Until these commands have been run against real captures, this report is a
template and must not be cited as a measured result.

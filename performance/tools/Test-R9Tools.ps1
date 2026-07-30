[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$tools = $PSScriptRoot
$repo = (Resolve-Path (Join-Path $tools '../..')).Path
$tmp = Join-Path ([IO.Path]::GetTempPath()) "icraft-r9-tools-$PID"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
try {
    foreach ($file in Get-ChildItem -LiteralPath $tools -Filter '*.ps1' -File) {
        $tokens = $null; $errors = $null
        [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors) | Out-Null
        Assert ($errors.Count -eq 0) "PowerShell parse errors in $($file.Name)"
    }

    $sample = Join-Path $tools 'tests/sample.jsonl'
    $validation = & (Join-Path $tools 'Validate-R9Jsonl.ps1') -Path $sample | ConvertFrom-Json
    Assert ($validation.schema -eq 'icraft.r9.validation.v1') 'sample validation did not return the expected schema'
    $rejected = $false
    try { & (Join-Path $tools 'Validate-R9Jsonl.ps1') -Path (Join-Path $tools 'tests/invalid-negative.jsonl') | Out-Null }
    catch { $rejected = $true }
    Assert $rejected 'invalid JSONL fixture was accepted'

    $input = Join-Path $tmp 'input'; New-Item -ItemType Directory -Force -Path $input | Out-Null
    Copy-Item -LiteralPath $sample -Destination (Join-Path $input 'frames.jsonl')
    $summaryPath = Join-Path $tmp 'summary.json'
    & (Join-Path $tools 'Measure-R9Runs.ps1') -InputPath $input -OutputPath $summaryPath | Out-Null
    $summary = Get-Content -Raw -LiteralPath $summaryPath | ConvertFrom-Json
    Assert ($summary.schema -eq 'icraft.r9.summary.v2') 'summary schema mismatch'
    Assert ($null -ne $summary.scenes[0].gpuP99Ms) 'GPU percentile is missing from summary'

    $manifestPath = Join-Path $tmp 'manifest.json'
    & (Join-Path $tools 'New-R9Manifest.ps1') -RepoRoot $repo -OutputPath $manifestPath -WgpuBackend 'test' -Resolution '1x1' | Out-Null
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    Assert ($manifest.capture.renderDistance -eq 16) 'manifest did not pin render distance 16'

    $matrixRoot = Join-Path $tmp 'matrix'
    & (Join-Path $tools 'Invoke-R9Matrix.ps1') -Command 'cargo' -SceneIds @('01') -Repetitions 1 -OutputRoot $matrixRoot -DryRun | Out-Null
    $run = Get-Content -Raw -LiteralPath (Join-Path $matrixRoot 'scene-01/rep-1/run.json') | ConvertFrom-Json
    Assert ($run.seed -eq 1337001 -and $run.renderDistance -eq 16) 'matrix metadata is not reproducible'

    $pgoPath = Join-Path $tmp 'pgo.json'
    & (Join-Path $tools 'Compare-R9Pgo.ps1') -BaselineBuildCommand 'cargo build' -InstrumentedBuildCommand 'cargo build' -OptimizedBuildCommand 'cargo build' -WorkloadCommand 'cargo run' -OutputPath $pgoPath -DryRun | Out-Null
    $pgo = Get-Content -Raw -LiteralPath $pgoPath | ConvertFrom-Json
    Assert ($pgo.gate.decision -eq 'pending' -and -not $pgo.gate.measured) 'PGO missing evidence was not fail-closed'
    Write-Output 'R9 PowerShell tool tests passed (no GPU/window measurement performed).'
} finally {
    if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force }
}

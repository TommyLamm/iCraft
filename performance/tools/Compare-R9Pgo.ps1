[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BaselineBuildCommand,
    [Parameter(Mandatory)][string]$InstrumentedBuildCommand,
    [Parameter(Mandatory)][string]$OptimizedBuildCommand,
    [Parameter(Mandatory)][string]$WorkloadCommand,
    [string]$OutputRoot = 'performance/reports/r9-pgo',
    [string]$LlvmProfdata = 'llvm-profdata',
    [ValidateSet(5)][int]$Repetitions = 5,
    [string]$BaselineRuns,
    [string]$PgoRuns,
    [string]$BaselineSummaryPath,
    [string]$PgoSummaryPath,
    [string]$OutputPath = 'performance/reports/r9-pgo-comparison.json',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (New-Item -ItemType Directory -Force -Path $OutputRoot)).Path
$profile = Join-Path $root 'profiles'
New-Item -ItemType Directory -Force -Path $profile | Out-Null
$git = try {
    $gitRoot = (Resolve-Path '.').Path
    $gitSafeRoot = $gitRoot -replace '\\', '/'
    $value = & git -c "safe.directory=$gitSafeRoot" -C $gitRoot rev-parse HEAD 2>$null
    ($value -join "`n").Trim()
} catch { $null }

# RUSTFLAGS is set by the wrapper at execution time. Keeping it separate from
# the cargo command avoids passing unsupported -C flags to cargo itself.
$commands = @(
    [ordered]@{ name = 'baseline-build'; command = $BaselineBuildCommand },
    [ordered]@{ name = 'baseline-workload'; command = "$WorkloadCommand --r9-output `"$(Join-Path $root 'baseline')`" --repetitions $Repetitions" },
    [ordered]@{ name = 'instrumented-build'; command = "`$env:RUSTFLAGS='-Cprofile-generate=$profile'; $InstrumentedBuildCommand" },
    [ordered]@{ name = 'profile-workload'; command = "`$env:LLVM_PROFILE_FILE='$(Join-Path $profile 'run-%p-%m.profraw')'; $WorkloadCommand --r9-output `"$(Join-Path $root 'instrumented')`" --repetitions $Repetitions" },
    [ordered]@{ name = 'profile-merge'; command = "$LlvmProfdata merge -sparse `"$(Join-Path $profile '*.profraw')`" -o `"$(Join-Path $profile 'merged.profdata')`"" },
    [ordered]@{ name = 'optimized-build'; command = "`$env:RUSTFLAGS='-Cprofile-use=$(Join-Path $profile 'merged.profdata')'; $OptimizedBuildCommand" },
    [ordered]@{ name = 'pgo-workload'; command = "$WorkloadCommand --r9-output `"$(Join-Path $root 'pgo')`" --repetitions $Repetitions" }
)

function Invoke-CommandText([string]$Text) {
    # Commands are deliberately supplied by the operator. ScriptBlock keeps
    # arguments such as `cargo run --release` intact in PowerShell.
    $script = [scriptblock]::Create($Text)
    & $script
    if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw "Command failed: $Text" }
}

function Get-Gate([string]$BaselinePath, [string]$PgoPath) {
    if (-not $BaselinePath -or -not $PgoPath) {
        return [ordered]@{ decision = 'pending'; measured = $false; accepted = $false; reasons = @('Measured baseline and PGO summaries were not supplied') }
    }
    if (-not (Test-Path -LiteralPath $BaselinePath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $PgoPath -PathType Leaf)) {
        return [ordered]@{ decision = 'pending'; measured = $false; accepted = $false; reasons = @('A supplied summary path does not exist') }
    }
    $baseline = Get-Content -Raw -LiteralPath $BaselinePath | ConvertFrom-Json -ErrorAction Stop
    $pgo = Get-Content -Raw -LiteralPath $PgoPath | ConvertFrom-Json -ErrorAction Stop
    $baseRows = @($baseline.scenes); $pgoRows = @($pgo.scenes)
    $details = [Collections.Generic.List[object]]::new(); $failures = [Collections.Generic.List[string]]::new()
    foreach ($b in $baseRows) {
        $matches = @($pgoRows | Where-Object { $_.sceneId -eq $b.sceneId -and $_.phase -eq $b.phase })
        if ($matches.Count -eq 0) { $matches = @($pgoRows | Where-Object { $_.sceneId -eq $b.sceneId }) }
        if ($matches.Count -ne 1) { $failures.Add("Missing unique PGO row for scene $($b.sceneId)"); continue }
        $p = $matches[0]
        $cpuImprovement = if ([double]$b.cpuP50Ms -gt 0) { ( [double]$b.cpuP50Ms - [double]$p.cpuP50Ms ) / [double]$b.cpuP50Ms } else { 0 }
        $p95Regression = if ([double]$b.cpuP95Ms -gt 0) { [double]$p.cpuP95Ms / [double]$b.cpuP95Ms - 1 } else { 0 }
        $p99Regression = if ([double]$b.cpuP99Ms -gt 0) { [double]$p.cpuP99Ms / [double]$b.cpuP99Ms - 1 } else { 0 }
        $workingRegression = if ([double]$b.workingSetP50Bytes -gt 0) { [double]$p.workingSetP50Bytes / [double]$b.workingSetP50Bytes - 1 } else { 0 }
        $checksumExact = ([string]$b.checksum -eq [string]$p.checksum)
        $accepted = $cpuImprovement -ge 0.03 -and $p95Regression -le 0.01 -and $p99Regression -le 0.01 -and $workingRegression -le 0.05 -and $checksumExact
        if (-not $accepted) { $failures.Add("Gate failed for scene $($b.sceneId)") }
        $details.Add([ordered]@{ sceneId = $b.sceneId; phase = $b.phase; cpuP50Improvement = $cpuImprovement; cpuP95Regression = $p95Regression; cpuP99Regression = $p99Regression; workingSetRegression = $workingRegression; checksumExact = $checksumExact; accepted = $accepted })
    }
    return [ordered]@{ decision = if ($failures.Count -eq 0) { 'accept' } else { 'reject' }; measured = $true; accepted = ($failures.Count -eq 0 -and $details.Count -gt 0); reasons = @($failures); scenes = @($details); thresholds = [ordered]@{ cpuP50Improvement = 0.03; cpuP95RegressionMax = 0.01; cpuP99RegressionMax = 0.01; workingSetRegressionMax = 0.05; checksum = 'exact' } }
}

if ($DryRun) {
    $commands | ForEach-Object { Write-Output "$($_.name): $($_.command)" }
} else {
    $profTool = Get-Command $LlvmProfdata -ErrorAction SilentlyContinue
    if (-not $profTool -and -not (Test-Path -LiteralPath $LlvmProfdata)) { throw "llvm-profdata not found: $LlvmProfdata" }
    foreach ($entry in $commands) {
        if ($entry.name -eq 'profile-merge') {
            $profraw = @(Get-ChildItem -LiteralPath $profile -Filter '*.profraw' -File -ErrorAction Stop | ForEach-Object { $_.FullName })
            if ($profraw.Count -eq 0) { throw "No .profraw files found under $profile" }
            & $LlvmProfdata merge -sparse @profraw -o (Join-Path $profile 'merged.profdata')
            if ($LASTEXITCODE -ne 0) { throw 'llvm-profdata merge failed' }
        } else { Invoke-CommandText $entry.command }
    }
}

$hashes = @{}
foreach ($entry in $commands) {
    $bytes = [Text.Encoding]::UTF8.GetBytes([string]$entry.command)
    $hashes[$entry.name] = ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))).ToLowerInvariant()
}
$gate = Get-Gate $BaselineSummaryPath $PgoSummaryPath
$result = [ordered]@{
    schema = 'icraft.r9.pgo-comparison.v2'
    generatedUtc = [DateTime]::UtcNow.ToString('o')
    patchId = $git
    repetitions = $Repetitions
    llvmProfdata = if ($profTool) { $profTool.Source } else { $LlvmProfdata }
    dryRun = [bool]$DryRun
    commands = $commands
    commandSha256 = $hashes
    baselineRuns = $BaselineRuns
    pgoRuns = $PgoRuns
    baselineSummary = $BaselineSummaryPath
    pgoSummary = $PgoSummaryPath
    gate = $gate
}
$parent = Split-Path -Parent $OutputPath
if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
$result | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 -LiteralPath $OutputPath
Write-Output (Resolve-Path -LiteralPath $OutputPath)

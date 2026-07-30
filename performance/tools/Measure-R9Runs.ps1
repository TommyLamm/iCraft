[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InputPath,
    [string]$OutputPath = 'performance/reports/r9-summary.json',
    [string]$ManifestPath
)

$ErrorActionPreference = 'Stop'
$validator = Join-Path $PSScriptRoot 'Validate-R9Jsonl.ps1'

function Get-Quantile([double[]]$Values, [double]$Quantile) {
    if ($Values.Count -eq 0) { throw 'Cannot calculate a quantile for an empty sample' }
    $sorted = @($Values | Sort-Object)
    $index = ($sorted.Count - 1) * $Quantile
    $lower = [math]::Floor($index)
    $upper = [math]::Ceiling($index)
    if ($lower -eq $upper) { return [double]$sorted[$lower] }
    return [double]$sorted[$lower] + ($sorted[$upper] - $sorted[$lower]) * ($index - $lower)
}

if (-not (Test-Path -LiteralPath $InputPath -PathType Container)) {
    throw "Input path does not exist or is not a directory: $InputPath"
}
$files = @(Get-ChildItem -LiteralPath $InputPath -Filter '*.jsonl' -File -Recurse)
if ($files.Count -eq 0) { throw "No JSONL runs found under $InputPath" }

$records = [Collections.Generic.List[object]]::new()
foreach ($file in $files) {
    & $validator -Path $file.FullName | Out-Null
    foreach ($line in Get-Content -LiteralPath $file.FullName) {
        if (-not [string]::IsNullOrWhiteSpace($line)) {
            $records.Add(($line | ConvertFrom-Json -ErrorAction Stop))
        }
    }
}

$groups = @($records | Group-Object { "$($_.sceneId)|$($_.phase)" })
$rows = [Collections.Generic.List[object]]::new()
$sceneChecksums = @{}
foreach ($group in $groups) {
    $samples = @($group.Group)
    $checksums = @($samples | ForEach-Object { [string]$_.checksum } | Select-Object -Unique)
    if ($checksums.Count -ne 1) {
        throw "Checksum parity failure for scene/phase $($group.Name)"
    }
    $sceneId, $phase = $group.Name -split '\|', 2
    if (-not $sceneChecksums.ContainsKey($sceneId)) { $sceneChecksums[$sceneId] = @{} }
    $sceneChecksums[$sceneId][$phase] = $checksums[0]

    $frame = @($samples | ForEach-Object { [double]$_.frameTimeMs })
    $cpu = @($samples | ForEach-Object { [double]$_.cpuMs })
    $gpu = @($samples | ForEach-Object { [double]$_.gpuMs })
    $working = @($samples | ForEach-Object { [double]$_.workingSetBytes })
    $upload = @($samples | ForEach-Object { [double]$_.uploadBytes })
    $draw = @($samples | ForEach-Object { [double]$_.drawCalls })
    $buffer = @($samples | ForEach-Object { [double]$_.bufferBytes })
    $queueDepth = @($samples | ForEach-Object { [double]$_.queueDepth })
    $queueDelay = @($samples | ForEach-Object { [double]$_.queueDelayMs })
    $saveQueueDepth = @($samples | ForEach-Object { [double]$_.saveQueueDepth })
    $saveQueueDelay = @($samples | ForEach-Object { [double]$_.saveQueueDelayMs })
    $networkQueueDepth = @($samples | ForEach-Object { [double]$_.networkQueueDepth })
    $networkQueueDelay = @($samples | ForEach-Object { [double]$_.networkQueueDelayMs })
    $frameP99 = Get-Quantile $frame .99
    $rows.Add([ordered]@{
        sceneId = $sceneId
        phase = $phase
        frames = $samples.Count
        checksum = $checksums[0]
        cpuP50Ms = Get-Quantile $cpu .50
        cpuP95Ms = Get-Quantile $cpu .95
        cpuP99Ms = Get-Quantile $cpu .99
        gpuP50Ms = Get-Quantile $gpu .50
        gpuP95Ms = Get-Quantile $gpu .95
        gpuP99Ms = Get-Quantile $gpu .99
        frameP50Ms = Get-Quantile $frame .50
        frameP95Ms = Get-Quantile $frame .95
        frameP99Ms = $frameP99
        onePercentLowFps = if ($frameP99 -gt 0) { 1000 / $frameP99 } else { 0 }
        workingSetP50Bytes = Get-Quantile $working .50
        workingSetP95Bytes = Get-Quantile $working .95
        uploadP50Bytes = Get-Quantile $upload .50
        uploadP95Bytes = Get-Quantile $upload .95
        drawP50 = Get-Quantile $draw .50
        drawP95 = Get-Quantile $draw .95
        bufferP50Bytes = Get-Quantile $buffer .50
        bufferP95Bytes = Get-Quantile $buffer .95
        queueDepthP95 = Get-Quantile $queueDepth .95
        queueDelayP95Ms = Get-Quantile $queueDelay .95
        saveQueueDepthP95 = Get-Quantile $saveQueueDepth .95
        saveQueueDelayP95Ms = Get-Quantile $saveQueueDelay .95
        networkQueueDepthP95 = Get-Quantile $networkQueueDepth .95
        networkQueueDelayP95Ms = Get-Quantile $networkQueueDelay .95
    })
}

# A deterministic workload must produce the same checksum in before/after or
# non-PGO/PGO phases. We only report this fact; we do not repair or synthesize it.
$parity = [Collections.Generic.List[object]]::new()
foreach ($scene in ($sceneChecksums.Keys | Sort-Object)) {
    $phases = @($sceneChecksums[$scene].Keys)
    $values = @($phases | ForEach-Object { $sceneChecksums[$scene][$_] } | Select-Object -Unique)
    $parity.Add([ordered]@{ sceneId = $scene; phases = $phases; exact = ($values.Count -eq 1); checksums = $sceneChecksums[$scene] })
}

$manifest = $null
if ($ManifestPath) {
    if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) { throw "Manifest does not exist: $ManifestPath" }
    $manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json -ErrorAction Stop
    $manifestDistance = $null
    if ($null -ne $manifest.capture -and $null -ne $manifest.capture.renderDistance) {
        $manifestDistance = [int]$manifest.capture.renderDistance
    } elseif ($null -ne $manifest.settings -and $null -ne $manifest.settings.renderDistance) {
        $manifestDistance = [int]$manifest.settings.renderDistance
    }
    if ($null -ne $manifestDistance -and $manifestDistance -ne 16) {
        throw 'R9 summaries require render distance 16'
    }
}

$out = [ordered]@{
    schema = 'icraft.r9.summary.v2'
    generatedUtc = [DateTime]::UtcNow.ToString('o')
    inputPath = (Resolve-Path -LiteralPath $InputPath).Path
    manifestPath = if ($ManifestPath) { (Resolve-Path -LiteralPath $ManifestPath).Path } else { $null }
    manifest = $manifest
    scenes = @($rows)
    checksumParity = @($parity)
}
$parent = Split-Path -Parent $OutputPath
if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
$out | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 -LiteralPath $OutputPath
Write-Output (Resolve-Path -LiteralPath $OutputPath)

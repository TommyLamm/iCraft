[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Command,
    [string]$OutputRoot = 'performance/reports/r9-runs',
    [int]$Repetitions = 5,
    [int]$WarmupSeconds = 15,
    [int]$SampleSeconds = 30,
    [ValidateSet(16)][int]$RenderDistance = 16,
    [ValidateSet('before', 'after', 'non-pgo', 'pgo')][string]$Phase = 'before',
    [string]$SceneManifestPath = 'performance/benchmarks/r9-scenes.json',
    [string[]]$SceneIds = @('01', '02', '03', '04', '05', '06', '07', '08'),
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
if ($Repetitions -lt 1) { throw 'Repetitions must be at least one (the R9 gate uses five)' }
if ($WarmupSeconds -lt 0 -or $SampleSeconds -lt 1) { throw 'WarmupSeconds must be >= 0 and SampleSeconds must be >= 1' }
if (-not (Test-Path -LiteralPath $SceneManifestPath -PathType Leaf)) {
    throw "Scene manifest does not exist: $SceneManifestPath"
}
$sceneManifest = Get-Content -Raw -LiteralPath $SceneManifestPath | ConvertFrom-Json -ErrorAction Stop
if ($sceneManifest.renderDistance -ne 16) { throw 'R9 scene manifest must specify render distance 16' }
$scenes = @($sceneManifest.scenes)
if ($scenes.Count -ne 8) { throw "R9 scene manifest must contain exactly eight scenes (found $($scenes.Count))" }
$byId = @{}
foreach ($scene in $scenes) { $byId[[string]$scene.id] = $scene }
$normalizedSceneIds = foreach ($id in $SceneIds) {
    $idText = [string]$id
    if ($idText -match '^\d$') { $idText = $idText.PadLeft(2, '0') }
    if (-not $byId.ContainsKey($idText)) { throw "Scene '$idText' is not in $SceneManifestPath" }
    $idText
}

$root = (Resolve-Path (New-Item -ItemType Directory -Force -Path $OutputRoot)).Path
$validator = Join-Path $PSScriptRoot 'Validate-R9Jsonl.ps1'
foreach ($sceneId in $normalizedSceneIds) {
    $scene = $byId[[string]$sceneId]
    for ($rep = 1; $rep -le $Repetitions; $rep++) {
        $dir = Join-Path $root "scene-$sceneId/rep-$rep"
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $jsonl = Join-Path $dir 'frames.jsonl'
        $meta = [ordered]@{
            schema = 'icraft.r9.run.v2'
            sceneId = [string]$scene.id
            sceneName = [string]$scene.name
            seed = [int]$scene.seed
            phase = $Phase
            repetition = $rep
            renderDistance = $RenderDistance
            warmupSeconds = $WarmupSeconds
            sampleSeconds = $SampleSeconds
            capturedUtc = [DateTime]::UtcNow.ToString('o')
            command = $Command
            output = $jsonl
        }
        $meta | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $dir 'run.json') -Encoding utf8
        $args = @(
            '--scene', [string]$scene.id,
            '--seed', [string]$scene.seed,
            '--render-distance', [string]$RenderDistance,
            '--warmup-seconds', [string]$WarmupSeconds,
            '--sample-seconds', [string]$SampleSeconds,
            '--output-jsonl', $jsonl
        )
        if ($DryRun) {
            Write-Output (($Command + ' ' + ($args -join ' ')))
            continue
        }
        $oldScene = $env:R9_SCENE_ID; $oldSeed = $env:R9_SCENE_SEED; $oldPhase = $env:R9_PHASE; $oldOutput = $env:R9_OUTPUT_JSONL
        try {
            $env:R9_SCENE_ID = [string]$scene.id; $env:R9_SCENE_SEED = [string]$scene.seed
            $env:R9_PHASE = $Phase; $env:R9_OUTPUT_JSONL = $jsonl
            & $Command @args
            if ($LASTEXITCODE -ne 0) { throw "Workload failed for scene $sceneId repetition $rep ($LASTEXITCODE)" }
        } finally {
            $env:R9_SCENE_ID = $oldScene; $env:R9_SCENE_SEED = $oldSeed; $env:R9_PHASE = $oldPhase; $env:R9_OUTPUT_JSONL = $oldOutput
        }
        if (-not (Test-Path -LiteralPath $jsonl -PathType Leaf)) {
            throw "Workload did not produce $jsonl"
        }
        & $validator -Path $jsonl -ExpectedSceneId ([string]$scene.id) -ExpectedRepetition $rep | Out-Null
    }
}

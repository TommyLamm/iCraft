[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Path,
    [string]$ExpectedSceneId,
    [int]$ExpectedRepetition = 0,
    [int]$ExpectedRenderDistance = 0
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "JSONL file does not exist: $Path"
}

$required = @(
    'timestampUtc', 'frameIndex', 'sceneId', 'repetition', 'phase',
    'frameTimeMs', 'cpuMs', 'gpuMs', 'workingSetBytes', 'uploadBytes',
    'drawCalls', 'bufferBytes', 'queueDepth', 'queueDelayMs',
    'saveQueueDepth', 'saveQueueDelayMs', 'networkQueueDepth',
    'networkQueueDelayMs', 'checksum'
)
$numeric = @(
    'frameIndex', 'repetition', 'frameTimeMs', 'cpuMs', 'gpuMs',
    'workingSetBytes', 'uploadBytes', 'drawCalls', 'bufferBytes',
    'queueDepth', 'queueDelayMs', 'saveQueueDepth', 'saveQueueDelayMs',
    'networkQueueDepth', 'networkQueueDelayMs'
)
$invariant = [Globalization.CultureInfo]::InvariantCulture
$numberStyle = [Globalization.NumberStyles]::Float
$count = 0
$scene = $null
$lastByRepetition = @{}
$checksums = @{}
$phases = [Collections.Generic.HashSet[string]]::new()

foreach ($line in Get-Content -LiteralPath $Path) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $count++
    try { $record = $line | ConvertFrom-Json -ErrorAction Stop }
    catch { throw "Invalid JSON at line $count in $Path" }
    if ($null -eq $record -or $record -is [array]) {
        throw "JSON line $count is not an object in $Path"
    }

    foreach ($key in $required) {
        $property = $record.PSObject.Properties[$key]
        if ($null -eq $property -or $null -eq $property.Value -or
            ($property.Value -is [string] -and [string]::IsNullOrWhiteSpace([string]$property.Value))) {
            throw "Missing metric '$key' at line $count"
        }
    }

    $timestamp = [DateTime]::MinValue
    if (-not [DateTime]::TryParse([string]$record.timestampUtc, $invariant,
            [Globalization.DateTimeStyles]::RoundtripKind, [ref]$timestamp)) {
        throw "Invalid timestampUtc at line $count"
    }
    if ($null -eq $scene) { $scene = [string]$record.sceneId }
    if ([string]$record.sceneId -ne $scene) { throw "sceneId changes at line $count" }
    if ($ExpectedSceneId -and [string]$record.sceneId -ne $ExpectedSceneId) {
        throw "Expected scene '$ExpectedSceneId', got '$($record.sceneId)' at line $count"
    }

    $parsed = @{}
    foreach ($key in $numeric) {
        $value = 0.0
        if (-not [double]::TryParse([string]$record.$key, $numberStyle, $invariant, [ref]$value) -or
            [double]::IsNaN($value) -or [double]::IsInfinity($value) -or $value -lt 0) {
            throw "Invalid numeric metric '$key' at line $count"
        }
        $parsed[$key] = $value
    }
    if ($parsed.frameIndex -ne [math]::Floor($parsed.frameIndex)) {
        throw "frameIndex must be an integer at line $count"
    }
    if ($parsed.repetition -ne [math]::Floor($parsed.repetition) -or $parsed.repetition -lt 1) {
        throw "repetition must be a positive integer at line $count"
    }
    if ($ExpectedRepetition -gt 0 -and $parsed.repetition -ne $ExpectedRepetition) {
        throw "Expected repetition $ExpectedRepetition, got $($parsed.repetition) at line $count"
    }
    $repKey = [string][int]$parsed.repetition
    if (-not $lastByRepetition.ContainsKey($repKey)) { $lastByRepetition[$repKey] = -1 }
    if ($parsed.frameIndex -le $lastByRepetition[$repKey]) {
        throw "frameIndex must strictly increase within repetition $repKey at line $count"
    }
    $lastByRepetition[$repKey] = $parsed.frameIndex
    $phases.Add([string]$record.phase) | Out-Null
    $checksumKey = "$repKey|$($record.phase)"
    if ($checksums.ContainsKey($checksumKey) -and $checksums[$checksumKey] -ne [string]$record.checksum) {
        throw "checksum changes within repetition $repKey phase '$($record.phase)' at line $count"
    }
    $checksums[$checksumKey] = [string]$record.checksum

    $distanceProperty = $record.PSObject.Properties['renderDistance']
    if ($ExpectedRenderDistance -gt 0 -and $null -ne $distanceProperty -and
        [int]$distanceProperty.Value -ne $ExpectedRenderDistance) {
        throw "Expected render distance $ExpectedRenderDistance at line $count"
    }
}

if ($count -eq 0) { throw "No frames found in $Path" }
if ($ExpectedRenderDistance -gt 0) {
    $distance = (Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -ErrorAction SilentlyContinue).renderDistance
    if ($null -ne $distance -and [int]$distance -ne $ExpectedRenderDistance) {
        throw "Expected render distance $ExpectedRenderDistance, got $distance"
    }
}

[pscustomobject]@{
    schema = 'icraft.r9.validation.v1'
    path = (Resolve-Path -LiteralPath $Path).Path
    sceneId = $scene
    frames = $count
    repetitions = $lastByRepetition.Count
    phases = @($phases | Sort-Object)
    checksums = $checksums
} | ConvertTo-Json -Depth 6

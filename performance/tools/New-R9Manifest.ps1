[CmdletBinding()]
param(
    [string]$OutputPath = 'performance/reports/r9-manifest.json',
    [string]$RepoRoot = '.',
    [string]$SettingsPath,
    [string]$WgpuBackend,
    [string]$Resolution,
    [ValidateSet(16)][int]$RenderDistance = 16
)

$ErrorActionPreference = 'Stop'
function Get-SafeCim([string]$Class) {
    try { @(Get-CimInstance -ClassName $Class -ErrorAction Stop) } catch { @() }
}

$resolvedRepo = (Resolve-Path -LiteralPath $RepoRoot).Path
$gitRepo = $resolvedRepo -replace '\\', '/'
$os = @(Get-SafeCim 'Win32_OperatingSystem' | Select-Object -First 1 Caption, Version, BuildNumber, OSArchitecture)
$cpu = @(Get-SafeCim 'Win32_Processor' | Select-Object -First 1 Name, NumberOfLogicalProcessors, MaxClockSpeed)
$computer = @(Get-SafeCim 'Win32_ComputerSystem' | Select-Object -First 1 TotalPhysicalMemory, Manufacturer, Model)
$gpus = @(Get-SafeCim 'Win32_VideoController' | Select-Object Name, DriverVersion, VideoModeDescription, AdapterRAM)

function Invoke-Git([string[]]$Arguments) {
    try {
        $value = & git @Arguments 2>$null
        if ($LASTEXITCODE -eq 0) { return (($value -join "`n").Trim()) }
    } catch { }
    return $null
}
$gitPrefix = @('-c', "safe.directory=$gitRepo", '-C', $resolvedRepo)
$commit = Invoke-Git ($gitPrefix + @('rev-parse', 'HEAD'))
$dirtyText = Invoke-Git ($gitPrefix + @('status', '--porcelain'))
$dirty = if ($null -eq $dirtyText) { $null } else { -not [string]::IsNullOrWhiteSpace($dirtyText) }

$settings = [ordered]@{}
if ($SettingsPath) {
    if (-not (Test-Path -LiteralPath $SettingsPath -PathType Leaf)) { throw "Settings file does not exist: $SettingsPath" }
    try { $settings = Get-Content -Raw -LiteralPath $SettingsPath | ConvertFrom-Json -ErrorAction Stop }
    catch { throw "Settings file is not valid JSON: $SettingsPath" }
}
# Keep capture-time CLI values alongside the supplied settings. Null means that
# the capture did not provide the value; it is never guessed from a default.
$settingsCapture = [ordered]@{
    wgpuBackend = $WgpuBackend
    resolution = $Resolution
    renderDistance = $RenderDistance
    suppliedSettingsPath = if ($SettingsPath) { (Resolve-Path -LiteralPath $SettingsPath).Path } else { $null }
}
$doc = [ordered]@{
    schema = 'icraft.r9.manifest.v2'
    capturedUtc = [DateTime]::UtcNow.ToString('o')
    host = $env:COMPUTERNAME
    os = $os
    cpu = $cpu
    memory = $computer
    gpu = $gpus
    git = [ordered]@{ commit = $commit; dirty = $dirty; root = $resolvedRepo }
    settings = $settings
    capture = $settingsCapture
    powershell = $PSVersionTable.PSVersion.ToString()
}
$parent = Split-Path -Parent $OutputPath
if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
$doc | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 -LiteralPath $OutputPath
Write-Output (Resolve-Path -LiteralPath $OutputPath)

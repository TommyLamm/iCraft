[CmdletBinding()]
param([string]$OutputPath = "performance/reports/r9-manifest.json", [string]$RepoRoot = ".", [string]$SettingsPath)
$ErrorActionPreference = 'Stop'
function Get-SafeCim([string]$Class) { try { Get-CimInstance -ClassName $Class -ErrorAction Stop } catch { @() } }
$os = @(Get-SafeCim Win32_OperatingSystem | Select-Object -First 1 Caption,Version,BuildNumber,OSArchitecture)
$cpu = @(Get-SafeCim Win32_Processor | Select-Object -First 1 Name,NumberOfLogicalProcessors,MaxClockSpeed)
$gpus = @(Get-SafeCim Win32_VideoController | Select-Object Name,DriverVersion,VideoModeDescription,AdapterRAM)
$git = try { (git -C (Resolve-Path $RepoRoot) rev-parse HEAD 2>$null).Trim() } catch { $null }
$dirty = try { [bool](git -C (Resolve-Path $RepoRoot) status --porcelain 2>$null) } catch { $null }
$settings = if ($SettingsPath -and (Test-Path -LiteralPath $SettingsPath)) { Get-Content -Raw -LiteralPath $SettingsPath | ConvertFrom-Json } else { [ordered]@{} }
$doc = [ordered]@{ schema = 'icraft.r9.manifest.v1'; capturedUtc = [DateTime]::UtcNow.ToString('o'); host = $env:COMPUTERNAME; os = $os; cpu = $cpu; gpu = $gpus; git = [ordered]@{ commit=$git; dirty=$dirty }; settings=$settings; powershell=$PSVersionTable.PSVersion.ToString() }
$parent = Split-Path -Parent $OutputPath; if ($parent) { New-Item -ItemType Directory -Force $parent | Out-Null }
$doc | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 -LiteralPath $OutputPath
Write-Output (Resolve-Path $OutputPath)

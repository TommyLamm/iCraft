[CmdletBinding()]
param([Parameter(Mandatory)][string]$Path)
$ErrorActionPreference='Stop'
$required = 'timestampUtc','frameIndex','sceneId','repetition','phase','frameTimeMs','cpuMs','gpuMs','workingSetBytes','uploadBytes','drawCalls','bufferBytes','queueDepth','queueDelayMs','checksum'
$numeric = 'frameIndex','repetition','frameTimeMs','cpuMs','gpuMs','workingSetBytes','uploadBytes','drawCalls','bufferBytes','queueDepth','queueDelayMs'
$count=0; $last=-1; $checks=@{}
foreach($line in Get-Content -LiteralPath $Path) { if ([string]::IsNullOrWhiteSpace($line)) { continue }; $count++; try { $r=$line|ConvertFrom-Json } catch { throw "Invalid JSON at line $count in $Path" }; foreach($k in $required){ if($null -eq $r.$k -or ($r.$k -is [string] -and [string]::IsNullOrWhiteSpace($r.$k))){ throw "Missing metric '$k' at line $count" } }; foreach($k in $numeric){ $v=0.0; if(-not [double]::TryParse([string]$r.$k,[Globalization.NumberStyles]::Float,[Globalization.CultureInfo]::InvariantCulture,[ref]$v) -or $v -lt 0){ throw "Invalid numeric metric '$k' at line $count" } }; if($r.frameIndex -lt $last){throw "frameIndex is not monotonic at line $count"}; $last=$r.frameIndex; $checks[[string]$r.repetition]=[string]$r.checksum }
if($count -eq 0){throw "No frames found in $Path"}; [pscustomobject]@{path=(Resolve-Path $Path).Path; frames=$count; repetitions=$checks.Count; checksums=$checks} | ConvertTo-Json -Depth 5

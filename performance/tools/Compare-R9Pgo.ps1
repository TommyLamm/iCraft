[CmdletBinding()]
param(
 [Parameter(Mandatory)][string]$BaselineBuildCommand,
 [Parameter(Mandatory)][string]$InstrumentedBuildCommand,
 [Parameter(Mandatory)][string]$OptimizedBuildCommand,
 [Parameter(Mandatory)][string]$WorkloadCommand,
 [string]$OutputRoot='performance/reports/r9-pgo', [string]$LlvmProfdata='llvm-profdata', [int]$Repetitions=5,
 [string]$BaselineRuns, [string]$PgoRuns, [string]$OutputPath='performance/reports/r9-pgo-comparison.json', [switch]$DryRun)
$ErrorActionPreference='Stop'; if($Repetitions -ne 5){throw 'R9 PGO protocol requires exactly five repetitions'}
$root=(Resolve-Path (New-Item -ItemType Directory -Force -Path $OutputRoot)).Path; $profile=Join-Path $root 'profiles'; New-Item -ItemType Directory -Force $profile|Out-Null
$git=try{(git rev-parse HEAD 2>$null).Trim()}catch{$null}; $commands=@(
 [ordered]@{name='baseline-build';command=$BaselineBuildCommand},
 [ordered]@{name='baseline-workload';command=$WorkloadCommand+' --r9-output '+(Join-Path $root 'baseline')},
 [ordered]@{name='instrumented-build';command=$InstrumentedBuildCommand+' -Cprofile-generate='+$profile},
 [ordered]@{name='profile-workload';command='LLVM_PROFILE_FILE="'+(Join-Path $profile 'run-%p-%m.profraw')+'" '+$WorkloadCommand+' --r9-output '+(Join-Path $root 'instrumented')},
 [ordered]@{name='profile-merge';command=$LlvmProfdata+' merge -sparse "'+(Join-Path $profile '*.profraw')+'" -o "'+(Join-Path $profile 'merged.profdata')+'"'},
 [ordered]@{name='optimized-build';command=$OptimizedBuildCommand+' -Cprofile-use='+ (Join-Path $profile 'merged.profdata')+' -Zprofile-use-missing-functions=warn'},
 [ordered]@{name='pgo-workload';command=$WorkloadCommand+' --r9-output '+(Join-Path $root 'pgo')+' --repetitions 5'})
if($DryRun){$commands|%{"$($_.name): $($_.command)"};return}
$lp=Get-Command $LlvmProfdata -ErrorAction SilentlyContinue;if(!$lp -and !(Test-Path -LiteralPath $LlvmProfdata)){throw "llvm-profdata not found: $LlvmProfdata"}
foreach($c in $commands){if($c.name -eq 'profile-merge'){& $LlvmProfdata merge -sparse (Join-Path $profile '*.profraw') -o (Join-Path $profile 'merged.profdata')}else{& $env:ComSpec /d /s /c $c.command;if($LASTEXITCODE -and $LASTEXITCODE -ne 0){throw "Failed: $($c.name)"}}}
$hashes=@{};foreach($c in $commands){$bytes=[Text.Encoding]::UTF8.GetBytes($c.command);$hashes[$c.name]=([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))).ToLowerInvariant()};$result=[ordered]@{schema='icraft.r9.pgo-comparison.v1';patchId=$git;repetitions=5;llvmProfdata=$lp.Source;commands=$commands;commandSha256=$hashes;baselineRuns=$BaselineRuns;pgoRuns=$PgoRuns;acceptance='CPU p50 improvement >=3%; p95/p99 regress <=1%; working set regress <=5%; exact checksum';note='Run Measure-R9Runs.ps1 on baselineRuns and pgoRuns; no result is synthesized'}
$par=Split-Path -Parent $OutputPath;if($par){New-Item -ItemType Directory -Force $par|Out-Null};$result|ConvertTo-Json -Depth 8|Set-Content -Encoding utf8 -LiteralPath $OutputPath


$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskProcess=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.collector)
if($taskProcess){if($taskProcess.Name -ne 'node.exe' -or $taskProcess.CommandLine -notmatch 'live-paper[/\\]runner\.mjs'){throw 'Current collector identity mismatch'}}else{
 $taskLast=Get-Content -LiteralPath (Join-Path $taskRuntime 'collector-lifecycle.jsonl') -Tail 1|ConvertFrom-Json
 if($taskLast.pid -ne $taskPids.collector -or $taskLast.cause -ne 'status_publication_failed' -or $taskLast.errorCode -ne 'EPERM'){throw 'Absent collector cause differs from verified status failure'}
 if(@(Get-CimInstance Win32_Process|Where-Object {$_.Name -eq 'node.exe' -and $_.CommandLine -match 'live-paper[/\\]runner\.mjs'}).Count){throw 'Another collector is active'}
}
$taskActivation=Get-FileHash -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Algorithm SHA256
$taskMarker=Join-Path $taskRuntime 'COLLECTOR_STOP'
foreach($taskStop in @($taskMarker,(Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'EXECUTION_STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'Existing STOP requires provenance review'}}
$taskPayload=@{source='coupled_spread_paper_deployment_20261005';at=[DateTime]::UtcNow.ToString('o');collectorPid=$taskPids.collector;activationChanged=$false}|ConvertTo-Json -Compress
$taskStream=[IO.File]::Open($taskMarker,[IO.FileMode]::CreateNew)
try{$taskBytes=[Text.Encoding]::UTF8.GetBytes($taskPayload);$taskStream.Write($taskBytes,0,$taskBytes.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}
for($taskAttempt=0;$taskAttempt -lt 30;$taskAttempt++){if(!(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 500}
if(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue){throw 'Collector remains alive; no duplicate launched'}
foreach($taskLock in @((Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'),(Join-Path $taskRuntime 'competition-runs/sig-pilot-settlement-20261004-human/execution-owner.lock'))){if(Test-Path -LiteralPath $taskLock){throw 'Collector lease remains'}}
if((Get-Content -LiteralPath $taskMarker -Raw) -ne $taskPayload){throw 'STOP provenance changed'}
& node data/prepare-coupled-spread-live-20261005.mjs
if($LASTEXITCODE -ne 0){throw 'Coupled builder failed; collector remains stopped'}
Move-Item -LiteralPath $taskMarker -Destination ($taskMarker+'.coupled-spread.archived')
if((Get-FileHash -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Algorithm SHA256).Hash -ne $taskActivation.Hash){throw 'Activation changed'}
Copy-Item -LiteralPath (Join-Path $taskRuntime 'collector-detached-20261004.stdout.log') -Destination (Join-Path $taskRuntime 'collector-coupled-spread-before-20261005.stdout.log')
& (Join-Path $taskWorkspace 'Start-BaalServices.ps1')
@{at=[DateTime]::UtcNow.ToString('o');previousCollectorPid=$taskPids.collector;paperPid=$taskPids.paper;dashboardPid=$taskPids.dashboard;activationUnchanged=$true;newPolicyActivated=$false;accountingReset=$false;additionalServices=0}|ConvertTo-Json|Set-Content -LiteralPath 'data/coupled-spread-deployment-20261005.json'

$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPaperDir=Join-Path $taskRuntime 'probability-receipt-cohorts/source-probability-native-20261004-v3'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskActivation=Get-FileHash -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Algorithm SHA256
$taskBank=Get-FileHash -LiteralPath (Join-Path $taskPaperDir 'bank.json') -Algorithm SHA256
$taskCollector=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.collector)
$taskPaper=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.paper)
if(!$taskCollector -or $taskCollector.Name -ne 'node.exe' -or $taskCollector.CommandLine -notmatch 'live-paper[/\\]runner\.mjs' -or !$taskPaper -or $taskPaper.Name -ne 'node.exe' -or $taskPaper.CommandLine -notmatch 'probability-receipt-observe-v3\.mjs'){throw 'Exact current services not verified'}
$taskMarkers=@((Join-Path $taskRuntime 'COLLECTOR_STOP'),(Join-Path $taskPaperDir 'OBSERVER_STOP'))
foreach($taskMarker in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'EXECUTION_STOP'))+$taskMarkers){if(Test-Path -LiteralPath $taskMarker){throw 'Existing STOP requires provenance review'}}
$taskPayload=@{source='calibrated_poll_paper_deploy_20261005';at=[DateTime]::UtcNow.ToString('o');collectorPid=$taskPids.collector;paperPid=$taskPids.paper;activationChanged=$false}|ConvertTo-Json -Compress
foreach($taskMarker in $taskMarkers){$taskStream=[IO.File]::Open($taskMarker,[IO.FileMode]::CreateNew);try{$taskBytes=[Text.Encoding]::UTF8.GetBytes($taskPayload);$taskStream.Write($taskBytes,0,$taskBytes.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}}
for($taskAttempt=0;$taskAttempt -lt 30;$taskAttempt++){if(!(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue) -and !(Get-Process -Id $taskPids.paper -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 500}
if((Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue) -or (Get-Process -Id $taskPids.paper -ErrorAction SilentlyContinue)){throw 'Existing services still alive; no duplicates launched'}
foreach($taskLock in @((Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'),(Join-Path $taskRuntime 'competition-runs/sig-pilot-settlement-20261004-human/execution-owner.lock'),(Join-Path $taskPaperDir 'worker.lock'))){if(Test-Path -LiteralPath $taskLock){throw 'Original service lease remains'}}
foreach($taskMarker in $taskMarkers){if((Get-Content -LiteralPath $taskMarker -Raw) -ne $taskPayload){throw 'STOP provenance changed'};Move-Item -LiteralPath $taskMarker -Destination ($taskMarker+'.calibrated-paper.archived')}
if((Get-FileHash -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Algorithm SHA256).Hash -ne $taskActivation.Hash -or (Get-FileHash -LiteralPath (Join-Path $taskPaperDir 'bank.json') -Algorithm SHA256).Hash -ne $taskBank.Hash){throw 'Activation or frozen prior bank changed'}
& (Join-Path $taskWorkspace 'Start-BaalServices.ps1')
@{at=[DateTime]::UtcNow.ToString('o');stoppedCollectorPid=$taskPids.collector;stoppedPaperPid=$taskPids.paper;activationUnchanged=$true;priorPaperBankUnchanged=$true;additionalAccountClients=0;additionalPersistentServices=0}|ConvertTo-Json|Set-Content -LiteralPath 'data/calibrated-paper-deployment-20261005.json'

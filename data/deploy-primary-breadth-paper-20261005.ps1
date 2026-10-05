$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskMain=Get-Content -LiteralPath (Join-Path $taskRuntime 'probability-receipt-active.json') -Raw|ConvertFrom-Json
$taskPaperDir=Join-Path $taskRuntime ('probability-receipt-cohorts/'+$taskMain.cohort)
$taskActivationFile=Join-Path $taskRuntime 'competition-activation.json'
$taskActivation=(Get-FileHash -LiteralPath $taskActivationFile -Algorithm SHA256).Hash
$taskBanks=@{}
foreach($taskPointerName in @('probability-receipt-active.json','calibrated-poll-paper-active.json','primary-breadth-paper-active.json')){
 $taskPointer=Get-Content -LiteralPath (Join-Path $taskRuntime $taskPointerName) -Raw|ConvertFrom-Json
 $taskDir=Join-Path $taskRuntime ('probability-receipt-cohorts/'+$taskPointer.cohort)
 foreach($taskStop in @('STOP','OBSERVER_STOP')){if(Test-Path -LiteralPath (Join-Path $taskDir $taskStop)){throw 'Existing paper STOP requires provenance review'}}
 $taskBanks[$taskPointer.cohort]=(Get-FileHash -LiteralPath (Join-Path $taskDir 'bank.json') -Algorithm SHA256).Hash
}
$taskCollector=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.collector)
$taskPaper=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.paper)
if(!$taskCollector -or $taskCollector.Name -ne 'node.exe' -or $taskCollector.CommandLine -notmatch 'live-paper[/\\]runner\.mjs' -or !$taskPaper -or $taskPaper.Name -ne 'node.exe' -or $taskPaper.CommandLine -notmatch 'probability-receipt-observe-v3\.mjs'){throw 'Exact current services not verified'}
$taskMarkers=@((Join-Path $taskRuntime 'COLLECTOR_STOP'),(Join-Path $taskPaperDir 'OBSERVER_STOP'))
foreach($taskMarker in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'EXECUTION_STOP'))+$taskMarkers){if(Test-Path -LiteralPath $taskMarker){throw 'Existing STOP requires provenance review'}}
$taskPayload=@{source='primary_breadth_paper_deploy_20261005';at=[DateTime]::UtcNow.ToString('o');collectorPid=$taskPids.collector;paperPid=$taskPids.paper;activationChanged=$false}|ConvertTo-Json -Compress
foreach($taskMarker in $taskMarkers){$taskStream=[IO.File]::Open($taskMarker,[IO.FileMode]::CreateNew);try{$taskBytes=[Text.Encoding]::UTF8.GetBytes($taskPayload);$taskStream.Write($taskBytes,0,$taskBytes.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}}
for($taskAttempt=0;$taskAttempt -lt 40;$taskAttempt++){if(!(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue) -and !(Get-Process -Id $taskPids.paper -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 500}
if((Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue) -or (Get-Process -Id $taskPids.paper -ErrorAction SilentlyContinue)){throw 'Existing services still alive; no duplicates launched'}
foreach($taskLock in @((Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'),(Join-Path $taskRuntime 'competition-runs/sig-pilot-settlement-20261004-human/execution-owner.lock'))){if(Test-Path -LiteralPath $taskLock){throw 'Original service lease remains'}}
foreach($taskCohort in $taskBanks.Keys){if(Test-Path -LiteralPath (Join-Path $taskRuntime ('probability-receipt-cohorts/'+$taskCohort+'/worker.lock'))){throw 'Paper worker lease remains'};if((Get-FileHash -LiteralPath (Join-Path $taskRuntime ('probability-receipt-cohorts/'+$taskCohort+'/bank.json')) -Algorithm SHA256).Hash -ne $taskBanks[$taskCohort]){throw 'Frozen bank changed'}}
if((Get-FileHash -LiteralPath $taskActivationFile -Algorithm SHA256).Hash -ne $taskActivation){throw 'Activation changed'}
foreach($taskMarker in $taskMarkers){if((Get-Content -LiteralPath $taskMarker -Raw) -ne $taskPayload){throw 'STOP provenance changed'};Move-Item -LiteralPath $taskMarker -Destination ($taskMarker+'.primary-breadth-20261005.archived')}
& (Join-Path $taskWorkspace 'Start-BaalServices.ps1')
@{at=[DateTime]::UtcNow.ToString('o');stoppedCollectorPid=$taskPids.collector;stoppedPaperPid=$taskPids.paper;retainedDashboardPid=$taskPids.dashboard;activationUnchanged=$true;bankFileHashes=$taskBanks;additionalAccountClients=0;additionalPersistentServices=0}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath 'data/primary-breadth-paper-deployment-20261005.json'

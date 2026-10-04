$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskMarker=Join-Path $taskRuntime 'COLLECTOR_STOP'
foreach($taskFile in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),$taskMarker)){if(Test-Path -LiteralPath $taskFile){throw 'Existing STOP requires provenance review'}}
$taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if($taskBudget.schemaVersion -ne 3 -or $taskBudget.maxReadsPerMinute -ne 20 -or $taskBudget.writesEnabled -ne $false -or $taskBudget.executionOwner -ne 'Daenerys'){throw 'Read budget or execution ownership changed'}
if((Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json).enabled -ne $false){throw 'Contest activation changed'}
$taskCollector=Get-CimInstance Win32_Process -Filter "ProcessId=$($taskPids.collector)"
$taskPaper=Get-CimInstance Win32_Process -Filter "ProcessId=$($taskPids.paper)"
if(!$taskCollector -or $taskCollector.CommandLine -notlike '*live-paper/runner.mjs*--config live-paper/config.json*' -or !$taskPaper -or $taskPaper.CommandLine -notlike '*probability-receipt-observe.mjs*--name source-probability-20261003-v1*'){throw 'Original process identities changed'}
$taskReason=@{source='probability-owned-risk-20261004';at=[DateTime]::UtcNow.ToString('o');reason='Checked paper accounting continuation; same serialized read budget, no contest writes. Engineering reload, not a human STOP.'}|ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($taskMarker,$taskReason)
$taskDeadline=[DateTime]::UtcNow.AddSeconds(40)
while(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue){if([DateTime]::UtcNow -ge $taskDeadline){throw 'Original collector remains; no duplicate permitted'};Start-Sleep -Milliseconds 400}
if(Test-Path -LiteralPath (Join-Path $taskRuntime '../bot.lock')){throw 'Original collector lease remains'}
$taskOldDir=Join-Path $taskRuntime 'probability-receipt-cohorts/source-probability-20261003-v1'
$taskDeadline=[DateTime]::UtcNow.AddSeconds(15)
do{$taskStats=Get-Content -LiteralPath (Join-Path $taskOldDir 'stats.json') -Raw|ConvertFrom-Json;if($taskStats.sourceClosed){break};if([DateTime]::UtcNow -ge $taskDeadline){throw 'Paper did not consume closed frontier'};Start-Sleep -Milliseconds 400}while($true)
$taskPaperMarker=Join-Path $taskOldDir 'OBSERVER_STOP'
if(Test-Path -LiteralPath $taskPaperMarker){throw 'Original paper STOP exists'}
[System.IO.File]::WriteAllText($taskPaperMarker,$taskReason)
$taskDeadline=[DateTime]::UtcNow.AddSeconds(15)
while(Get-Process -Id $taskPids.paper -ErrorAction SilentlyContinue){if([DateTime]::UtcNow -ge $taskDeadline){throw 'Original paper remains; no duplicate permitted'};Start-Sleep -Milliseconds 300}
if(Test-Path -LiteralPath (Join-Path $taskOldDir 'worker.lock')){throw 'Original paper lease remains'}
node data/prepare-probability-owned-risk-20261004.mjs
if($LASTEXITCODE -ne 0){throw 'Continuation preparation failed; engineering marker retained'}
if([System.IO.File]::ReadAllText($taskMarker) -cne $taskReason){throw 'Engineering marker provenance changed'}
foreach($taskFile in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'))){if(Test-Path -LiteralPath $taskFile){throw 'Human STOP arrived'}}
Remove-Item -LiteralPath $taskMarker
$taskNode=(Get-Command node).Source
$taskNewPaper=Start-Process -FilePath $taskNode -ArgumentList @('live-paper/probability-receipt-observe-v2.mjs','--name','source-probability-owned-risk-20261004-v2') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'paper-owned-risk-20261004.out.log') -RedirectStandardError (Join-Path $taskRuntime 'paper-owned-risk-20261004.err.log')
$taskNewCollector=Start-Process -FilePath $taskNode -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'collector-owned-risk-20261004.out.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-owned-risk-20261004.err.log')
$taskPids.collector=$taskNewCollector.Id;$taskPids.paper=$taskNewPaper.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath 'data/current-runtime-pids.json'
[pscustomobject]@{at=[DateTime]::UtcNow.ToString('o');collectorPid=$taskPids.collector;paperPid=$taskPids.paper;dashboardPid=$taskPids.dashboard;additionalPersistentProcesses=0;competitionWrites=$false}|ConvertTo-Json

$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskBefore=Get-Content -LiteralPath 'data/probability-preservation-before-20261003.json' -Raw|ConvertFrom-Json
foreach($taskName in @('collector','paper','dashboard','metrics')){if($taskPids.$taskName -ne $taskBefore.pids.$taskName -or !(Get-Process -Id $taskPids.$taskName -ErrorAction SilentlyContinue)){throw 'Fleet owner changed'}}
$taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
$taskActivation=Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json
if($taskBudget.schemaVersion -ne 3 -or $taskBudget.maxReadsPerMinute -ne 20 -or $taskBudget.writesEnabled -ne $false -or $taskBudget.executionOwner -ne 'Daenerys' -or $taskActivation.enabled -ne $false){throw 'Read budget or ownership changed'}
function Assert-TaskHumanStop {foreach($taskFlag in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'))){if(Test-Path -LiteralPath $taskFlag){throw 'Human STOP remains'}}}
function Stop-TaskOwner([int]$taskPid,[string]$taskExpected,[string]$taskMarker,[string]$taskReason){
 Assert-TaskHumanStop
 $taskProc=Get-CimInstance Win32_Process -Filter "ProcessId=$taskPid"
 if(!$taskProc -or $taskProc.CommandLine -notlike $taskExpected){throw 'Original process identity changed'}
 $taskHandle=[System.IO.File]::Open($taskMarker,[System.IO.FileMode]::CreateNew,[System.IO.FileAccess]::Write,[System.IO.FileShare]::Read)
 try{$taskBytes=[System.Text.Encoding]::UTF8.GetBytes($taskReason);$taskHandle.Write($taskBytes);$taskHandle.Flush($true)}finally{$taskHandle.Dispose()}
 $taskDeadline=[DateTime]::UtcNow.AddSeconds(40)
 while(Get-Process -Id $taskPid -ErrorAction SilentlyContinue){if([DateTime]::UtcNow -ge $taskDeadline){throw 'Original owner remains live; no duplicate permitted'};Start-Sleep -Milliseconds 500}
 if([System.IO.File]::ReadAllText($taskMarker) -cne $taskReason){throw 'Marker provenance changed'}
 Assert-TaskHumanStop
}
$taskPaperMarker=Join-Path $taskRuntime 'executable-edge-cohorts/executable-edge-20261003-v2/OBSERVER_STOP'
$taskCollectorMarker=Join-Path $taskRuntime 'COLLECTOR_STOP'
Stop-TaskOwner $taskPids.paper '*executable-edge-observe.mjs*--bulk-host-version 2*' $taskPaperMarker 'Codex engineering reload for checked58d7cf2 probability paper companion; preserve frozen original journals. Not a human STOP.'
foreach($taskDir in @('executable-edge-cohorts/executable-edge-20261003-v2','bid-anchor-live-cohorts/bid-anchor-live-20261003-v1','bulk-dip-cohorts/bulk-dip-live-20261003-v2','probability-receipt-cohorts/source-probability-20261003-v1')){if(Test-Path -LiteralPath (Join-Path $taskRuntime "$taskDir/worker.lock")){throw 'Original paper lock remains'}}
Stop-TaskOwner $taskPids.collector '*live-paper/runner.mjs --config live-paper/config.json*' $taskCollectorMarker 'Codex engineering reload for checked58d7cf2 probability focus; same serialized20read budget/no orders. Not a human STOP.'
if(Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock')){throw 'Collector lock remains'}
if(Get-NetTCPConnection -LocalPort 8787 -State Listen -ErrorAction SilentlyContinue){throw 'Collector port remains owned'}
Remove-Item -LiteralPath $taskCollectorMarker
Remove-Item -LiteralPath $taskPaperMarker
$taskNode=(Get-Command node).Source
$taskCollector=Start-Process -FilePath $taskNode -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'collector-probability-20261003.out.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-probability-20261003.err.log') -PassThru
$taskPaper=Start-Process -FilePath $taskNode -ArgumentList @('live-paper/executable-edge-observe.mjs','--name','executable-edge-20261003-v2','--bid-anchor','bid-anchor-live-20261003-v1','--bulk-dip','bulk-dip-live-20261003-v2','--bulk-host-version','2','--probability','source-probability-20261003-v1') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'paper-probability-20261003.out.log') -RedirectStandardError (Join-Path $taskRuntime 'paper-probability-20261003.err.log') -PassThru
$taskPids.collector=$taskCollector.Id
$taskPids.paper=$taskPaper.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath 'data/current-runtime-pids.json'
[pscustomobject]@{at=[DateTime]::UtcNow.ToString('o');pids=$taskPids;cohort='source-probability-20261003-v1';extraBackgroundProcesses=0;newApiClients=0;competitionWrites=$false}|ConvertTo-Json -Depth 3|Tee-Object -FilePath 'data/probability-paper-deployment-20261003.json'

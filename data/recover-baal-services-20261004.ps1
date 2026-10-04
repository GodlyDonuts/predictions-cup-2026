$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPidsFile=Join-Path $taskWorkspace 'data/current-runtime-pids.json'
$taskPids=Get-Content -LiteralPath $taskPidsFile -Raw|ConvertFrom-Json
foreach ($taskStop in @('STOP','COLLECTOR_STOP','PROBABILITY_STOP')) {
  if (Test-Path -LiteralPath (Join-Path $taskRuntime $taskStop)) { throw 'Existing runtime STOP requires provenance review' }
}
if (Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) { throw 'Human workspace STOP present' }
$taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if ($taskBudget.schemaVersion -ne 3 -or !$taskBudget.enabled -or $taskBudget.maxReadsPerMinute -ne 20 -or $taskBudget.writesEnabled -ne $false -or $taskBudget.executionOwner -ne 'Daenerys') { throw 'Budget or ownership changed' }
if ((Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json).enabled -ne $false) { throw 'Contest activation changed' }
$taskRate=Get-Content -LiteralPath (Join-Path $taskRuntime 'rate.json') -Raw|ConvertFrom-Json
if ($taskRate.manualReview) { throw 'Persistent manual review remains; no restart' }
$taskProcesses=@(Get-CimInstance Win32_Process)
if ($taskProcesses | Where-Object { $_.Name -eq 'node.exe' -and $_.CommandLine -match 'live-paper[/\\](runner|probability-receipt-observe)\.mjs|next start .*--port 3000' }) { throw 'Existing bot service; no duplicate start' }
foreach ($taskSavedPid in @($taskPids.collector,$taskPids.paper,$taskPids.dashboard)) {
  if ($taskProcesses | Where-Object ProcessId -eq $taskSavedPid) { throw 'Saved PID reused; inspect identity before recovery' }
}
$taskLease=Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'
$taskWorkerDir=Join-Path $taskRuntime 'probability-receipt-cohorts/source-probability-20261003-v1'
$taskWorkerLock=Join-Path $taskWorkerDir 'worker.lock'
if ((Test-Path -LiteralPath (Join-Path $taskWorkerDir 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskWorkerDir 'OBSERVER_STOP'))) { throw 'Paper worker STOP present' }
$taskRecoveredLocks=@()
foreach ($taskLock in @($taskLease,$taskWorkerLock)) {
  if (!(Test-Path -LiteralPath $taskLock)) { continue }
  $taskLockText=Get-Content -LiteralPath $taskLock -Raw
  $taskOwnerPid=if ($taskLock -eq $taskLease) { [int]$taskLockText } else { [int]($taskLockText|ConvertFrom-Json).pid }
  if ($taskProcesses | Where-Object ProcessId -eq $taskOwnerPid) { throw 'Lease owner remains alive' }
  if ((Get-Content -LiteralPath $taskLock -Raw) -ne $taskLockText) { throw 'Lease changed during recovery' }
  $taskRecoveredLocks+=@{kind=if($taskLock -eq $taskLease){'collector'}else{'paper'};previousPid=$taskOwnerPid}
  Remove-Item -LiteralPath $taskLock
}
if (!(Test-Path -LiteralPath 'data/frontend-build-20261004-baal-pair/.next/BUILD_ID')) { throw 'Verified dashboard build missing' }
if (Get-NetTCPConnection -LocalPort 3000 -State Listen -ErrorAction SilentlyContinue) { throw 'Dashboard port already occupied' }
$taskNode=(Get-Command node).Source
$taskCollector=Start-Process -FilePath $taskNode -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'collector-baal-recovery-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-baal-recovery-20261004.stderr.log')
$taskPids.collector=$taskCollector.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
$taskPaper=Start-Process -FilePath $taskNode -ArgumentList @('live-paper/probability-receipt-observe.mjs','--name','source-probability-20261003-v1') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'paper-baal-recovery-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'paper-baal-recovery-20261004.stderr.log')
$taskPids.paper=$taskPaper.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
$taskDashboard=Start-Process -FilePath $taskNode -ArgumentList @('frontend/node_modules/next/dist/bin/next','start','data/frontend-build-20261004-baal-pair','--hostname','127.0.0.1','--port','3000') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'dashboard-baal-recovery-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'dashboard-baal-recovery-20261004.stderr.log')
$taskPids.dashboard=$taskDashboard.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
@{at=[DateTime]::UtcNow.ToString('o');pids=$taskPids;recoveredLocks=$taskRecoveredLocks;readAdmissionsPerMinute=20;competitionOrders=0;competitionActivation=$false;accountingReset=$false;retiredExperimentStopMarkersRetained=$true;startupOnly=$true}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath 'data/baal-service-recovery-20261004.json'
$taskPids|ConvertTo-Json -Compress

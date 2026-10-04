param([switch]$ResumePreparation,[switch]$Prepared)
$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskStop=Join-Path $taskRuntime 'COLLECTOR_STOP'
if ((Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP')) -or (!$ResumePreparation -and (Test-Path -LiteralPath $taskStop))) { throw 'Existing STOP requires provenance review' }
$taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if ($taskBudget.schemaVersion -ne 3 -or $taskBudget.maxReadsPerMinute -ne 20 -or $taskBudget.writesEnabled -ne $false -or $taskBudget.executionOwner -ne 'Daenerys') { throw 'Shared budget or ownership changed' }
if ((Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json).enabled -ne $false) { throw 'Activation changed' }
$taskPidsFile=Join-Path $taskWorkspace 'data/current-runtime-pids.json'
$taskPids=Get-Content -LiteralPath $taskPidsFile -Raw|ConvertFrom-Json
$taskOldPid=[int]$taskPids.collector
$taskOld=Get-CimInstance Win32_Process -Filter "ProcessId=$taskOldPid"
if ($ResumePreparation) {
 if ($taskOld -or !(Test-Path -LiteralPath $taskStop)) { throw 'Resume requires retired collector and original deployment STOP' }
 $taskMarker=Get-Content -LiteralPath $taskStop -Raw
 $taskMarkerData=$taskMarker|ConvertFrom-Json
 if ($taskMarkerData.source -ne 'pair-anchor-deployment-20261004' -or $taskMarkerData.oldPid -ne $taskOldPid) { throw 'STOP is not this deployment; retained' }
} else {
 if (!$taskOld -or $taskOld.Name -ne 'node.exe' -or $taskOld.CommandLine -notmatch 'live-paper[/\\]runner\.mjs') { throw 'Collector identity changed' }
 $taskMarker=@{source='pair-anchor-deployment-20261004';oldPid=$taskOldPid;at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json -Compress
 [System.IO.File]::WriteAllText($taskStop,$taskMarker)
 for ($taskAttempt=0;$taskAttempt -lt 80;$taskAttempt++) { if (!(Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue)) { break };Start-Sleep -Milliseconds 500 }
}
if (Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue) { throw 'Original collector remains; no duplicate start' }
if ((Get-Content -LiteralPath $taskStop -Raw) -ne $taskMarker) { throw 'STOP provenance changed; retained' }
if ($Prepared -and !$ResumePreparation) { throw 'Prepared recovery requires original deployment STOP' }
if ($Prepared) { node data/check-pair-anchor-prepared-20261004.mjs } else { node data/prepare-pair-anchor-20261004.mjs }
if ($LASTEXITCODE -ne 0) { throw 'Preparation failed; STOP retained' }
if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock')) { throw 'Collector lease remains' }
if ((Get-Content -LiteralPath $taskStop -Raw) -ne $taskMarker) { throw 'STOP provenance changed; retained' }
$taskCurrentBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if (($taskCurrentBudget|ConvertTo-Json -Compress) -ne ($taskBudget|ConvertTo-Json -Compress)) { throw 'Budget changed during preparation' }
if ((Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json).enabled -ne $false) { throw 'Activation changed during preparation' }
Remove-Item -LiteralPath $taskStop
if ((Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP'))) { throw 'Human STOP arrived; no restart' }
$taskNew=Start-Process -FilePath (Get-Command node).Source -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'collector-pair-anchor-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-pair-anchor-20261004.stderr.log')
$taskPids.collector=$taskNew.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
@{at=[DateTime]::UtcNow.ToString('o');retiredCollector=$taskOldPid;collector=$taskNew.Id;paper=$taskPids.paper;dashboard=$taskPids.dashboard;persistentProcesses=3;extraProcesses=0;readAllocation=20;competitionWrites=$false;pairHostRevision=4}|ConvertTo-Json|Set-Content -LiteralPath 'data/pair-anchor-deployment-20261004.json'
@{collector=$taskNew.Id;retired=$taskOldPid;competitionWrites=$false}|ConvertTo-Json -Compress

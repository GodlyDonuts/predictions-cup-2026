$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskStop=Join-Path $taskRuntime 'COLLECTOR_STOP'
if ((Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP')) -or (Test-Path -LiteralPath $taskStop)) { throw 'Existing STOP requires provenance review' }
$taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if ($taskBudget.schemaVersion -ne 3 -or $taskBudget.maxReadsPerMinute -ne 20 -or $taskBudget.writesEnabled -ne $false -or $taskBudget.executionOwner -ne 'Daenerys') { throw 'Shared budget or ownership changed' }
$taskPidsFile=Join-Path $taskWorkspace 'data/current-runtime-pids.json'
$taskPids=Get-Content -LiteralPath $taskPidsFile -Raw|ConvertFrom-Json
$taskOldPid=[int]$taskPids.collector
$taskOld=Get-CimInstance Win32_Process -Filter "ProcessId=$taskOldPid"
if (!$taskOld -or $taskOld.Name -ne 'node.exe' -or $taskOld.CommandLine -notmatch 'live-paper[/\\]runner\.mjs') { throw 'Collector identity changed' }
$taskMarker=@{source='nh-paper-v5-deployment-20261004';oldPid=$taskOldPid;at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($taskStop,$taskMarker)
for ($taskAttempt=0;$taskAttempt -lt 80;$taskAttempt++) { if (!(Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue)) { break };Start-Sleep -Milliseconds 500 }
if (Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue) { throw 'Original collector remains; no duplicate start' }
if ((Get-Content -LiteralPath $taskStop -Raw) -ne $taskMarker) { throw 'STOP provenance changed; retained' }
node data/prepare-nh-paper-v5-20261004.mjs
if ($LASTEXITCODE -ne 0) { throw 'Preparation failed; STOP retained for recovery' }
if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock')) { throw 'Collector lease remains' }
if ((Get-Content -LiteralPath $taskStop -Raw) -ne $taskMarker) { throw 'STOP provenance changed; retained' }
Remove-Item -LiteralPath $taskStop
if ((Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP'))) { throw 'Human STOP arrived; no restart' }
$taskNew=Start-Process -FilePath (Get-Command node).Source -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'collector-nh-v5-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-nh-v5-20261004.stderr.log')
$taskPids.collector=$taskNew.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
@{at=[DateTime]::UtcNow.ToString('o');retiredCollector=$taskOldPid;collector=$taskNew.Id;paper=$taskPids.paper;dashboard=$taskPids.dashboard;persistentProcesses=3;extraProcesses=0;readAllocation=20;competitionWrites=$false;maker='no-maker-tape-20261004-v5';pair='baal-no-pair-20261004-v3'}|ConvertTo-Json|Set-Content -LiteralPath 'data/nh-paper-v5-deployment-20261004.json'
@{collector=$taskNew.Id;retired=$taskOldPid;competitionWrites=$false}|ConvertTo-Json -Compress

$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskStop=Join-Path $taskRuntime 'COLLECTOR_STOP'
foreach ($taskPath in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),$taskStop)) { if (Test-Path -LiteralPath $taskPath) { throw 'Existing STOP retained; provenance review required' } }
$taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if ($taskBudget.schemaVersion -ne 3 -or $taskBudget.maxReadsPerMinute -ne 20 -or $taskBudget.writesEnabled -ne $false -or $taskBudget.executionOwner -ne 'Daenerys') { throw 'Budget or ownership changed' }
if ((Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json).enabled -ne $false) { throw 'Activation changed' }
$taskChecks=Get-Content -LiteralPath 'data/native-intent-all-checks-20261004.tap' -Raw
if ($taskChecks -notmatch 'pass 171' -or $taskChecks -notmatch 'fail 0' -or $taskChecks -notmatch 'skipped 0') { throw 'Correctness checks have not passed' }
$taskEvidence=Get-Content -LiteralPath 'data/native-intent-preparation-20261004.json' -Raw|ConvertFrom-Json
if ($taskEvidence.originalScorecardsExact -ne 100 -or $taskEvidence.observedCachedScorecardsExact -ne 100 -or $taskEvidence.frozenTradingEngineUnchanged -ne $true) { throw 'Source parity required' }
foreach ($taskCode in $taskEvidence.hostSourceHashes.PSObject.Properties) { if ((Get-FileHash -LiteralPath $taskCode.Name -Algorithm SHA256).Hash.ToLowerInvariant() -ne $taskCode.Value) { throw 'Checked source changed' } }
$taskPidsFile=Join-Path $taskWorkspace 'data/current-runtime-pids.json'
$taskPids=Get-Content -LiteralPath $taskPidsFile -Raw|ConvertFrom-Json
$taskOldPid=[int]$taskPids.collector
$taskOld=Get-CimInstance Win32_Process -Filter "ProcessId=$taskOldPid"
if (!$taskOld -or $taskOld.Name -ne 'node.exe' -or $taskOld.CommandLine -notmatch 'live-paper[/\\]runner\.mjs') { throw 'Collector identity changed' }
node data/native-intent-preservation-20261004.mjs before
if ($LASTEXITCODE -ne 0) { throw 'Preservation snapshot failed' }
$taskMarker=@{source='native-intent-deployment-20261004';oldPid=$taskOldPid;at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($taskStop,$taskMarker)
for ($taskAttempt=0;$taskAttempt -lt 80;$taskAttempt++) { if (!(Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue)) { break };Start-Sleep -Milliseconds 500 }
if (Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue) { throw 'Collector remains; no duplicate start' }
if ((Get-Content -LiteralPath $taskStop -Raw) -ne $taskMarker) { throw 'STOP provenance changed; retained' }
if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock')) { throw 'Collector lease remains' }
node data/native-intent-preservation-20261004.mjs after
if ($LASTEXITCODE -ne 0) { throw 'Original accounting changed; STOP retained' }
$taskCurrentBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if (($taskCurrentBudget|ConvertTo-Json -Compress) -ne ($taskBudget|ConvertTo-Json -Compress)) { throw 'Budget changed; STOP retained' }
if ((Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json).enabled -ne $false) { throw 'Activation changed; STOP retained' }
Remove-Item -LiteralPath $taskStop
if ((Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP'))) { throw 'Human STOP arrived; no restart' }
$taskNew=Start-Process -FilePath (Get-Command node).Source -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'collector-native-intent-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-native-intent-20261004.stderr.log')
$taskPids.collector=$taskNew.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
@{at=[DateTime]::UtcNow.ToString('o');retiredCollector=$taskOldPid;collector=$taskNew.Id;paper=$taskPids.paper;dashboard=$taskPids.dashboard;persistentProcesses=3;extraProcesses=0;readAllocation=20;competitionWrites=$false;nativeHostRevision=3}|ConvertTo-Json|Set-Content -LiteralPath 'data/native-intent-deployment-20261004.json'
@{collector=$taskNew.Id;retired=$taskOldPid;competitionWrites=$false}|ConvertTo-Json -Compress

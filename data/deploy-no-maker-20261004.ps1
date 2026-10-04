$ErrorActionPreference = 'Stop'
$taskWorkspace = (Get-Location).Path
$taskRuntime = Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'
$taskStop = Join-Path $taskRuntime 'COLLECTOR_STOP'
if ((Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP')) -or (Test-Path -LiteralPath $taskStop)) { throw 'Existing STOP requires provenance review; no restart performed' }
$taskPidsFile = Join-Path $taskWorkspace 'data\current-runtime-pids.json'
$taskPids = Get-Content -LiteralPath $taskPidsFile -Raw | ConvertFrom-Json
$taskOldPid = [int]$taskPids.collector
$taskOld = Get-CimInstance Win32_Process -Filter "ProcessId=$taskOldPid"
if (!$taskOld -or $taskOld.CommandLine -notmatch 'live-paper[/\\]runner\.mjs' -or $taskOld.Name -ne 'node.exe') { throw 'Exact collector PID is not owned by this deployment' }
$taskMarker = @{source='no-maker-deployment-20261004';oldPid=$taskOldPid;at=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($taskStop,$taskMarker)
for ($taskAttempt=0; $taskAttempt -lt 20; $taskAttempt++) { if (!(Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue)) { break }; Start-Sleep -Milliseconds 500 }
if (Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue) { throw 'Existing collector still alive; no second collector started' }
if ((Get-Content -LiteralPath $taskStop -Raw) -ne $taskMarker) { throw 'STOP provenance changed; marker retained' }
if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup\bot.lock')) { throw 'Collector lease remains; inspect before restarting' }
Remove-Item -LiteralPath $taskStop
if ((Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')) -or (Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP'))) { throw 'Human STOP arrived; no restart performed' }
$taskNode = (Get-Command node).Source
$taskNew = Start-Process -FilePath $taskNode -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'collector-no-maker-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-no-maker-20261004.stderr.log')
$taskPids.collector = $taskNew.Id
$taskPids | ConvertTo-Json | Set-Content -LiteralPath $taskPidsFile
@{at=[DateTime]::UtcNow.ToString('o');retiredCollector=$taskOldPid;collector=$taskNew.Id;paper=$taskPids.paper;dashboard=$taskPids.dashboard;metrics=$taskPids.metrics;addedProcesses=0;readAllocation=20;competitionWrites=$false} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskWorkspace 'data\no-maker-deployment-20261004.json')
@{retiredCollector=$taskOldPid;collector=$taskNew.Id;extraBackgroundProcesses=0;competitionWrites=$false} | ConvertTo-Json -Compress

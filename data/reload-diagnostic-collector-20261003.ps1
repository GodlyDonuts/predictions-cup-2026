$ErrorActionPreference='Stop'
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskOld=18392
$taskProcess=Get-CimInstance Win32_Process -Filter "ProcessId=$taskOld"
if(!$taskProcess -or $taskProcess.CommandLine -notlike '*live-paper/runner.mjs --config live-paper/config.json*'){throw 'Expected collector owner changed'}
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
if($taskPids.collector -ne $taskOld){throw 'Collector PID record changed'}
foreach($taskOther in @($taskPids.paper,$taskPids.dashboard,$taskPids.metrics)){if(!(Get-Process -Id $taskOther -ErrorAction SilentlyContinue)){throw 'Existing fleet member missing'}}
$taskActivation=Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json
$taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
if($taskActivation.enabled -ne $false -or $taskBudget.schemaVersion -ne 3 -or $taskBudget.maxReadsPerMinute -ne 20 -or $taskBudget.writesEnabled -ne $false -or $taskBudget.executionOwner -ne 'Daenerys'){throw 'Execution ownership or read budget changed'}
foreach($taskHumanStop in @((Join-Path (Get-Location).Path 'STOP'),(Join-Path $taskRuntime 'STOP'))){if(Test-Path -LiteralPath $taskHumanStop){throw 'Human STOP remains active'}}
$taskStop=Join-Path $taskRuntime 'COLLECTOR_STOP'
$taskProvenance='Codex engineering reload of tested diagnostic read focus. Same serialized20read budget; no orders, new collector or paper reset. Not a human STOP. 20261003.'
$taskHandle=[System.IO.File]::Open($taskStop,[System.IO.FileMode]::CreateNew,[System.IO.FileAccess]::Write,[System.IO.FileShare]::Read)
try{$taskBytes=[System.Text.Encoding]::UTF8.GetBytes($taskProvenance);$taskHandle.Write($taskBytes);$taskHandle.Flush($true)}finally{$taskHandle.Dispose()}
$taskDeadline=[DateTime]::UtcNow.AddSeconds(40)
while(Get-Process -Id $taskOld -ErrorAction SilentlyContinue){if([DateTime]::UtcNow -ge $taskDeadline){throw 'Collector remains live; do not duplicate'};Start-Sleep -Milliseconds 500}
if(Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock')){throw 'Collector lock remains'}
if(Get-NetTCPConnection -LocalPort 8787 -State Listen -ErrorAction SilentlyContinue){throw 'Collector port remains owned'}
if([System.IO.File]::ReadAllText($taskStop) -cne $taskProvenance){throw 'STOP provenance changed'}
foreach($taskHumanStop in @((Join-Path (Get-Location).Path 'STOP'),(Join-Path $taskRuntime 'STOP'))){if(Test-Path -LiteralPath $taskHumanStop){throw 'Human STOP arrived'}}
Remove-Item -LiteralPath $taskStop
$taskNew=Start-Process -FilePath (Get-Command node).Source -ArgumentList @('live-paper/runner.mjs','--config','live-paper/config.json') -WorkingDirectory (Get-Location).Path -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'collector-diagnostic-20261003.out.log') -RedirectStandardError (Join-Path $taskRuntime 'collector-diagnostic-20261003.err.log') -PassThru
$taskPids.collector=$taskNew.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath 'data/current-runtime-pids.json'
[pscustomobject]@{at=[DateTime]::UtcNow.ToString('o');retiredPid=$taskOld;collectorPid=$taskNew.Id;paperPid=$taskPids.paper;dashboardPid=$taskPids.dashboard;metricsPid=$taskPids.metrics;extraBackgroundProcesses=0;competitionWrites=$false}|ConvertTo-Json|Tee-Object -FilePath 'data/diagnostic-collector-reload-20261003.json'

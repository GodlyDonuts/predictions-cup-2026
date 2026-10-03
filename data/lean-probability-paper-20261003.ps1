$ErrorActionPreference='Stop'
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskOld=$taskPids.paper
$taskProc=Get-CimInstance Win32_Process -Filter "ProcessId=$taskOld"
if(!$taskProc -or $taskProc.CommandLine -notlike '*executable-edge-observe.mjs*--probability source-probability-20261003-v1*'){throw 'Paper owner identity changed'}
foreach($taskFlag in @((Join-Path (Get-Location).Path 'STOP'),(Join-Path $taskRuntime 'STOP'))){if(Test-Path -LiteralPath $taskFlag){throw 'Human STOP remains'}}
$taskActivation=Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json
if($taskActivation.enabled -ne $false){throw 'Contest activation changed'}
$taskDirs=@('executable-edge-cohorts/executable-edge-20261003-v2','bid-anchor-live-cohorts/bid-anchor-live-20261003-v1','bulk-dip-cohorts/bulk-dip-live-20261003-v2','probability-receipt-cohorts/source-probability-20261003-v1')
# This exact owned process is only replaying/simulating local paper journals.
# Stop its costly synchronous bootstrap; retain every journal and owned lease.
Stop-Process -Id $taskOld
$taskDeadline=[DateTime]::UtcNow.AddSeconds(10)
while(Get-Process -Id $taskOld -ErrorAction SilentlyContinue){if([DateTime]::UtcNow -ge $taskDeadline){throw 'Original paper remains live'};Start-Sleep -Milliseconds 200}
foreach($taskDir in $taskDirs){$taskLock=Join-Path $taskRuntime "$taskDir/worker.lock";if(Test-Path -LiteralPath $taskLock){$taskOwner=Get-Content -LiteralPath $taskLock -Raw|ConvertFrom-Json;if($taskOwner.pid -ne $taskOld){throw 'Foreign paper lease encountered'};$taskArchive=Join-Path $taskRuntime "$taskDir/retired-bootstrap-lease-20261003.json";if(Test-Path -LiteralPath $taskArchive){throw 'Lease archive already exists'};Move-Item -LiteralPath $taskLock -Destination $taskArchive}}
$taskNode=(Get-Command node).Source
$taskNew=Start-Process -FilePath $taskNode -ArgumentList @('live-paper/probability-receipt-observe.mjs','--name','source-probability-20261003-v1') -WorkingDirectory (Get-Location).Path -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'paper-probability-lean-20261003.out.log') -RedirectStandardError (Join-Path $taskRuntime 'paper-probability-lean-20261003.err.log') -PassThru
$taskPids.paper=$taskNew.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath 'data/current-runtime-pids.json'
[pscustomobject]@{at=[DateTime]::UtcNow.ToString('o');retiredPid=$taskOld;paperPid=$taskNew.Id;retiredFrozenCohorts=3;activePaperEngine='source-probability-20261003-v1';journalRemoval=$false;bankModification=$false;extraBackgroundProcesses=0;competitionWrites=$false}|ConvertTo-Json|Tee-Object -FilePath 'data/probability-lean-deployment-20261003.json'

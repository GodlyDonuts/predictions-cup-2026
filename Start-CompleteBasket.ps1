param([string]$Name='basket-complete-20261002-v1',[switch]$Initialize,[switch]$KeepSession)
$ErrorActionPreference='Stop'
if($Name -notmatch '^[a-z0-9-]{1,80}$'){throw 'Invalid cohort name'}
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'
$taskCohort=Join-Path $taskRuntime "basket-complete-pair-cohorts\$Name"
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'BASKET_STOP'),(Join-Path $taskRuntime 'COLLECTOR_STOP'),(Join-Path $taskCohort 'STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskCohort 'worker.lock')){throw 'Observer lock exists; inspect owning process'}
if($Initialize){& (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper\basket-complete-watch.mjs') $Name --init --once;if($LASTEXITCODE -ne 0){throw 'Initialization failed'}}
if(!(Test-Path -LiteralPath (Join-Path $taskCohort 'bank.json'))){throw 'Initialize a new prospective cohort first'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/basket-complete-watch.mjs',$Name) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskCohort 'stdout.log') -RedirectStandardError (Join-Path $taskCohort 'stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskCohort 'pid.txt')
Write-Output "Complete-pair observer PID $($taskProcess.Id); no API calls or orders"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Observer failed; inspect restricted status'}}

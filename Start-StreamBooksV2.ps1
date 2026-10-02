param([Parameter(Mandatory=$true)][string]$Name,[switch]$Initialize,[switch]$KeepSession)
$ErrorActionPreference='Stop'
if($Name -notmatch '^[a-z][a-z0-9-]{0,63}$' -or $Name -match '^(con|prn|aux|nul|com[1-9]|lpt[1-9])$'){throw 'Invalid stream input cohort name'}
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'}
$taskCohort=Join-Path $taskRuntime ('stream-book-v2-cohorts/'+$Name)
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'STREAM_BOOK_V2_STOP'),(Join-Path $taskCohort 'STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect provenance before restarting'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'stream-book-gateway.lock')){throw 'Stream gateway lock exists; inspect its owner before restarting'}
if($Initialize){
 & (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper/stream-book-watch-v2.mjs') --name $Name --init --once
 if($LASTEXITCODE -ne 0){throw 'Stream input initialization failed'}
}
if(!(Test-Path -LiteralPath (Join-Path $taskCohort 'bank.json'))){throw 'Initialize a new frozen stream input cohort first'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/stream-book-watch-v2.mjs','--name',$Name) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskCohort 'stdout.log') -RedirectStandardError (Join-Path $taskCohort 'stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskCohort 'pid.txt')
Write-Output "Stream input PID $($taskProcess.Id); cohort $Name; existing capture only; no orders or API requests"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Stream input exited; inspect restricted local status'}}

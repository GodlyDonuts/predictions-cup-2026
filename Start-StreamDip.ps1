param([Parameter(Mandatory=$true)][string]$Name,[string]$InputCohort='stream-books-20261002',[switch]$Initialize,[switch]$KeepSession)
$ErrorActionPreference='Stop'
foreach($taskName in @($Name,$InputCohort)){if($taskName -notmatch '^[a-z][a-z0-9-]{0,63}$' -or $taskName -match '^(con|prn|aux|nul|com[1-9]|lpt[1-9])$'){throw 'Invalid stream dip cohort name'}}
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'}
$taskCohort=Join-Path $taskRuntime ('stream-dip-cohorts/'+$Name)
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'STREAM_DIP_STOP'),(Join-Path $taskCohort 'STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect provenance before restarting'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'stream-dip.lock')){throw 'Stream dip lock exists; inspect its owner before restarting'}
if($Initialize){
 & (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper/stream-dip-watch.mjs') --name $Name --input $InputCohort --init --once
 if($LASTEXITCODE -ne 0){throw 'Stream dip initialization failed'}
}
if(!(Test-Path -LiteralPath (Join-Path $taskCohort 'bank.json'))){throw 'Initialize a new frozen stream dip cohort first'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/stream-dip-watch.mjs','--name',$Name) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskCohort 'stdout.log') -RedirectStandardError (Join-Path $taskCohort 'stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskCohort 'pid.txt')
Write-Output "Stream dip PID $($taskProcess.Id); cohort $Name; paper wallets only; no orders or API requests"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Stream dip exited; inspect restricted local status'}}

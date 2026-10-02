param([Parameter(Mandatory=$true)][string]$Name,[switch]$Initialize,[switch]$KeepSession)
$ErrorActionPreference='Stop'
if($Name -notmatch '^[a-z][a-z0-9-]{0,63}$' -or $Name -match '^(con|prn|aux|nul|com[1-9]|lpt[1-9])$'){throw 'Invalid limit-entry cohort name'}
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'}
$taskCohort=Join-Path $taskRuntime ('limit-entry-cohorts/'+$Name)
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'LIMIT_ENTRY_STOP'),(Join-Path $taskCohort 'STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect provenance before restarting'}}
if(Test-Path -LiteralPath (Join-Path $taskCohort 'worker.lock')){throw 'Limit-entry lock exists; inspect its owning process'}
if($Initialize){
 & (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper/limit-entry-watch.mjs') --name $Name --init --once
 if($LASTEXITCODE -ne 0){throw 'Limit-entry initialization failed'}
}
if(!(Test-Path -LiteralPath (Join-Path $taskCohort 'bank.json'))){throw 'Use -Initialize to create a new frozen cohort'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/limit-entry-watch.mjs','--name',$Name) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskCohort 'stdout.log') -RedirectStandardError (Join-Path $taskCohort 'stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskCohort 'pid.txt')
Write-Output "Limit-entry PID $($taskProcess.Id); cohort $Name; zero API requests; quote-conditioned hypothetical accounting only"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Limit-entry worker stopped; inspect local status'}}

param([Parameter(Mandatory=$true)][string]$Name,[switch]$Initialize,[switch]$KeepSession)
$ErrorActionPreference='Stop'
if($Name -notmatch '^[a-z][a-z0-9-]{0,63}$' -or $Name -match '^(con|prn|aux|nul|com[1-9]|lpt[1-9])$'){throw 'Invalid comparison name'}
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'}
$taskCohort=Join-Path $taskRuntime ('entry-comparisons/'+$Name)
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'ENTRY_COMPARISON_STOP'),(Join-Path $taskCohort 'STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect provenance before restarting'}}
if(Test-Path -LiteralPath (Join-Path $taskCohort 'worker.lock')){throw 'Comparison lock exists; inspect its owning process'}
if($Initialize){
 & (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper/entry-comparison-watch.mjs') --name $Name --init --once
 if($LASTEXITCODE -ne 0){throw 'Comparison initialization failed'}
}
if(!(Test-Path -LiteralPath (Join-Path $taskCohort 'bank.json'))){throw 'Use -Initialize to create a new frozen comparison'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/entry-comparison-watch.mjs','--name',$Name) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskCohort 'stdout.log') -RedirectStandardError (Join-Path $taskCohort 'stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskCohort 'pid.txt')
Write-Output "Entry comparison PID $($taskProcess.Id); cohort $Name; zero API requests and local counterfactual wallets only"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Comparison stopped; inspect local status'}}

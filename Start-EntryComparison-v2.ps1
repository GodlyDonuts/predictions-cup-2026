param([string]$Name='entry-common-20261002',[switch]$KeepSession)
$ErrorActionPreference='Stop'
if($Name -notmatch '^[a-z][a-z0-9-]{0,63}$' -or $Name -match '^(con|prn|aux|nul|com[1-9]|lpt[1-9])$'){throw 'Invalid cohort name'}
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskCohort=Join-Path $taskRuntime "entry-comparisons/$Name"
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'ENTRY_COMPARISON_STOP'),(Join-Path $taskCohort 'STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskCohort 'worker.lock')){throw 'Comparison lock remains; inspect owning process'}
if(!(Test-Path -LiteralPath (Join-Path $taskCohort 'bank.json'))){throw 'Existing frozen bank required'}
$taskPrior=Get-Content -LiteralPath (Join-Path $taskCohort 'stats.json') -Raw | ConvertFrom-Json
if($taskPrior.status -ne 'failed'){throw 'Recovery requires a verified failed prior host'}
$taskPriorPid=if($taskPrior.pid){[int]$taskPrior.pid}else{[int](Get-Content -LiteralPath (Join-Path $taskCohort 'pid.txt'))}
if(Get-Process -Id $taskPriorPid -ErrorAction SilentlyContinue){throw 'Prior host is still alive'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/entry-comparison-watch-v2.mjs','--name',$Name) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskCohort 'v2-stdout.log') -RedirectStandardError (Join-Path $taskCohort 'v2-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskCohort 'pid.txt')
Write-Output "Streaming comparison recovery PID $($taskProcess.Id); existing bank and journal; zero API requests"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Comparison recovery failed; inspect restricted status'}}

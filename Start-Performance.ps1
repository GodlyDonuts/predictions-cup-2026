param([switch]$KeepSession)
$ErrorActionPreference='Stop'
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'PERFORMANCE_STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect its provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'performance-watch.lock')){throw 'Performance lock exists; inspect its process'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('execution/performance-watch.mjs') -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'performance-stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'performance-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'performance-pid.txt')
Write-Output "Read-only official performance PID $($taskProcess.Id)"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Performance worker stopped; inspect local status'}}

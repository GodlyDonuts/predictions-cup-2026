$ErrorActionPreference='Stop'
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'FEE_FREE_STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect its provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'fee-free-shadow.lock')){throw 'Fee-free worker lock exists; inspect its process'}
if(!(Test-Path -LiteralPath (Join-Path $taskRuntime 'fee-free-bank.json'))){throw 'Initialize a frozen fee-free bank first'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/fee-free-shadow.mjs') -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'fee-free-stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'fee-free-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'fee-free-pid.txt')
Write-Output "Fee-free paper worker PID $($taskProcess.Id); zero API requests"

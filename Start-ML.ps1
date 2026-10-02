$ErrorActionPreference='Stop'
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
if(Test-Path -LiteralPath (Join-Path $PSScriptRoot 'STOP')){throw 'Workspace STOP exists'}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP')){throw 'Paper STOP exists'}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'ML_STOP')){throw 'ML_STOP exists; inspect its reason before restarting'}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'ml-shadow.lock')){throw 'ML worker lock exists; inspect its process before restarting'}
if(!(Test-Path -LiteralPath (Join-Path $taskRuntime 'ml-bank.json'))){throw 'Train a model bank first'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/ml-shadow.mjs') -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'ml-stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'ml-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'ml-pid.txt')
Write-Output "ML paper worker PID $($taskProcess.Id); no market API requests"

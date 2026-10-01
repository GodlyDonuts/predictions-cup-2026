param([string]$ConfigPath='live-paper/config.json')
$ErrorActionPreference='Stop'
$taskRoot=$PSScriptRoot
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
if(Test-Path -LiteralPath (Join-Path $taskRoot 'STOP')){throw 'Workspace STOP exists. Review its reason before restarting.'}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'STOP')){throw 'Paper STOP exists. Remove it only for an intended restart.'}
New-Item -ItemType Directory -Path $taskRuntime -Force | Out-Null
$taskIdentity=[Security.Principal.WindowsIdentity]::GetCurrent().Name
& icacls.exe $taskRuntime /inheritance:r /grant:r "${taskIdentity}:(OI)(CI)F" '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
if($LASTEXITCODE -ne 0){throw 'Could not restrict runtime directory permissions'}
$taskConfig=(Resolve-Path -LiteralPath $ConfigPath).Path
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/runner.mjs','--config',('"'+$taskConfig+'"')) -WorkingDirectory $taskRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'pid.txt')
Write-Output "Paper runner PID $($taskProcess.Id); dashboard http://127.0.0.1:8787"

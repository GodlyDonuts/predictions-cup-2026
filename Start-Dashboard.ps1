param([switch]$KeepSession)
$ErrorActionPreference='Stop'
$taskFrontend=Join-Path $PSScriptRoot 'frontend'
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'
if(!(Test-Path -LiteralPath (Join-Path $taskFrontend '.next\BUILD_ID'))){throw 'Build the frontend with pnpm build first'}
if(Get-NetTCPConnection -LocalPort 3000 -State Listen -ErrorAction SilentlyContinue){throw 'Port 3000 already has a listener; inspect it before starting another dashboard'}
$taskNext=Join-Path $taskFrontend 'node_modules\next\dist\bin\next'
if(!(Test-Path -LiteralPath $taskNext)){throw 'Install frontend dependencies first'}
New-Item -ItemType Directory -Path $taskRuntime -Force | Out-Null
if(!(Test-Path -LiteralPath (Join-Path $taskRuntime 'official-sync.json'))){'{"enabled":true,"purpose":"human-authorized local official-account display"}' | Set-Content -LiteralPath (Join-Path $taskRuntime 'official-sync.json')}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @(('"'+$taskNext+'"'),'start','--hostname','127.0.0.1','--port','3000') -WorkingDirectory $taskFrontend -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'frontend-stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'frontend-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'frontend-pid.txt')
Write-Output "Paper dashboard PID $($taskProcess.Id); http://127.0.0.1:3000"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Dashboard exited unsuccessfully; inspect local log'}}

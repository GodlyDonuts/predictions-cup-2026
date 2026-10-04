$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPidsFile=Join-Path $taskWorkspace 'data/current-runtime-pids.json'
$taskPids=Get-Content -LiteralPath $taskPidsFile -Raw|ConvertFrom-Json
$taskOldPid=[int]$taskPids.dashboard
$taskOld=Get-CimInstance Win32_Process -Filter "ProcessId=$taskOldPid"
if (!$taskOld -or $taskOld.Name -ne 'node.exe' -or $taskOld.CommandLine -notmatch 'next start data/frontend-build-20261004-baal-final --hostname 127.0.0.1 --port 3000') { throw 'Dashboard identity changed' }
if (!(Test-Path -LiteralPath 'data/frontend-build-20261004-baal-pair/.next/BUILD_ID')) { throw 'Checked production build missing' }
Stop-Process -Id $taskOldPid
for ($taskAttempt=0;$taskAttempt -lt 20;$taskAttempt++) { if (!(Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue)) { break };Start-Sleep -Milliseconds 100 }
if (Get-Process -Id $taskOldPid -ErrorAction SilentlyContinue) { throw 'Original dashboard remains' }
$taskNew=Start-Process -FilePath (Get-Command node).Source -ArgumentList @('frontend/node_modules/next/dist/bin/next','start','data/frontend-build-20261004-baal-pair','--hostname','127.0.0.1','--port','3000') -WorkingDirectory $taskWorkspace -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRuntime 'dashboard-baal-pair-20261004.stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'dashboard-baal-pair-20261004.stderr.log')
$taskPids.dashboard=$taskNew.Id
$taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
@{at=[DateTime]::UtcNow.ToString('o');displayName='Baal';dashboardPid=$taskNew.Id;retiredPid=$taskOldPid;productionBuild='frontend-build-20261004-baal-pair';extraPersistentProcesses=0}|ConvertTo-Json|Set-Content -LiteralPath 'data/baal-pair-dashboard-deployment-20261004.json'
@{displayName='Baal';dashboardPid=$taskNew.Id;retiredPid=$taskOldPid}|ConvertTo-Json -Compress

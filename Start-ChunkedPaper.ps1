param([Parameter(Mandatory=$true)][ValidateSet('dip','dip-scale','dip-responsive','fee-free','ml','shock','basket')][string]$Name,[switch]$KeepSession)
$ErrorActionPreference='Stop'
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
foreach($taskStop in @('STOP',($Name -like 'dip*'?'DIP_STOP':$Name -eq 'fee-free'?'FEE_FREE_STOP':$Name -eq 'ml'?'ML_STOP':$Name -eq 'shock'?'SHOCK_STOP':'BASKET_STOP'))){if(Test-Path -LiteralPath (Join-Path $taskRuntime $taskStop)){throw 'Paper STOP exists; review provenance'}}
if(Test-Path -LiteralPath (Join-Path $PSScriptRoot 'STOP')){throw 'Workspace STOP exists'}
$taskLock=Join-Path $taskRuntime ($Name -eq 'basket'?'basket-watch.lock':$Name+'-shadow.lock')
if(Test-Path -LiteralPath $taskLock){throw 'Prior reader lock remains'}
$taskStats=Join-Path $taskRuntime ($Name+'-stats.json')
$taskPrior=Get-Content -LiteralPath $taskStats -Raw | ConvertFrom-Json
if($taskPrior.status -ne 'failed'){throw 'This recovery launcher requires a verified failed prior reader'}
$taskPriorPid=if($taskPrior.pid){[int]$taskPrior.pid}else{[int](Get-Content -LiteralPath (Join-Path $taskRuntime ($Name+'-pid.txt')))}
if(Get-Process -Id $taskPriorPid -ErrorAction SilentlyContinue){throw 'Prior reader is still alive'}
$taskScript=if($Name -like 'dip*'){'dip-worker-v2'}elseif($Name -eq 'basket'){'basket-watch-v2'}else{$Name+'-shadow-v2'}
$taskArgs=@('live-paper/'+$taskScript+'.mjs');if($Name -like 'dip*'){$taskArgs+=@('--name',$Name)}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList $taskArgs -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime ($Name+'-v2-stdout.log')) -RedirectStandardError (Join-Path $taskRuntime ($Name+'-v2-stderr.log')) -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime ($Name+'-pid.txt'))
Write-Output "Chunked paper reader $Name PID $($taskProcess.Id)"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Chunked paper reader failed; inspect restricted local diagnostics'}}

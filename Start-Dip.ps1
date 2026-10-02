param([ValidateSet('dip','dip-scale')][string]$CohortName='dip')
$ErrorActionPreference='Stop'
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'DIP_STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect its provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime ($CohortName+'-shadow.lock'))){throw 'Dip worker lock exists; inspect its process'}
if(!(Test-Path -LiteralPath (Join-Path $taskRuntime ($CohortName+'-bank.json')))){throw 'Initialize frozen dip bank first'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/dip-shadow.mjs','--name',$CohortName) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime ($CohortName+'-stdout.log')) -RedirectStandardError (Join-Path $taskRuntime ($CohortName+'-stderr.log')) -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime ($CohortName+'-pid.txt'))
Write-Output "Diversified dip paper worker PID $($taskProcess.Id); no API requests"

param([switch]$Initialize)
$ErrorActionPreference='Stop'
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'SHOCK_STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect its provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'shock-shadow.lock')){throw 'Shock worker lock exists; inspect its process'}
if($Initialize){
  & (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper\shock-shadow.mjs') --init --once
  if($LASTEXITCODE -ne 0){throw 'Shock cohort initialization failed'}
}
if(!(Test-Path -LiteralPath (Join-Path $taskRuntime 'shock-bank.json'))){throw 'Use -Initialize to freeze a new prospective shock cohort'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/shock-shadow.mjs') -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'shock-stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'shock-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'shock-pid.txt')
Write-Output "Shock paper worker PID $($taskProcess.Id); zero API requests"

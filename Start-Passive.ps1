param([switch]$Initialize,[switch]$KeepSession)
$ErrorActionPreference='Stop'
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'PASSIVE_STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect its provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'passive-watch.lock')){throw 'Passive observer lock exists; inspect its process'}
if($Initialize){
  & (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper\passive-watch.mjs') --init --once
  if($LASTEXITCODE -ne 0){throw 'Passive observer initialization failed'}
}
if(!(Test-Path -LiteralPath (Join-Path $taskRuntime 'passive-bank.json'))){throw 'Use -Initialize to create a prospective passive observer'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/passive-watch.mjs') -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'passive-stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'passive-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'passive-pid.txt')
Write-Output "Passive opportunity observer PID $($taskProcess.Id); zero API requests and no inferred fills"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Passive observer stopped; inspect local status'}}

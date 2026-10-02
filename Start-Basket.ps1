param([switch]$Initialize,[switch]$KeepSession)
$ErrorActionPreference='Stop'
$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
foreach($taskStop in @((Join-Path $PSScriptRoot 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'BASKET_STOP'))){if(Test-Path -LiteralPath $taskStop){throw 'STOP exists; inspect its provenance'}}
if(Test-Path -LiteralPath (Join-Path $taskRuntime 'basket-watch.lock')){throw 'Basket observer lock exists; inspect its process'}
if($Initialize){
 & (Get-Command node.exe).Source (Join-Path $PSScriptRoot 'live-paper\basket-watch.mjs') --init --once
 if($LASTEXITCODE -ne 0){throw 'Basket observer initialization failed'}
}
if(!(Test-Path -LiteralPath (Join-Path $taskRuntime 'basket-bank.json'))){throw 'Use -Initialize to freeze the first prospective basket observer'}
$taskProcess=Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList @('live-paper/basket-watch.mjs') -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskRuntime 'basket-stdout.log') -RedirectStandardError (Join-Path $taskRuntime 'basket-stderr.log') -PassThru
$taskProcess.Id | Set-Content -LiteralPath (Join-Path $taskRuntime 'basket-pid.txt')
Write-Output "Basket depth observer PID $($taskProcess.Id); no API calls, orders or inferred fills"
if($KeepSession){$taskProcess.WaitForExit();if($taskProcess.ExitCode -ne 0){throw 'Basket observer stopped; inspect local status'}}

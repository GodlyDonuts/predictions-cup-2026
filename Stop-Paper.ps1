$taskRuntime=if($env:SIG_PAPER_DIR){[IO.Path]::GetFullPath($env:SIG_PAPER_DIR)}else{Join-Path $env:LOCALAPPDATA 'SigTradingCup\paper-runtime'}
New-Item -ItemType Directory -Path $taskRuntime -Force | Out-Null
'User requested paper runner stop' | Set-Content -LiteralPath (Join-Path $taskRuntime 'STOP')
Write-Output 'Stop requested. The runner aborts pending reads and preserves its journal.'

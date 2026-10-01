$ErrorActionPreference = 'Stop'
Set-Content -LiteralPath (Join-Path $PSScriptRoot 'STOP') -Value 'Stop requested'
Write-Output 'Stop requested. The bot finishes its current request and then stops.'

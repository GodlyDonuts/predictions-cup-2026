param([switch]$Once)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$arguments = @('bot.mjs')
if ($Once) { $arguments += '--once' }
& node @arguments

param([switch]$Test)
$ErrorActionPreference='Stop'
$baalLegacyToolchainRoot=Join-Path $env:LOCALAPPDATA 'SigTradingCup/rust-toolchain'
$env:CARGO_HOME=Join-Path $baalLegacyToolchainRoot 'cargo'
$env:RUSTUP_HOME=Join-Path $baalLegacyToolchainRoot 'rustup'
$env:PATH=(Join-Path $env:CARGO_HOME 'bin')+[IO.Path]::PathSeparator+$env:PATH
$baalLegacyCargo=Join-Path $env:CARGO_HOME 'bin/cargo.exe'
Push-Location $PSScriptRoot
try {
 if (!(Test-Path -LiteralPath 'Cargo.lock')) { & $baalLegacyCargo generate-lockfile; if($LASTEXITCODE -ne 0){throw 'Lockfile generation failed'} }
 if($Test){ & $baalLegacyCargo test --locked; if($LASTEXITCODE -ne 0){throw 'Legacy probability tests failed'} }
 & $baalLegacyCargo build --release --locked
 if($LASTEXITCODE -ne 0){throw 'Legacy probability build failed'}
 Copy-Item -LiteralPath (Join-Path $env:RUSTUP_HOME 'toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll') -Destination (Join-Path $PSScriptRoot 'target/release/libunwind.dll') -Force
}finally{Pop-Location}

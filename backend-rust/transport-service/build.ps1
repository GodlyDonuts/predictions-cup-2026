param([switch]$Test)
$ErrorActionPreference='Stop'
$baalTransportToolchainRoot=Join-Path $env:LOCALAPPDATA 'SigTradingCup/rust-toolchain'
$env:CARGO_HOME=Join-Path $baalTransportToolchainRoot 'cargo'
$env:RUSTUP_HOME=Join-Path $baalTransportToolchainRoot 'rustup'
$env:PATH=(Join-Path $env:CARGO_HOME 'bin')+[IO.Path]::PathSeparator+$env:PATH
$baalTransportCargo=Join-Path $env:CARGO_HOME 'bin/cargo.exe'
if (!(Test-Path -LiteralPath $baalTransportCargo)) { throw 'Existing task-local Rust toolchain required' }
Push-Location $PSScriptRoot
try {
 if (!(Test-Path -LiteralPath 'Cargo.lock')) { & $baalTransportCargo generate-lockfile; if($LASTEXITCODE -ne 0){throw 'Lockfile generation failed'} }
 if($Test){ & $baalTransportCargo test --locked; if($LASTEXITCODE -ne 0){throw 'Rust transport tests failed'} }
 & $baalTransportCargo build --release --locked
 if($LASTEXITCODE -ne 0){throw 'Rust transport build failed'}
 Copy-Item -LiteralPath (Join-Path $env:RUSTUP_HOME 'toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll') -Destination (Join-Path $PSScriptRoot 'target/release/libunwind.dll') -Force
}finally{Pop-Location}

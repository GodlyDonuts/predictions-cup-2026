param([switch]$Test)
$ErrorActionPreference='Stop'
$baalQueueToolchainRoot=Join-Path $env:LOCALAPPDATA 'SigTradingCup/rust-toolchain'
$env:CARGO_HOME=Join-Path $baalQueueToolchainRoot 'cargo'
$env:RUSTUP_HOME=Join-Path $baalQueueToolchainRoot 'rustup'
$env:PATH=(Join-Path $env:CARGO_HOME 'bin')+[IO.Path]::PathSeparator+$env:PATH
$baalQueueCargo=Join-Path $env:CARGO_HOME 'bin/cargo.exe'
if (!(Test-Path -LiteralPath $baalQueueCargo)) { throw 'Existing task-local Rust toolchain required' }
Push-Location $PSScriptRoot
try {
 if (!(Test-Path -LiteralPath 'Cargo.lock')) { & $baalQueueCargo generate-lockfile; if($LASTEXITCODE -ne 0){throw 'Lockfile generation failed'} }
 if($Test){ & $baalQueueCargo test --locked; if($LASTEXITCODE -ne 0){throw 'Rust account queue tests failed'} }
 & $baalQueueCargo build --release --locked
 if($LASTEXITCODE -ne 0){throw 'Rust account queue build failed'}
 $baalQueueUnwind=Join-Path $env:RUSTUP_HOME 'toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll'
 Copy-Item -LiteralPath $baalQueueUnwind -Destination (Join-Path $PSScriptRoot 'target/release/libunwind.dll') -Force
}finally{Pop-Location}

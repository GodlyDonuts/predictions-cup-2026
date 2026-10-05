param([switch]$Test)
$ErrorActionPreference='Stop'
$baalToolchainRoot=Join-Path $env:LOCALAPPDATA 'SigTradingCup/rust-toolchain'
$env:CARGO_HOME=Join-Path $baalToolchainRoot 'cargo'
$env:RUSTUP_HOME=Join-Path $baalToolchainRoot 'rustup'
$env:PATH=(Join-Path $env:CARGO_HOME 'bin')+[IO.Path]::PathSeparator+$env:PATH
$baalCargo=Join-Path $env:CARGO_HOME 'bin/cargo.exe'
if (!(Test-Path -LiteralPath $baalCargo)) { throw 'Task-local Rust toolchain required' }
Push-Location $PSScriptRoot
try {
 if (!(Test-Path -LiteralPath 'Cargo.lock')) { & $baalCargo generate-lockfile; if($LASTEXITCODE -ne 0){throw 'Lockfile generation failed'} }
 if($Test){ & $baalCargo test --locked; if($LASTEXITCODE -ne 0){throw 'Rust journal tests failed'} }
 & $baalCargo build --release --locked
 if($LASTEXITCODE -ne 0){throw 'Rust journal build failed'}
 $baalUnwind=Join-Path $env:RUSTUP_HOME 'toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll'
 Copy-Item -LiteralPath $baalUnwind -Destination (Join-Path $PSScriptRoot 'target/release/libunwind.dll') -Force
}finally{Pop-Location}

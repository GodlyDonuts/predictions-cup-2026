# Baal native replay backend

Rust owns offline capture verification, frozen-model inference, wallet replay,
exact accounting and a causal dip parameter grid. The live API collector and
Next.js dashboard still run in Node. Fitting existing ML banks remains JavaScript.
This binary has no HTTP client, credentials or competition execution path.

Build on this PC with `./backend-rust/build.ps1 -Test`. The task-local official
Rust toolchain lives under `%LOCALAPPDATA%/SigTradingCup/rust-toolchain`; the helper
sets only its child process environment. The Windows GNU LLVM build ships
`libunwind.dll` beside the executable. Other systems can use `cargo build
--release --locked` inside this directory. Do not commit `target/`.

```powershell
$runtime = Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$cohort = Join-Path $runtime 'research-cohorts/baal-net-20261004-e'
npm run replay:rust -- --journal (Join-Path $runtime 'journal.jsonl') --bank (Join-Path $cohort 'bank.json') --end-byte 4255286972 --expected-sha a19efc0c0ff5d52f8ce851ce5665631d3e3c76899e3e46c3dbc7653723c62507 --out (Join-Path $cohort 'new-native-replay.json') --workers 8
npm run sweep:rust -- --journal (Join-Path $runtime 'journal.jsonl') --config (Join-Path $cohort 'native-sweep-config.json') --end-byte 4255286972 --expected-sha a19efc0c0ff5d52f8ce851ce5665631d3e3c76899e3e46c3dbc7653723c62507 --out (Join-Path $cohort 'new-native-sweep.json') --workers 8
npm run test:rust-parity
```

Always use a new output path. Each wrapper result has an immutable receipt with
the original prefix, exact bank/config file, binary, output and compiled source
graph hashes. Neither inputs nor prior results are overwritten. Raw journal
hashes include historical state, which is verified without allocating that state.
The accepted source format is canonical JSON emitted by the original journal
writer, with its final hash field; incomplete/noncanonical records fail closed.

Frozen model replay preserves the bank's fit/selection boundaries and three
validation checkpoints, exact tournament/instrument support, prior-only feature
histories and its snapshot ordering policy. Ridge and boosted stump models with
up to 12 features are supported. Unknown/breadth/GPU bank policies fail closed.
No weights are refitted and no production selection changes. Legacy ordering is
preserved when absent; new banks use actual receipt ordering.

The dip grid processes every captured event across the whole timeline. It tests
2,304 predeclared combinations of lookback, history-gap tolerance, z threshold,
minimum edge and holding horizon. It includes delayed take-profit/stop exits,
zero explicit fees, spread, slippage, bounded depth participation, reserve and
exposure limits, version reuse protection and exact FIFO accounting. The
60/80/100% checkpoints continue one wallet; they are development results, not
independent holdouts or prospective profitability. No automatic winner is activated.
Captured price declines can contain information; a historical mean is a hypothesis.

Measured no-refit comparison on the original 4.25 GB/9,491-record/2.31-day prefix:
127.73 seconds JavaScript (four workers) versus 12.21 seconds Rust (eight), 10.46x
for this run. Verification dominates the native time. Model fitting is excluded
from both sides; this does not accelerate platform pacing. The original model
comparison had zero trades. Full scorecards stay in the restricted local runtime;
only aggregate measurements belong in Git.

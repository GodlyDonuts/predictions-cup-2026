# Baal native replay backend

Rust owns offline capture verification, frozen-model inference, wallet replay,
exact accounting, causal dip parameter grids and declared cost comparisons. The live API collector and
Next.js dashboard still run in Node. Fitting existing ML banks remains JavaScript.
This binary has no HTTP client, credentials or competition execution path.

`quote-replay --declaration <file> --sources <file> --workers 8 --out <new-file>`
runs the separate full-universe historical-dip cohort. Its three compact inputs
are public depth, non-executable quotes and private sanitized account eligibility.
Each source descriptor binds original bytes/hash and frozen/prior checkpoints.
All original prefixes are rehashed. Public session/invalidation/version checks
reuse the existing native validator. Publication availability orders observations;
quotes or eligibility at the same millisecond cannot inform a book decision.

Features use one observation per market/minute, exclude the two most recent
minutes, and compare current executable ask plus two-sided slippage with a lower
historical bid quantile. Fixed 1/3/6-hour windows, optional rebound confirmation,
holding horizons and 3/10/30-second latency/cost scenarios have independent wallets.
Quotes supply no depth and never fill orders. Account evidence must be complete,
published before the depth request, unexpired and exclude neither held side nor
an open order in that market. Missing historical account evidence blocks buys;
it is never backfilled using today's account snapshot. Account exclusions cancel
pending buys while held paper inventory can still be reduced. New intents use
publication availability; later independent requests/version/depth supply fills.
Historical warmup contributes no cash, fills or inherited P&L to a new cohort.

October 4 measured replay: 2.20 captured days, 146.37 MB, 237 markets, 24 fixed
strategies / 72 scenario wallets in 4.13 seconds (3.41 source verification +
0.49 feature/wallet replay seconds, plus process overhead). The old state-bearing
input remains archived; all 100 original scorecards were exactly unchanged on
their original source prefix. These are different workloads, not an identical
strategy speed ratio or a profit result. The frozen host runs inside the single
collector with no new API client/service and the same four-book/20-read budget.
Original journals, banks and wallets are retained. Zero fees; profitability is
unestablished. See `data/native-quote-preparation-20261004.json`.

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
npm run evaluate:rust -- --journal (Join-Path $runtime 'journal.jsonl') --declaration (Join-Path $runtime 'research-cohorts/baal-dip-subsequent-20261004-v1/declaration.json') --end-byte 4480806660 --expected-sha 88add681dd3390fbb3485e5db9f3c83459ca6f7ff7e6314eb15fe70d81f99eb5 --out (Join-Path $runtime 'research-cohorts/baal-dip-subsequent-20261004-v1/new-evaluation.json') --workers 8
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

The evaluator accepts a schema-1 declaration containing `mode`, `cohort`,
`declaredAt`, the exact `config`, a `development` prefix binding
(`records`, `bytes`, `hash`, `through`), named `strategies` with dip `parameters`,
named `scenarios` with `latencyMs`, `slippageTicks`, `depthFractionBps`, and
`stopBufferTicks` (1–5). It checks the original byte boundary, hash and clock.
Prior records warm features only; the later wallet starts with fresh cash and
no positions, fills or inherited P&L. Both `snapshot_limit_v1` and the experimental
`profit_floor_stop_buffer_v1` use delayed, bounded, versioned ladder fills.
Profit-floor take-profit limits round upward to preserve the required gain;
stop/horizon buffers admit only a bounded additional adverse move. These remain
offline counterfactual policies, with no exchange queue model or live deployment.

Use `causal_dip_subsequent_v1` for a subsequent-data comparison, and
`causal_dip_development_diagnostic_v1` for reused development data. The former
does not itself prove that the data was untouched: selection provenance must be
recorded separately. Data collected before a declaration is retrospective, even
if its performance was not inspected. Never relabel development reuse as a holdout.

Each scorecard contains distinct warmup, flat-history, z and cost-margin gates;
maximum reversion/edge diagnostics are in probability units. Matched FIFO rows
reconcile exact net P&L to midpoint movement minus quoted spread/ladder cost,
modeled slippage and fees. Partial rows and related variants are correlated;
their count is not an independent sample count. Unliquidated inventory remains
charged in conservative P&L and is separate from realized trade economics.

The October 4 subsequent comparison evaluated 100 cards over 294 newer records:
zero fills, zero net P&L, with no cost-qualified entries. A separately labeled
180-card development cost study took 11.53 seconds to verify 2.31 days/4.25 GB and
1.06 seconds to simulate. The best historical baseline's five closes produced
0.0825 midpoint gain minus 0.0275 spread/ladder cost and 0.0500 modeled slippage,
leaving 0.0050. Two ticks of slippage made that variant negative; the alternative
exit policy did not improve the baseline. No strategy or capital allocation was
activated. Aggregate evidence is `data/baal-native-evaluation-20261004.json`.

The dip grid processes every captured event across the whole timeline. It tests
2,304 predeclared combinations of lookback, history-gap tolerance, z threshold,
minimum edge and holding horizon. It includes delayed take-profit/stop exits,
zero explicit fees, spread, slippage, bounded depth participation, reserve and
exposure limits, version reuse protection and exact FIFO accounting. The
60/80/100% checkpoints continue one wallet; they are development results, not
independent holdouts or prospective profitability. No automatic winner is activated.
Captured price declines can contain information; a historical mean is a hypothesis.

`publication --journal <file> --scope <universe.json> --end-byte <bytes>
--expected-sha <sha256> --out <new-file>` validates a public quote-journal prefix
and exports its exact consumer state. The streaming reducer verifies every
record hash, chain, tournament/instrument scope, session boundary, invalidation,
receipt clock, tick/depth ladder and version/rollback rule. Complete malformed
records fail closed; explicit byte and original-byte hash bindings are required.
Records are bounded to 8 MiB. This command accepts no account credentials and
does not perform network requests or contest orders.

On the October 4 captured 33.15 MB/15,879-record prefix, original JavaScript
validation took 48.94 seconds and the native adapter took 1.50 seconds, a 32.66x
speedup with exact complete-state equality. This measures validation and startup
transport, separately from model fitting and platform request pacing. The active
source-probability V3 paper host uses the pinned native binary and runtime library
for prefix admission, then incremental JavaScript economics on new records. Its
live cold start was 1.82 seconds. V1/V2 banks, journals, FIFO basis, cash and fills
are preserved; the strategy and expired research were not reset or retrained.
See `data/baal-native-publication-benchmark-20261004.json` and
`data/probability-native-live-20261004.json`. This is a partial backend migration;
live order execution and incremental paper economics remain outside this native path.

The collector's versioned V3 producer also uses this pinned native prefix on
startup. Its subsequent publication records and complete immutable states match
optimized JavaScript V2 byte for byte through books, invalidations and shutdown.
On the same sealed 34.16 MB / 16,301-record prefix, optimized V2 took 5.16 seconds
and the native adapter took 1.54 seconds (3.34x), with exact complete-state parity.
The earlier 32.66x result compared the older original paper validator. Live V3
publication startup measured 1.68 seconds; its separately measured prior V2
startup was 5.44 seconds on a slightly smaller prefix. Eleven targeted integration
checks passed with no skips. See `data/native-collector-benchmark-20261004.json`
and `data/native-collector-live-20261004.json`. These timings describe the public
publication stage, not overall collector startup or platform pacing. The separate
collector journal migration is measured below.

Measured no-refit comparison on the original 4.25 GB/9,491-record/2.31-day prefix:
127.73 seconds JavaScript (four workers) versus 12.21 seconds Rust (eight), 10.46x
for this run. Verification dominates the native time. Model fitting is excluded
from both sides; this does not accelerate platform pacing. The original model
comparison had zero trades. Full scorecards stay in the restricted local runtime;
only aggregate measurements belong in Git.

The independent `journal-restore` crate restores the original collector journal
without changing its append/accounting interface. Build with
`./backend-rust/journal-restore/build.ps1 -Test`. Its CLI requires `--journal`,
`--end-byte`, `--expected-sha` and a new `--out` path. It validates compact
JSON.stringify number/string/key semantics, record hashes, sequence and chain;
accepts records up to 16 MiB and nesting up to 256; and returns the exact final
record body into restricted runtime storage. The adapter independently binds the
source prefix, rejects source/binary/library changes and responds to STOP while
restoring. Original `Journal.commit` and FIFO/accounting reducers remain intact.

On the same original 4.25 GB/9,491-record/2.31-day prefix, final Rust recovery took
18.68 seconds including the independent Node source hash, versus 30.06 seconds
JavaScript (1.61x). Initial prototypes measured only 1.01x and 1.20x; their
receipts are preserved. Small-object key admission, exact integer fast paths,
reusable line buffers and parsing only the final output removed the measured
overhead. Full final-body and tail/sequence equality passed. This is recovery
speed, separate from the 10.46x model replay measurement. See
`data/journal-native-benchmark-20261004.json` and the live preservation receipts.

`prepare-cache` writes an immutable compact observation cache from a fully verified
original prefix. It takes `--declaration`, `--journal`, `--end-byte`,
`--expected-sha` and a fresh `--out` path. Its receipt pins cache SHA, original
source SHA/byte boundary, scope and native engine. Cache frames preserve exact
nanosecond integers and contain normalized books/invalidation clocks, not raw
account or wallet state. Node hashes bytes and reads receipts; it never decodes
the numeric cache payload.

`evaluate-cached` uses the same declaration, source and output arguments plus
`--cache`, `--cache-sha`, `--cache-source-sha`, `--cache-end-byte`,
`--prior-end-byte` and `--prior-sha`. The prior digest must come from the caller's
last immutable verified result. The native reader rehashes the complete original
cached prefix, verifies every appended canonical record and proves the exact
prior source digest/boundary before evaluating. It computes the current SHA in
that same pass; optional `--expected-sha` can independently check a known target.
The frozen cache SHA must come from verified cache generation, not from an
arbitrary file presented at evaluation time. New outputs include `cacheProof`.

On the same 5.086 GB / 2.655-day source, full native evaluation took 14.02 seconds
and cached evaluation took 4.25 seconds (3.30x). The 27.24 MB cache is 185x smaller
than its original 5.048 GB prefix. All 100 full/cached scorecards and all 100
original frozen-engine scorecards match exactly. Model engine/evaluation sources
and declaration remain unchanged. The V2 live host also removes the redundant
Node source-hash pass, refreshes after collection cycles, and preserves prior
fills through an explicit execution continuation. No model profit follows from
these measurements. See `data/native-cache-benchmark-20261004.json` and
`data/native-cache-milestone-20261004.json`.

`evaluate-observed` and `evaluate-observed-cached` accept the respective ordinary
evaluation arguments and add a versioned, read-only `pendingReadIntents` array to
each scorecard. Intent identity, instrument, action, quantity, limit, decision
clock, exact source version, earliest independent request and expiry are exposed.
The const inspector is included alongside unchanged `engine.rs`; it does not
alter trading state. Ordinary evaluation omits the new fields. A new pinned
build/cache and host continuation are required; old binaries, caches and outputs
are retained. Before continuation, all original scorecard fields must match the
active archived engine on the identical original prefix, and full/cached observed
cards must agree. These fields request confirmation/risk reads only and grant no
competition execution authority.

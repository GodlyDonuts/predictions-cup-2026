# Realistic three-venue replay — October 19, 2026

The offline replay layer is implemented and exercised with a neural policy on
previously inspected real data. This is infrastructure readiness, not completion
of full-history training, independent testing, or a profitable checkpoint.

## Delivery and execution

`moloch/learning_realism/replay.py` provides `ReplayTape` with the same `.episode`
interface used by the existing rollouts. `open_replay(spec, dataset, mechanics,
role, frozen_finalist_sha256=...)` is the checked entry point for new experiments.
It validates the immutable dataset, source and mechanics hashes and refuses a
full-history specification whose coverage audit failed.

The hidden SIG exchange world advances at original recorded availability times.
Policy-visible SIG books are a separate cache. Poll requests capture the latest
already available snapshot and reveal it only on response completion. Price
offsets are chosen using the delivered book; later matching uses the hidden
exchange book. The actor cannot use an unseen price update to choose its limit.
Repeated snapshots preserve original source age and counterfactual liquidity
depletion. Missing history is never interpolated into executable depth.

SIG book and trade polling share the **same serialized** budget as order
preflights, submissions and cancellations: 100 read admissions/minute with
600 ms completion gaps, 24 writes/minute with 2500 ms completion gaps. Existing
Retry-After and unknown-acceptance state is retained. Trade prints require a
separate read, preserve execution time, and cannot invent book depth or cash.

Kalshi and Polymarket are observation-only, separately paced at configured
ceilings of 20 and 30 requests/second. Round-robin scope expands only as streams
appear on the tape. Each modeled request returns one observed stream, without
assuming undocumented batching discounts. External response latency defaults to
150 ms, explicitly an **uncalibrated hypothesis**. Source/receipt clocks and the
existing freshness checks continue to apply. No external trading adapter exists.

This profile conservatively models polling. It does not claim that SIG realtime
push deliveries cost one REST request each in production. A production-parity
push profile requires transport-provenance and subscription/reconnect evidence;
no free push channel or reconstructed full-depth history is assumed here.

Kalshi's current primary documentation describes token budgets; Basic's 200
read tokens/second at the default ten-token cost supports the configured
20 ordinary requests/second. Endpoint costs and actual account grants remain
authoritative: <https://docs.kalshi.com/getting_started/rate_limits>.
Polymarket's configured 30 requests/second is the existing collector ceiling,
not a claim that every endpoint has that published limit:
<https://docs.polymarket.com/api-reference/rate-limits>.

## Frozen chronology and scoring

The caller supplies one explicit competition-start timestamp and cutoff:

* Train: competition start up to cutoff minus eight days.
* Evolution: cutoff minus eight days up to cutoff minus four days.
* Independent test: final four days, accessible only with a frozen finalist hash.

All intervals are half-open. Burn-in, decisions, outstanding receipt/cancel drain
must fit entirely within one role; the implementation conservatively refuses a
terminal event exactly at the next boundary. No random row split is used.
`prepare.py` creates an immutable specification and per-venue coverage report
for every role. It refuses to represent sparse endpoint observations as complete
history. Its default maximum aggregate venue gap is 120 seconds, a declared
coverage threshold rather than proof of complete market-level history.

`protocol.growth_score` implements the proposed reward: daily mean log growth
minus 1.5 times sample standard deviation divided by sqrt(D), with the positive
score multiplied by sqrt(N/(N+10)). Net receipts/costs are aggregated by supplied
independent economic-event cluster before counting profitable opportunities;
splitting one opportunity into multiple fills cannot increase N. At least two
complete days are required; bankruptcy is explicitly ineligible, not a finite
positive score. Missing wealth must not be passed as zero or replaced with
unverified external reference marks.

This scoring function is available to the new experiment coordinator. The older
population launchers retain their frozen interval-HOLD objectives; they are not
silently relabeled as runs using the new daily reward. A final-test hash is an
access/provenance guard, not proof nobody previously inspected those dates.
Previously inspected data must be declared contaminated rather than described
as an untouched holdout.

## Validation and present limits

The focused replay/flow suite passes 27 tests, including nine new realism tests.
With the existing clock and execution-environment checks, 52 tests pass.
Tests cover shared budgets, actual delayed visibility, hidden-book price leakage,
external throughput, original staleness, separately charged trade reads,
backoff/unknown-state preservation, chronological boundaries, event grouping and
the exact reward formula.

[Real-data neural smoke](realistic-replay-smoke-001.json): 60 decisions, 19 SIG
polls, 1,221 Kalshi polls and 1,783 Polymarket polls including burn-in; 47 SIG reads
including preflights, zero SIG writes and zero modeled fill receipts. No HTTP was
used. Those cumulative counts span burn-in plus the 60-second decision period,
not a one-second burst. The random small policy is not a trained finalist.

[Selected dataset audit](realistic-replay-data-audit-001.json) covers only about
04:16–04:31 UTC on October 19: 1,953 SIG books, 16,316 Kalshi references and
41,021 Polymarket references. This is an already inspected engineering subset,
not an inventory of all archives. It cannot satisfy the requested multi-day
experiment. Full historical coverage and an uncontaminated independent test
have **not** been established.

The current policy heads still expose WAIT, BUY, SELL, CANCEL and EXIT with
price, sizing and patience choices. This work does not establish complete SIG
API action parity (for example every collateral or borrowing operation), nor
calibrated passive queue fills. Production source, worker, limits and holdings
were not changed by this replay task.

## Commands

Use the existing local learning virtual environment, without API credentials:

```powershell
python -m unittest moloch.learning_realism.test_replay moloch.learning_flow.test_features moloch.learning_flow.test_episodes -v
python -m moloch.learning_realism.smoke --dataset PATH_TO_INSPECTED_FLOW_DATA --mechanics research/tartarus-learning-v2/development-mechanics-v2.json --output NEW_SMOKE.json
python -m moloch.learning_realism.prepare --dataset PATH_TO_FULL_FLOW_DATA --mechanics research/tartarus-learning-v2/development-mechanics-v2.json --competition-start ACTUAL_START_ISO_UTC --cutoff FIXED_CUTOFF_ISO_UTC --output NEW_SPEC.json
```

The preparation command can write a failed-coverage report; only
`full_history_ready: true` specifications can be opened through `open_replay`.

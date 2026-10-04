# Execution-aligned training and latency diagnostics

## Scope

This patch fixes new-training defaults, adds executable-return diagnostics, repairs a stale import path, and improves the current transport's latency measurements. It does not establish profitability, activate a strategy, restart a collector, place an order, change a rate allocation, or alter any frozen bank, wallet, journal, configuration, or execution-owner record.

## Training

Both `trainBank()` and `live-paper/ml-bank.mjs` now default to `delayed_fifo_roundtrip_net_v1` and `market-isolated-v2`. The existing round-trip label engine is reused: delayed entry, a holding period starting at the fill, delayed FIFO exit, partial depth, stop exits, ticks, and configured fees. Censored and unfilled decisions remain counted exclusions, not assumed zero-cost successes.

The CLI also defaults to coverage-gated `supported-instruments-v1`: only instruments with the existing minimum training/development support can enter the prospective paper bank. `--training-scope all-markets-v1` retains the stricter all-market coverage option. `--allow-undercovered-exploration` is an explicit research override, incompatible with supported-instrument scope. The lower-level research API retains its explicit coverage options; direct callers requiring deployment-style coverage must pass them.

Example for a **new** paper cohort, using an immutable journal snapshot and a new output path:

```sh
node live-paper/ml-bank.mjs --journal /path/to/frozen-journal.jsonl --config live-paper/fee-free-config.json --out /path/to/new-roundtrip-bank.json
```

`fee-free-config.json` means zero *additional explicit* fee while retaining its extra tick stress and other conservative assumptions. It is not a claim about all venue costs. Use separate named outputs for fee/tick stress scenarios; the existing `cost-audit.mjs` compares those scenarios without modifying historical results.

Explicit historical reproduction remains available:

```sh
node live-paper/ml-bank.mjs --journal /path/to/frozen-journal.jsonl --config live-paper/config.json --out /path/to/new-legacy-reproduction.json --target-policy legacy_decision_time_bid_forecast_not_executable_roundtrip_target --history-policy global-reset-v1 --allow-undercovered-exploration
```

A missing target policy in an **existing frozen bank** still means legacy. Existing files are refused before journal loading or fitting, and the final hard-link publication still prevents an overwrite race. Unknown, repeated, missing-value, and contradictory CLI options fail early.

Each newly trained round-trip model includes `executionDiagnostics`: decision counts and observed/predicted net returns per share at the prospective runner's fixed 0.005, 0.01, and 0.015 margins. Fees already included in the target are not subtracted again. These are overlapping, completion-conditioned development probes, **not portfolio PnL or independent trades**. The historical RMSE ranking is unchanged; a selected model is not thereby profit-qualified. A fresh prospective paper cohort is still required before considering promotion.

## Latency

The production `transport-runtime.mjs` now emits v2 timing records. Stage durations use a monotonic clock while persisted dates, durable rate reservations, cooldowns, and request ordering retain their existing contracts. HTTP-error body decoding is timed rather than omitted. The transport records bounded event-loop histograms and closes them with the client. Default histogram resolution is 20 milliseconds; these are process-wide timer-delay windows, not pure CPU time or a per-order explanation.

The analyzer accepts mixed legacy and v2 records, labels clock provenance, and exposes nearest-rank median/p95/p99/max statistics and sample counts. Empty event-loop windows are null, not zero latency. Existing readers that only accept v1 must be updated with the transport; the repository collector/analyzer support both. No account content, keys, URLs, cursors, headers, or raw error messages are added to telemetry.

```sh
node live-paper/read-timing.mjs --file /path/to/read-timings.jsonl
```

This does not remove API caching, create higher request limits, measure exchange matching latency, or fully trace signal-to-fill time. No polling, concurrency, order authorization, fee, or risk limit was changed.

## Clean-checkout repair

`basket-inputs.mjs` pointed at nonexistent `../strategy.mjs`; the tracked helper is `../bot/strategy.mjs`. Correcting that import restores dependent module loading without changing basket economics. A new static relative-import test covers production modules.

## Verification

Tested offline on Node.js v22.16.0: **1,367 passing checks** in the selected bot, feed, paper, execution, and live-paper suites, including 13 new regression tests. Coverage includes end-to-end new CLI training and real shadow replay, legacy interpretation, delayed fills and exact costs, immutable banks, wall-clock jumps, error-body timing, histogram cleanup, p99, authorization, shared read budgets, restart/backoff behavior, and resolved imports.

This is **not an all-green claim for every repository test**. The run explicitly excluded:

- `realtime-client.test.mjs`, which needs the external Supabase SDK not installed in the offline workspace.
- The minimal-placement-schema test, which expects an untracked root `openapi.json` snapshot.
- `runner-lifecycle.test.mjs`, whose older mock wiring fails 44 cases once the stale import is resolved. The same 44 failures were reproduced on the pre-patch source with only a local helper-path compatibility copy. No production behavior was changed to satisfy that harness.

`LOCALAPPDATA` was pointed at an empty temporary test directory for Windows-oriented module paths. No real runtime directory or contest credentials were used. No production bank was retrained or activated; the CLI integration test uses synthetic observations only.

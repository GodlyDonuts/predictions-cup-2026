# Live market paper runner

This runner reads official SIG API v1 books and simulates a separate bankroll. It has no order placement/cancellation endpoint, realtime token mint, SDK dependency or execution handoff. Existing encrypted local credentials are read in memory; they are never embedded in a URL, dashboard, journal or repository. Contest positions and orders are excluded from paper scope at each cycle. Account exclusions are sequential observations, not a guarantee about concurrent discretionary orders.

## Start, inspect, stop and resume

From the repository root on Windows with Node 22+ and an already approved DPAPI credential:

```powershell
./Start-Paper.ps1
# Dashboard: http://127.0.0.1:8787/
./Stop-Paper.ps1
```

`Start-Paper.ps1` restricts the runtime folder ACL to the current Windows user, SYSTEM and Administrators, and starts a hidden background process. Runtime files are under `$env:LOCALAPPDATA/SigTradingCup/paper-runtime`: `stats.json`, hash-chained `journal.jsonl`, `rate.json`, PID and safe stdout/stderr. The dashboard binds only to 127.0.0.1 and refreshes every three seconds. It shows explicit unavailable/stale marks and skipped observations. It is available while the runner process is running; `stats.json` preserves final stats after stop.

Workspace `STOP` and runtime `STOP` abort pending reads. An intended restart requires reviewing/removing the relevant stop marker, then launching again. The local scanner's `bot.lock` excludes another local scanner/runner; do not delete a lock without checking its recorded PID. The journal restores the last committed paper state, pending intents, exact basis, fills and config hash. Changed config requires a separate runtime directory via `SIG_PAPER_DIR`; use the same shared collector lock. A partial or corrupt journal fails closed and needs explicit recovery. A crash may discard an uncommitted cycle; its displayed intermediate values are provisional until the cycle journal commit.

## Defaults and interpretation

## ML bank and prospective candidate wallets

Train locally from the one collector's verified capture, then start the shadow worker:

```powershell
node live-paper/ml-bank.mjs --journal "$env:LOCALAPPDATA/SigTradingCup/paper-runtime/journal.jsonl" --config live-paper/config.json --out "$env:LOCALAPPDATA/SigTradingCup/paper-runtime/ml-bank.json"
./Start-ML.ps1
```

The bank tries 128 ridge configurations: four future exit horizons, four feature subsets and eight regularization strengths. Insufficient matured labels reject a configuration explicitly. Features use current depth/spread/source age and preceding observations. Training normalization fits only training data; training labels crossing the chronological development split are purged. All attempted models and rejection reasons remain in the frozen local bank. Three lowest development RMSE models per usable horizon each spawn three paper margins, with independent cash. This selection is exploratory and the short correlated development split does not establish profitability. The target forecasts future best-bid proceeds after adverse exit tick/assumed fee; entry ask/tick/fee are charged separately. Full-size future exit depth is unproven, so targets are hypotheses rather than fills.

The shadow worker makes zero API requests and reads the existing journal every 15 seconds. Predictions, intents and fills start strictly after the frozen training capture cutoff, with six fresh observations for warmup. It shares the reviewed delayed taker, exact FIFO, depth, reserve, position and exposure machinery. Positions exit at the candidate horizon or existing stop rule, using a subsequent usable bid with costs; no price touch or midpoint recovery creates a fill. Source prefix and model-bank hash must match on restart. Cash, fills, history and intents persist atomically. Failed verification stops the worker and publishes failed status; incomplete concurrent journal reads may require restart after inspection. Runtime files contain the full model bank, `ml-shadow-state.json`, `ml-stats.json` and a distinct worker lock, under the existing restricted runtime directory. Do not overwrite a bank while its worker is active; use a separately named cohort/runtime for retraining. Workspace/runtime STOP halt both paper processes. Dashboard `/api/ml` presents all candidate wallets; stale update timestamps indicate a stopped worker. Existing collector requests and rate allocation are unchanged.

## Original strategy defaults

Named cohorts under runtime `ml-cohorts/<name>/ml-bank.json` are discovered by the same ML worker. Each has a separate frozen bank/hash/cutoff, state and stats file. Names contain lowercase letters/digits/hyphens. Original wallets remain unchanged. The aggregate dashboard namespaces candidate IDs by cohort and shows summed model/wallet counts. A failed cohort marks the aggregate degraded; root source verification still fails closed. Training refuses existing banks and publishes completed banks atomically with a non-overwriting hard link (unsupported filesystems fail rather than exposing partial models).

`--family boosted_stumps` trains a competing nonlinear family: regularized residual decision stumps with training-only quantile thresholds, minimum leaf sizes and shrinkage. It tries 128 configurations across the same four horizons and uses the same purged causal labels and persistence baseline as ridge. New family/cohort performance is exploratory; added model counts do not create additional independent data or establish an edge. To add a cohort, create a new directory under the restricted runtime tree, then train with `--out` pointing to that directory's `ml-bank.json`. The worker reads the original single collector journal; adding cohorts adds zero API requests. Never reset older cohorts to make comparative results look better.

ML worker status distinguishes waiting for a concurrent journal append, running, stopped and failed. A partial read advances no cursor or wallet; it retries at the next poll and fails after four consecutive incomplete reads. A completed corrupt chain or backward event clock fails immediately. Status retains candidate accounting during waiting/failure. `ML_STOP` in the runtime directory stops only the ML worker within its 200ms sleep checks, leaving the collector running; inspect/remove that marker for an intended ML restart. Per-candidate telemetry reports unique observations, warmup, invalid forecasts, cost-margin passes/rejections and zero top-ask capacity. These counts begin when telemetry is installed and are not reconstructed as historical totals.

- Four fixed single-YES contracts: Alaska Senate, Florida Senate, North Carolina Senate and Michigan Governor. This is not whole-universe coverage.
- Each strategy has an independent 100,000-SUSQie paper bankroll; funds are not transferred from the contest account.
- A lagged rolling-mean mean-reversion strategy waits for five observations and a 1.5 z-score dislocation with minimum expected edge after assumed costs. Its signal is an uncalibrated quantitative hypothesis.
- A small buy-and-hold control requests ten shares per exchange to measure spread/fees and mark movement. Its deliberate control purchases are not alpha signals. No-trade baseline P&L is zero.
- Decisions are recorded at receipt time. Hypothetical taker fills require a subsequent REST observation after 1,000ms modeled latency, sufficient depth, and the decision limit. Fills use 1% of each displayed level with one adverse tick and an unverified 10bps fee scenario. One observed version cannot be spent twice by the same strategy. The two independent strategies are separate counterfactual runs and can each use the same historical book.
- Book disappearance, quote touches and price marks never create passive fills. No maker fill rate, actual execution, market impact, hidden liquidity or trade-tape completeness is claimed. A later snapshot cannot prove executable liquidity remained available at an unobserved time.
- Source timestamps/version, exact tournament context, positive on-tick depth and request latency are checked. Books with missing/stale versions, rollback, crossed depth or real account exposure are rejected. Marks become unavailable after `maxDataAgeMs + 4 * cycleIntervalMs`; estimated liquidation also requires sufficient haircutted bid depth. Midpoint marks, realized FIFO P&L and hypothetical liquidation P&L are distinct.
- Long-only integer shares, exact micro-SUSQ accounting, 50-share position caps, 1,000-SUSQ acquisition exposure cap and 99,000-SUSQ reserve. Fee charges round up; spread/slippage can immediately cause losses. No shorting, settlement processing, netting or automatic strategy promotion.
- One serialized GET transport waits six seconds between admissions (at most ten starts per minute). 429/503 extend its cooldown, persisted across restart. Temporary network/server outages pause account polling with degraded status; invalid authentication or reconciliation stops the runner. Market failures are recorded as skips. All account keys share the platform budget: other machines/clients must coordinate separately. There is no distributed quota guarantee.

## Test and refine against captured data

The same serialized collector also rotates the 237 catalogued tournament exchanges through the documented `/exchanges/prices` endpoint, with at most 100 IDs per batch and one batch after each four-book cycle. Every request shares the existing six-second admission interval, Retry-After persistence and account read budget. Existing frozen models still use the four depth books. Bulk responses add breadth for later cross-market feature development without changing wallet signals or fills. Public observations are recorded in each journal cycle's `bulkPrices` field; missing IDs and collection errors are explicit. The catalogue was observed October 1 and must be refreshed when tournament membership changes.

Inspect http://127.0.0.1:8787/api/breadth for the latest collection cache. Each exchange carries its own request and receipt times; the endpoint's newest collection age does not describe every cached quote. The official bulk schema has no source timestamp or depth: its bid/ask/latest-price observations are non-executable and cannot prove current quote freshness, fill capacity or profitable arbitrage. Missing exchanges are removed from the cache after their next batch; invalid or incomplete batches never replace valid coverage. Account state and credentials are excluded.

```powershell
node --test live-paper/*.test.mjs
node live-paper/replay.mjs --journal "$env:LOCALAPPDATA/SigTradingCup/paper-runtime/journal.jsonl" --config live-paper/config.json
```

Replay verifies the complete source journal chain and runs without credentials or network access. Compare configurations using copies of the configuration file, preserve the original run's config and source checksum, and use unseen future captures to evaluate changes. The default replay is whole-path parameter exploration, not a held-out performance result. Synthetic correctness tests and short live samples do not establish profitable strategy performance.

For an offline explanation of missing entries, run:

```powershell
node live-paper/diagnostics.mjs --journal "$env:LOCALAPPDATA/SigTradingCup/paper-runtime/journal.jsonl" --config live-paper/config.json
```

Diagnostics verify the captured journal prefix and report source/config hashes, per-market accepted versus duplicate versions, explicit skip reasons, request-to-receipt latency, source age, sampling gaps, spread and entry-edge distributions, and overlapping warmup/variance/z-score/cost gates. Identical versions do not extend signal history. Counts distinguish passing both signal gates from actual new intents. Top-ask depth capacity is a current snapshot diagnostic, not a future fill guarantee. The edge is the strategy's lagged-mean hypothesis less ask, adverse tick and assumed entry fee; subsequent exit costs are additional. No parameters are fitted or changed, no credentials are loaded, and no network requests are made. A partial concurrent append fails validation; retry after that append finishes. Save aggregate output rather than publishing original account snapshots.

## Frozen chronological evaluation

`data/quant-evaluation-plan-2026-10-02.json` prespecifies the October 2 01:00–07:00 UTC window, the complete base configuration and all six cost/depth/latency variants. Prior data stays exploratory. Each variant starts flat, with fresh in-window warmup; observations and account exclusion reconciliation must be in the window, with requests at or after reconciliation. The start is inclusive and the end exclusive. Run:

```powershell
node live-paper/evaluate.mjs --journal "$env:LOCALAPPDATA/SigTradingCup/paper-runtime/journal.jsonl" --plan data/quant-evaluation-plan-2026-10-02.json
```

The evaluator validates the original complete journal chain, then builds an explicitly derived window chain. Its original-source checksum and transformed-window checksum are separate. It reports every variant, no-trade/control benchmarks, realized versus midpoint versus estimated liquidation returns, remaining positions and pending intents. An end quote does not force a sale. Missing/stale/depth-insufficient liquidation is unavailable. Before the boundary it returns awaiting capture; partially captured windows remain incomplete. Fewer than 100 completed mean-reversion trades is insufficient for the specified review threshold; passing that count still does not establish profitability or statistical independence. Fees remain unverified scenarios. The creation timestamp alone is not proof of preregistration: use the published commit and issue timestamp predating the boundary. Changed hypotheses require a new future plan and fresh data; do not overwrite this plan or cherry-pick sensitivities. Independent review is required before interpreting performance. This offline tool adds no network requests and does not change the collector or strategy.

## Review fixes, engine revision 3

The initial revision had five independently reproduced evidence defects. Revision 3 preserves journal event-clock/outage/skip timing in replay; compares source timestamps at nanosecond precision; rejects incompatible equal versions and conflicting tournament contexts; aborts incomplete or unmappable account reconciliation; derives round-trip win counts from the complete durable fill ledger; and persists a manual-review stop sentinel for overflowing Retry-After. Identical source versions cannot create new fills. `sellExecutions` is separate from completed flat-to-flat trades and their cumulative win rate.

Original journals are preserved. Resuming revision 2 canonicalizes consumed version keys and recomputes reported round trips without changing cash, fees, FIFO lots or fills. Historical optimization results remain unvalidated until corrected replay and independent re-review. A rate record with `manualReview: true` never schedules a guessed earlier retry; inspect the service condition and record before an intentional restart. Nine targeted review regressions cover the reproduced cases in addition to the existing tests.

Account exclusion validation follows the distinct official schemas: tournament position quantities are signed (positive YES, negative NO), so either nonzero side excludes the exchange. A valid zero position is flat. Order quantities must be positive. Missing/noncanonical exchange IDs, noninteger or unsafe quantities and incomplete order pagination abort the whole reconciliation. A signed-position review regression additionally checks exclusion through book normalization and rejects malformed zero-position rows.

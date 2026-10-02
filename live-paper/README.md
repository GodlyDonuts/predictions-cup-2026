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

## Review fixes, engine revision 3

The initial revision had five independently reproduced evidence defects. Revision 3 preserves journal event-clock/outage/skip timing in replay; compares source timestamps at nanosecond precision; rejects incompatible equal versions and conflicting tournament contexts; aborts incomplete or unmappable account reconciliation; derives round-trip win counts from the complete durable fill ledger; and persists a manual-review stop sentinel for overflowing Retry-After. Identical source versions cannot create new fills. `sellExecutions` is separate from completed flat-to-flat trades and their cumulative win rate.

Original journals are preserved. Resuming revision 2 canonicalizes consumed version keys and recomputes reported round trips without changing cash, fees, FIFO lots or fills. Historical optimization results remain unvalidated until corrected replay and independent re-review. A rate record with `manualReview: true` never schedules a guessed earlier retry; inspect the service condition and record before an intentional restart. Nine targeted review regressions cover the reproduced cases in addition to the existing tests.

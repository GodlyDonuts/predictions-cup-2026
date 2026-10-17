# Moloch: current research handoff

**Observed 2026-10-17 02:25:28 UTC.** This is the current baseline. Older dated reports are history. No paper is declared fully implemented.

## Production and actual capital

Sole installed VPS worker **595256**, source **f21c8b5f**, binary **b398c7e7**, native **7a74f8c3**, started02:16:57UTC. It is recovering the original ledger. **Resumed trading is not yet verified**; published trading progress still belongs to old592183. No active execution STOP exists.

The installed closure-row cache addresses repeated parsing during entry preparation; exact expiry diagnostics were added. All14 financial bridge bytes, four model-generation records, positions, losses, capital authorization, source clocks, deadlines and daily windows remain intact. No extra account client or pacing mode was activated.

|Financial observation |Verified value at stopped sequence381266 |
| --- | ---: |
|Initial authorized allocation |95,579.915 SUSQies |
|Cash |48,061.580 |
|Owned position cost |47,382.105 |
|Combined attributed realized P&L |−136.230 |
|New-trading realized / inherited gross |−181.360 /45.130 |
|Official ledger fill rows |283 |
|New filled BUY orders since earlier R11 release |5 |
|Shares / principal in those five buys |38,885 /22,701.270 |

Latest purchase: Alaska1067 YES10,000 for3,113.77 across18 official rows. Open acquisitions are not realized profit. Dashboard daily P&L, whole-account liquidation P&L and current leaderboard rank are unverified here. Fill rows, logical orders and shares are different volume measures.

[Actual installation](../research/moloch-throughput-capital-20261017/closure-cache-deployment/production-rollout-summary.json), [boundary](../research/moloch-throughput-capital-20261017/closure-cache-deployment/boundary-summary.json), [final build/source proof](../research/moloch-throughput-capital-20261017/closure-cache-final-proof/validation.json), [original fills/generations](../research/moloch-throughput-capital-20261017/native-upgrade-live-proof/live-capital-verification.json).

## Actual entry strategy and allocation

Last verified running generation **b27c4699** had eleven reviewed fundamental policies. Exact settings and all four original records are preserved for replacement recovery;595256 recovery is pending the observation above.

- V1:1042 YES/max400 principal;1068 YES/max10,000. Original V1 history/deadlines remain distinct.
- V2 terminal:966 YES,977 NO,1067 YES,865 YES,993 YES,885 YES,1051 NO and851 NO each max10,000 principal;872 YES max3,000.
- V2 cumulative ceiling1m shares, at most16 independently closed ACK-backed entries, at most10,000 shares per order. Cash, depth and global/market limits still apply. The1m ceiling is not a1m-share order.
- SC979/WI1004 were withheld from new flat-cohort origination because existing inventory cannot be relabeled.

Entries use source-bound value and reviewed join/improve/bounded modes. Ordering is a preference, **not an empirically optimal fill/value policy**. Forecast probabilities and exception allowances are assumptions, not calibrated confidence bounds. Profitable high-frequency market making is not established.

Earliest current review expiry:977 at**October17 12:37:03UTC**; others expire later that day. Binary installation does not refresh evidence. Genuine source refresh must preserve clocks and position origins.

## Paper7 traceability

Original supplied paper SHA256:3b7199c71dea9d31da1119476e80481648c937e0519af47b5e0e797db99a5603. Source checkpointf21c8b5f. Code presence is not activation or alpha.

|Recommendation |Concrete implementation |Missing economics/status |
| --- | --- | --- |
|Separate terminal and deadline value |[fundamental_policy.rs](../moloch/live-runtime/src/fundamental_policy.rs) implements distinct objectives/source checks; V2 terminal policies ran before restart. |No calibrated deadline-price model; legacy histories remain distinct. |
|Separate acquisition from resale |[execution_distribution.rs](../moloch/live-runtime/src/execution_distribution.rs) has AcquisitionShadow; [worker.rs](../moloch/live-runtime/src/worker.rs) records actual attempts/fills/closures. |Collection is not a fitted acquisition-probability or mode optimizer. Pending/zero/partial/full outcomes remain distinct. |
|Joint maker fill/value/capital economics |[maker_policy.rs](../moloch/live-runtime/src/maker_policy.rs) and execution_distribution contain estimator/qualification components. |Qualification incomplete; maker gate OFF. No profitable maker activation claimed. |
|Calibrated correlated election posterior |[Atlas](../research/atlas_model/README.md) fitted416 races/1,291 pollster-race observations; [value_surface.rs](../moloch/live-runtime/src/value_surface.rs) preserves research classification. |Publication-clock, revised-archive and horizon problems remain. No qualified live posterior. |
|Whole-account tournament/rank optimization |[prize_utility.rs](../moloch/live-runtime/src/prize_utility.rs) scores supplied scenarios; [probabilistic_portfolio.rs](../moloch/live-runtime/src/probabilistic_portfolio.rs) optimizes a reviewed sleeve. |No calibrated whole-account/opponent generator. Nonnegative-gain sleeve does not generate separate negative-mean rank-convex candidates. |
|Kalshi as causal information |[Kalshi socket](../moloch/external-data/kalshi-websocket.mjs), [source_innovation.rs](../moloch/live-runtime/src/source_innovation.rs), [execution_router.rs](../moloch/live-runtime/src/execution_router.rs) preserve reference/innovation routing. |No qualified profitable lead-lag alpha or assumed contract equivalence. |
|Depth, acquisition probability and capital time allocation |Capital/depth limits and top-ups exist; [alpha_proposal.rs](../moloch/live-runtime/src/alpha_proposal.rs) compares alternatives. |Caller order retained because passive fill probability is unknown. Empirical acquisition allocation missing. |
|Event-driven account awareness and latency |Existing-socket invalidation, priority reconciliation, [serviced_read.rs](../moloch/live-runtime/src/serviced_read.rs), model reload and [decision_trace.rs](../moloch/live-runtime/src/decision_trace.rs) are wired. |Complete socket-to-dispatch attribution/latency target unproved; installed-cache production benefit pending. |

Paper5/6 posterior, curve, nowcast, semantic portfolio and payoff-floor components must not be described as fully active strategies merely because source exists. Older reports retain their dates/statuses; current deployment evidence and this table control the present claim.

## Data and measured speed

At02:20:37UTC, Kalshi collector9240 was collecting and its socket receiving:234 direct mappings plus3 analog references;237 tracked/valid books; zero recorded socket sequence gaps. Analog coverage is not direct contract equivalence. The cache has no pricing authority. This collector runs on the PC through its existing VPS connection; VPS process uptime alone does not establish external-feed availability.

Latest completed-order trace before this release:8,498ms from first recorded preflight to applied ACK, policy/risk538ms, intent101ms. This single observation is not sustained capacity. A copied8,597,641-byte closure journal returned identical records: test-build cold5,838.69ms/warm1,218.56ms with full-byte hashing. That is a component benchmark, not production improvement.

SDK settings remain100 reads/minute with600ms completion pacing and24 total writes/minute with2,500ms pacing. Actual sustained admissions/HTTP latency need separate measurement. [Official API docs](https://sig.thesuper.market/api/v1/docs), rechecked02:11:57UTC, give standard accounts100 reads/30 writes per minute shared across keys, with additional engine throttling. Higher DMM allowances are not established for this account. Batch/multi-leg requests count as one write; open realtime connections do not consume that budget.

Separate header-bound cadence sourceb2b3a8aa/3347be98 is **uncompiled, unactivated and deferred**. No rate increase or performance gain is claimed.

## Next: strategy economics

The human directs infrastructure work to stop unless essential. Finish verification of the already-installed release, then:

1. Compare source-bound join/improve/bounded acquisition outcomes: actual quantity, zero/partial/full fills, timing, pending censoring and reserved/owned capital time. Anonymous prints do not prove hypothetical passive fills; unknown counterfactuals stay unknown.
2. Fit/qualify acquisition probability and conditional value separately from terminal forecasts. Use executable marginal gain, capital time, failures and existing inventory to improve mode choice/allocation.
3. Close posterior/whole-account gaps. New research should supply causal clocks, exact contract/settlement semantics, training/holdout chronology, uncertainty, executable depth and a concrete integration point.

Whether the installed cache enables repeat purchases and reduces idle cash remains open until verified. No winning strategy or reconciled whole-account profitability is established.

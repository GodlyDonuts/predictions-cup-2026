# Moloch: current research handoff

**Updated 2026-10-17 03:22 UTC; recovery/trading observation03:19:50 UTC.** This is the current baseline. Older dated reports are history. No paper is declared fully implemented.

## Production and actual capital

Installed source **f21c8b5f**, binary **b398c7e7**, native **7a74f8c3**. Worker595256 traded after the cache release and added **seven BUY orders /32,994 shares /28,611.865 principal /41 official fill rows**. This includes repeat top-ups on851 and977. The previous02:31 snapshot's zero-new-fill statement is historical and superseded by [hash-verified actual fill evidence](../research/moloch-throughput-capital-20261017/current-research-proof/financial-and-entry-snapshot.json).

**Operational interruption:**595256 stopped03:02:44UTC on disk exhaustion. The owner interrupted the offline candidate build, removed inactive regenerable build caches, and restarted the **same installed release/control as601007 at03:06:17UTC**. Fresh trading by601007 is now **verified**, with healthy realtime/no active STOP, new-PID progress and order count1,441 versus1,440 at the stopped boundary. [Fresh same-release recovery proof](../research/moloch-throughput-capital-20261017/disk-recovery-live-proof/live-verification.json). The original stop is preserved. SDK active=null/globalUnknown0/manualReview=false, all1,440 dispatch ACKs/zero unknown attempts, intact journal tails, all14 original bridge bytes and all four original model generations were verified before restart. [Recovery review](../research/moloch-throughput-capital-20261017/current-research-proof/same-release-recovery-review.json) and [new-PID observation](../research/moloch-throughput-capital-20261017/current-research-proof/recovery-observation.json) preserve this distinction. No financial history, position, loss, forecast, source clock, deadline or activation was reset. No extra account client or new pacing mode was introduced. Inactive build targets were removed and57 files from three redundant offline benchmark directories were preserved byte-for-byte in a633,850,246-byte compressed archive, freeing space; account/native histories remain untouched. The03:07 observation had4,749MiB free. Existing account-queue history occupied about95GiB before recovery, so this cleanup does not establish bounded future storage or high uptime. [Preservation proof](../research/moloch-throughput-capital-20261017/current-research-proof/offline-copy-preservation.json).

|Financial observation |Verified at sequence391890 /03:19:50UTC |
| --- | ---: |
|Initial authorized allocation |95,579.915 SUSQies |
|Cash |19,449.715 |
|Owned position cost |75,993.970 |
|Combined attributed realized P&L |−136.230 |
|New-trading realized / inherited gross |−181.360 /45.130 |
|Official ledger fill rows |324 |
|New filled BUY orders since earlier R11 release |12 |
|Shares / principal in those twelve buys |71,879 /51,313.135 |
|Owned cost / (cash + owned cost) |79.62% |

The allocation ratio uses cost basis. It is not liquidation value, gross exposure, reserved-capital usage or return. Open acquisitions are not realized profit. Dashboard daily P&L, whole-account liquidation P&L and current leaderboard rank remain unverified here. Fill rows, logical orders and shares are different volume measures.

The seven orders since the cache release were977 NO8,430;851 NO9,909;885 YES3,856;977 NO148;851 NO648;977 NO3;1051 NO10,000. The proof records each principal, official-row count and receipt hash. These buys establish additional capital deployment; they do not isolate a causal latency gain from the cache.

[Installed release](../research/moloch-throughput-capital-20261017/closure-cache-deployment/production-rollout-summary.json), [final build/source proof](../research/moloch-throughput-capital-20261017/closure-cache-final-proof/validation.json), [earlier fills/generations](../research/moloch-throughput-capital-20261017/native-upgrade-live-proof/live-capital-verification.json). The installed build passed811 checks with unchanged-library checks explicitly reused; passing checks do not establish alpha.

## Actual entry strategy and allocation

Current recovered generation **b27c4699** has eleven reviewed fundamental policies. Exact settings and all four original records were recovered unchanged by595256 and verified again before the same-release restart. Actual repeat purchases and capital deployment are verified;601007 has completed restoration and resumed trading.

- V1:1042 YES/max400 principal;1068 YES/max10,000. Original V1 history/deadlines remain distinct.
- V2 terminal:966 YES,977 NO,1067 YES,865 YES,993 YES,885 YES,1051 NO and851 NO each max10,000 principal;872 YES max3,000.
- V2 cumulative ceiling1m shares, at most16 independently closed ACK-backed entries, at most10,000 shares per order. Cash, depth and global/market limits still apply. The1m ceiling is not a1m-share order.
- SC979/WI1004 were withheld from new flat-cohort origination because existing inventory cannot be relabeled.

Entries use source-bound value and reviewed join/improve/bounded modes. Ordering is a preference, **not an empirically optimal fill/value policy**. Forecast probabilities and exception allowances are assumptions, not calibrated confidence bounds. Profitable high-frequency market making is not established.

Earliest current review expiry:977 at**October17 12:37:03UTC**; others expire later that day. Binary installation does not refresh evidence. Genuine source refresh must preserve clocks and position origins.

## Paper7 traceability

Original supplied paper SHA256:3b7199c71dea9d31da1119476e80481648c937e0519af47b5e0e797db99a5603. Installed source checkpointf21c8b5f; pending local entry-timing candidates are separate. Code presence is not activation or alpha.

|Recommendation |Concrete implementation |Missing economics/status |
| --- | --- | --- |
|Separate terminal and deadline value |[fundamental_policy.rs](../moloch/live-runtime/src/fundamental_policy.rs) implements distinct objectives/source checks; V2 terminal policies ran before restart. |No calibrated deadline-price model; legacy histories remain distinct. |
|Separate acquisition from resale |[execution_distribution.rs](../moloch/live-runtime/src/execution_distribution.rs) has AcquisitionShadow; [worker.rs](../moloch/live-runtime/src/worker.rs) records actual attempts/fills/closures. |Collection is not a fitted acquisition-probability or mode optimizer. Pending/zero/partial/full outcomes remain distinct. |
|Joint maker fill/value/capital economics |[maker_policy.rs](../moloch/live-runtime/src/maker_policy.rs) and execution_distribution contain estimator/qualification components. |Qualification incomplete; maker gate OFF. No profitable maker activation claimed. |
|Calibrated correlated election posterior |[Atlas](../research/atlas_model/README.md) fitted416 races/1,291 pollster-race observations; [value_surface.rs](../moloch/live-runtime/src/value_surface.rs) preserves research classification. |Publication-clock, revised-archive and horizon problems remain. No qualified live posterior. |
|Whole-account tournament/rank optimization |[prize_utility.rs](../moloch/live-runtime/src/prize_utility.rs) scores supplied scenarios; [probabilistic_portfolio.rs](../moloch/live-runtime/src/probabilistic_portfolio.rs) optimizes a reviewed sleeve. |No calibrated whole-account/opponent generator. Nonnegative-gain sleeve does not generate separate negative-mean rank-convex candidates. |
|Kalshi as causal information |[Kalshi socket](../moloch/external-data/kalshi-websocket.mjs), [source_innovation.rs](../moloch/live-runtime/src/source_innovation.rs), [execution_router.rs](../moloch/live-runtime/src/execution_router.rs) preserve reference/innovation routing. |No qualified profitable lead-lag alpha or assumed contract equivalence. |
|Depth, acquisition probability and capital time allocation |Capital/depth limits and top-ups exist; [alpha_proposal.rs](../moloch/live-runtime/src/alpha_proposal.rs) compares alternatives. |Caller order retained because passive fill probability is unknown. Empirical acquisition allocation missing. |
|Event-driven account awareness and latency |Existing-socket invalidation, priority reconciliation, [serviced_read.rs](../moloch/live-runtime/src/serviced_read.rs), model reload and [decision_trace.rs](../moloch/live-runtime/src/decision_trace.rs) are wired. |Complete socket-to-dispatch attribution/latency target unproved; repeat purchases verified, but causal latency benefit unmeasured. |

Paper5/6 posterior, curve, nowcast, semantic portfolio and payoff-floor components must not be described as fully active strategies merely because source exists. Older reports retain their dates/statuses; current deployment evidence and this table control the present claim.

## Data and measured speed

At02:20:37UTC, Kalshi collector9240 was collecting and its socket receiving:234 direct mappings plus3 analog references;237 tracked/valid books; zero recorded socket sequence gaps. Analog coverage is not direct contract equivalence. The cache has no pricing authority. This collector runs on the PC through its existing VPS connection; VPS process uptime alone does not establish external-feed availability.

Latest completed-order trace before this release:8,498ms from first recorded preflight to applied ACK, policy/risk538ms, intent101ms. This single observation is not sustained capacity. A copied8,597,641-byte closure journal returned identical records: test-build cold5,838.69ms/warm1,218.56ms with full-byte hashing. That is a component benchmark, not production improvement.

SDK settings remain100 reads/minute with600ms completion pacing and24 total writes/minute with2,500ms pacing. Actual sustained admissions/HTTP latency need separate measurement. [Official API docs](https://sig.thesuper.market/api/v1/docs), rechecked02:11:57UTC, give standard accounts100 reads/30 writes per minute shared across keys, with additional engine throttling. Higher DMM allowances are not established for this account. Batch/multi-leg requests count as one write; open realtime connections do not consume that budget.

Separate header-bound cadence sourceb2b3a8aa/3347be98 is **uncompiled, unactivated and deferred**. No rate increase or performance gain is claimed.

## Remaining execution blockers and candidate status

The cache did not eliminate every entry expiry. Actual993 diagnostics at02:58:59.414UTC show plan minted02:58:53.084, validUntil02:58:57.543, validation1,871ms late, with **physicalAttempted=false**. The same snapshot retains exact millisecond clocks. This is a genuine rejected attempt alongside the seven successful buys.

A second active defect was found in bounded sizing: quantity was clipped at the reviewed ceiling, then the actual limit was lowered without re-expanding quantity. With budget2.16, ceiling.44 and asks2@.42+3@.43, five shares are fully fundable at final limit.43/reserve2.15; old sizing selected four. This is a genuine Session regression fixture, not an observed missed live fill.

Local strategy candidate **453fead6** combines immediate native preview/one bounded genuine-budget wait, target-only expensive hint reconstruction and maximum fully limit-funded quantity across actual ask prefixes. Equal quantities retain the lower limit. All existing value ceilings, cash/market/depth/FIFO/top-up/native constraints remain; join/improve behavior is unchanged. Twelve added regressions cover the affected paths, including genuine seven receipts and native reservation. **ARM64 checks/build are in progress; no pass, release or activation is claimed yet.** The earlier interrupted build remains retained. No native/SDK/pacing/source-clock/probability/risk changes are included.

The [actual acquisition audit](../research/moloch-acquisition-audit-20261017/README.md) independently reproduces its exact derived summary from a pinned private four-order snapshot. Selected orders took13.494–16.365s from native quote creation to physical attempt; attempt-to-first-execution0.492–1.220s, full execution up to6.589s. First executions reached their original REST receipts8.409–8.918s later. These clocks are distinct; neither reservation time nor observation delay is venue fill latency. All four use boundedLimitMayRest and still hold22,343 shares costing19,428.305;85.300 improvement versus frozen limits is not profit. The incomplete selected sample cannot estimate fill probability or compare modes. No raw account receipts are published.

## Next: strategy economics

The human directs infrastructure work to stop unless essential. Disk recovery is essential for existing production uptime; no new infrastructure program is authorized by this report. Actual601007 trading is verified. Continue with:

1. Compare source-bound join/improve/bounded acquisition outcomes: actual quantity, zero/partial/full fills, timing, pending censoring and reserved/owned capital time. Anonymous prints do not prove hypothetical passive fills; unknown counterfactuals stay unknown.
2. Fit/qualify acquisition probability and conditional value separately from terminal forecasts. Use executable marginal gain, capital time, failures and existing inventory to improve mode choice/allocation.
3. Close posterior/whole-account gaps. New research should supply causal clocks, exact contract/settlement semantics, training/holdout chronology, uncertainty, executable depth and a concrete integration point.

Research agents should start from this handoff and the eight-row traceability table, not historical completion headlines. No paper is declared fully implemented and no winning strategy or reconciled whole-account profitability is established.

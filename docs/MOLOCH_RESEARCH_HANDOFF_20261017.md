# Moloch: current research handoff

**Updated 2026-10-17 03:59:35 UTC; financial audit boundary 396960 / 338 fills; live verification 03:57:59 UTC.** This is the current baseline. Older dated reports are history. No paper is declared fully implemented.

## Production and actual capital

Installed source **a557c779**, binary **7d44e839**, native **7a74f8c3**. Sole worker **607949** is **verified trading at 03:57:59 UTC**, with fresh progress from that process, healthy realtime and no active STOP. All 14 financial bridge bytes, four original model records and eleven policies recovered unchanged. [Actual live verification](../research/moloch-throughput-capital-20261017/entry-strategy-live-proof/live-verification.json). The original [03:43:30 installation receipt](../research/moloch-throughput-capital-20261017/entry-strategy-deployment/production-rollout-summary.json) correctly records its earlier restoring state. The latest proof has 338 actual fill rows and zero new post-upgrade fills; it does not establish the fixes' performance or profit benefit.

This release fixes active bounded quantity under-sizing at the final marginal ask, removes unconditional waiting before actual native preview, and reconstructs expensive hint proofs only for the selected target. 836 checks and optimized ARM64 release pass; reuse of unchanged compiled checks is explicit. Production latency improvement, additional fills and profit from these fixes remain unproved. No native/SDK/cadence/source-clock/probability/risk/activation change was made.

An earlier disk-full interruption was recovered before this upgrade. Essential retention preserved the original histories in verified compressed archives; current queue generations and recovery proofs were excluded. About 8.2 GiB was free at 03:55 UTC. This is a dated capacity observation, not an uptime guarantee. [Recovery evidence](../research/moloch-throughput-capital-20261017/current-research-proof/same-release-recovery-review.json) and [preservation receipt](../research/moloch-throughput-capital-20261017/entry-strategy-proof/superseded-queue-preservation-summary.json). Infrastructure work remains deferred unless required to keep existing production operating.

|Financial observation |338 fills; audit sequence 396960, live sequence 397271 |
| --- | ---: |
|Total authorized allocation |95,579.915 SUSQies |
|Cash |16,092.385 |
|Owned position cost |79,347.690 |
|Combined attributed realized P&L |−139.840 |
|New-trading realized / inherited gross |−184.970 /45.130 |
|Official ledger fill rows |338 |
|Filled BUY orders since cache release |14 |
|Shares / principal in those fourteen buys |39,152 /32,685.115 |
|Owned cost / (cash + owned cost) |83.14% |
|Native modeled reserved cash, separate from owned cost |3,741.075 |

The allocation ratio uses cost basis. It is not liquidation value, gross exposure, reserved-capital usage or return. Open acquisitions are not realized profit. Dashboard daily P&L, whole-account liquidation P&L and current leaderboard rank remain unverified here. Fill rows, logical orders and shares are different volume measures.

At the earlier03:19 observation, seven of the later fourteen BUY orders since the cache release were977 NO8,430;851 NO9,909;885 YES3,856;977 NO148;851 NO648;977 NO3;1051 NO10,000. The proof records each principal, official-row count and receipt hash. At the stopped boundary there are55 new rows since cache release:49 BUY rows in14 orders (39,152shares/32,685.115principal), and6 SELL rows in3orders (1,082shares/715.920proceeds). [Exact aggregation](../research/moloch-throughput-capital-20261017/entry-strategy-deployment/capital-aggregation.json). These buys establish additional capital deployment; they do not isolate a causal latency gain from the cache.

[Current installed release](../research/moloch-throughput-capital-20261017/entry-strategy-deployment/production-rollout-summary.json), [836-check source/build evidence](../research/moloch-throughput-capital-20261017/entry-strategy-proof/strategy-check-summary.json), and [earlier fills/generations](../research/moloch-throughput-capital-20261017/native-upgrade-live-proof/live-capital-verification.json). Passing checks establish implementation behavior, not trading alpha. The repository's `data/portfolio/latest.json` is an October 3 historical snapshot; it is not the financial source for this report.

## Actual entry strategy and allocation

Current recovered generation **b27c4699** has eleven reviewed fundamental policies. Exact settings and all four original model records are verified unchanged in worker 607949. Repeat purchases and capital deployment preceded this release; no additional fill from the new worker is claimed at the latest proof.

- V1:1042 YES/max400 principal;1068 YES/max10,000. Original V1 history/deadlines remain distinct.
- V2 terminal:966 YES,977 NO,1067 YES,865 YES,993 YES,885 YES,1051 NO and851 NO each max10,000 principal;872 YES max3,000.
- V2 cumulative ceiling1m shares, at most16 independently closed ACK-backed entries, at most10,000 shares per order. Cash, depth and global/market limits still apply. The1m ceiling is not a1m-share order.
- SC979/WI1004 were withheld from new flat-cohort origination because existing inventory cannot be relabeled.

Entries use source-bound value and reviewed join/improve/bounded modes. Ordering is a preference, **not an empirically optimal fill/value policy**. Forecast probabilities and exception allowances are assumptions, not calibrated confidence bounds. Profitable high-frequency market making is not established.

Earliest current review expiry:977 at**October17 12:37:03UTC**; others expire later that day. Binary installation does not refresh evidence. Genuine source refresh must preserve clocks and position origins.

## Ordinary trading remains distinct

The optional empirically qualified maker gate is absent/OFF; **ordinary paired-spread entries remain enabled**. Installed executionModel is null and ordinarySideEdge=enforce. Native spread/flow/depth/funding checks and a fresh microprice proxy edge of.001 permit entries; this is not a calibrated filled-value or profitable maker model. Native quantity starts at10,000 and clips to depth, paired funding, cash and exposure room.

Unmanaged ordinary inventory normally seeks.005/share gain, but its60-second maximum-age policy may override that floor and exit at current executable bids. Fundamental V2 terminal cohorts use their distinct reviewed holding objective. Actual ordinary1033 NO60 cost39.90 and later proceeds39.90: realized0, despite a sell limit allowing a loss. This single trade does not explain the earlier−181.36 aggregate. Original entry depth/room and fill execution/observation clocks are required to explain exact sizing/age latency; quote-to-quote time alone cannot. [Ordinary guard](../moloch/live-runtime/src/ordinary_side_edge.rs), [worker](../moloch/live-runtime/src/worker.rs), [native strategy](../moloch/native/src/engine.rs).

## Exchange capabilities and activation

The [source-bound installed feature state](../research/moloch-throughput-capital-20261017/entry-strategy-live-proof/current-feature-state.json) keeps `collateralAccounting=false`, `executionModel=null` and `executionDistribution=null`. The optional empirical maker model is therefore inactive; ordinary spread trading is still enabled. The package scanner and prospective package lifecycle exist, but the current control has no trusted package certificates or allocation reviews. These settings do not establish any profitable split, merge, conversion or atomic-package trade. The retained actual ledger audit must distinguish ordinary buys/sells, complementary-buy netting and genuine package operations; an opposite buy that nets inventory is not proof of a package strategy.

Kalshi is a reference-data source, not a second trading account. Atlas, election curves, nowcasting, semantic payoff floors and tournament utility modules are research or selectively wired components; source presence does not make them active allocation strategies. See the traceability table below and [Paper 5 capability history](MOLOCH_PAPER5_20261016.md). This paragraph describes the source-bound installed control, not a new activation.

## Paper7 traceability

Original supplied paper SHA256:3b7199c71dea9d31da1119476e80481648c937e0519af47b5e0e797db99a5603. Installed source is a557c779; the source/build and deployment evidence above identify its exact bytes. Code presence is not activation or alpha.

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

## Execution delays and latest fixes

The cache did not eliminate every entry expiry. Actual993 diagnostics at02:58:59.414UTC show plan minted02:58:53.084, validUntil02:58:57.543, validation1,871ms late, with **physicalAttempted=false**. The same snapshot retains exact millisecond clocks. This is a genuine rejected attempt alongside the seven successful buys.

A second active defect was found in bounded sizing: quantity was clipped at the reviewed ceiling, then the actual limit was lowered without re-expanding quantity. With budget2.16, ceiling.44 and asks2@.42+3@.43, five shares are fully fundable at final limit.43/reserve2.15; old sizing selected four. This is a genuine Session regression fixture, not an observed missed live fill.

Installed strategy release **a557c779** combines immediate native preview/one bounded genuine-budget wait, target-only expensive hint reconstruction and maximum fully limit-funded quantity across actual ask prefixes. Equal quantities retain the lower limit. All existing value ceilings, cash/market/depth/FIFO/top-up/native constraints remain; join/improve behavior is unchanged. Twelve added regressions cover the affected paths, including genuine seven receipts and native reservation. **836 ARM64 checks and the optimized release/engine probe pass**; unchanged compiled-library/worker/SDK/native checks are explicitly reused. All262 built source files match published Git bytes exactly or by CRLF normalization. Binary 7d44e839 is now **verified trading as 607949**; additional fills and economic benefit from this worker remain unproved. [Actual checks, retained failed fixture and reuse/source evidence](../research/moloch-throughput-capital-20261017/entry-strategy-proof/strategy-check-summary.json). The initial native-reservation fixture failed the unchanged write-gap rule; the corrected fixture first proves denial, then rebuilds after the real gap using the same original receipts. Original failure logs and the earlier interrupted build are retained. No native/SDK/pacing/source-clock/probability/risk changes are included.

The [actual acquisition audit](../research/moloch-acquisition-audit-20261017/README.md) independently reproduces its exact derived summary from a pinned private four-order snapshot. Selected orders took13.494–16.365s from native quote creation to physical attempt; attempt-to-first-execution0.492–1.220s, full execution up to6.589s. First executions reached their original REST receipts8.409–8.918s later. These clocks are distinct; neither reservation time nor observation delay is venue fill latency. All four use boundedLimitMayRest and still hold22,343 shares costing19,428.305;85.300 improvement versus frozen limits is not profit. The incomplete selected sample cannot estimate fill probability or compare modes. No raw account receipts are published.

## Next: strategy economics

The human directs infrastructure work to stop unless essential. Production trading is verified; prioritize the strategy work below:

1. Compare source-bound join/improve/bounded acquisition outcomes: actual quantity, zero/partial/full fills, timing, pending censoring and reserved/owned capital time. Anonymous prints do not prove hypothetical passive fills; unknown counterfactuals stay unknown.
2. Fit/qualify acquisition probability and conditional value separately from terminal forecasts. Use executable marginal gain, capital time, failures and existing inventory to improve mode choice/allocation.
3. Close posterior/whole-account gaps. New research should supply causal clocks, exact contract/settlement semantics, training/holdout chronology, uncertainty, executable depth and a concrete integration point.

Research agents should start from this handoff and the eight-row traceability table, not historical completion headlines. No paper is declared fully implemented and no winning strategy or reconciled whole-account profitability is established.

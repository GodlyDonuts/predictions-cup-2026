# Moloch research baseline

New05:42UTC: [b796e2c1 profit preparation](../research/moloch-profitable-preparation-20261017/build-proof/README.md) checked850/release, not yet deployed; R20 remains sole611948. [New prospective price cohort](../research/moloch-victory-directive-20261017/prospective-v2/README.md):17,349 decisions/23,697 labels, zero selected after-cost trades across all horizons/comparators. No alpha/model activation. Dated observations below remain historical.

Latest: source4bbeae46 /binaryc734ca4c /sole611948 **verified trading/realtimeHealthy/noSTOP at04:54:38UTC**, zero post-upgrade fills at that proof. [Fresh-process evidence](../research/moloch-profitable-exit-refresh-20261017/live-proof/live-verification.json). Exact stopped boundary351fills/cash9,463.510/owned85,976.565/combined attributed realized−139.840. [Actual installation](../research/moloch-profitable-exit-refresh-20261017/deployment/README.md); [new Victory Directive and evidence map](../research/moloch-victory-directive-20261017/README.md). Prior verified R19 trading observation follows.

Read [the current research handoff](MOLOCH_RESEARCH_HANDOFF_20261017.md) first. It separates deployed strategies, source-only components, missing economics, capital/fill evidence and next work. Older dated reports are history; previous baseline versions remain in Git.

Prior R19 **a557c779 / binary 7d44e839 / native 7a74f8c3**, sole worker 607949 is **verified trading at 04:01:26 UTC**, healthy realtime/noSTOP. Two post-upgrade reviewed BUY orders filled: 7,438 shares /6,453.985 principal /nine official rows. [Actual fresh proof](../research/moloch-throughput-capital-20261017/entry-strategy-fill-proof/live-verification.json). The sizing/preparation fixes passed 836 checks and the optimized ARM64 build; unchanged compiled checks were reused explicitly. All original history, 14 bridge bytes, four model generations and eleven policies are preserved.

Latest ledger: **347 fills, cash 9,638.400, owned cost 85,801.675, combined attributed realized −139.840** (new −184.970 /inherited +45.130). Owned cost/(cash+owned cost)=**89.90%**, not return or liquidation value. Open holdings are not realized profit; whole-account dashboard/rank P&L remains unverified. The independent FIFO attribution below covers its explicitly frozen earlier 338-fill boundary.

|Area |Current claim |
| --- | --- |
|Fundamental/capital |Eleven policies; larger V2 budgets/top-ups implemented. Repeat purchases/capital deployment verified and same-release recovery now verified. |
|Probabilities |Reviewed assumptions; no calibrated live correlated posterior. |
|Mode choice |Join/improve/bounded and actual outcome collection exist; empirical optimizer missing. |
|Ordinary / maker economics |Ordinary entries remain active; [actual FIFO audit](../research/moloch-realized-attribution-20261017/README.md) attributes all −184.97 new realized loss to them. Optional empirical maker gate OFF. |
|Kalshi |Reference feed last checked 02:20 UTC: 234 direct/3 analog mappings. Qualified profitable lead-lag alpha unproved. |
|Account/rank |Components/reviewed sleeve exist; calibrated whole-account/rank generation incomplete. |
|Speed |Repeat buys verified; remaining plan expiries observed. a557c779 sizing/preparation fixes verified trading; additional fills verified; causal latency/economic benefit unproved. Cadence work deferred. |

The earlier 338-fill finite audit attributes 0 realized /57,011.775 held principal to reviewed fundamental entries and +45.13 realized /22,335.915 held to inherited lots. Open outcomes are unproved.

No paper is declared fully implemented. Strategy/acquisition economics are the priority. Infrastructure stops unless essential; improve mode choice/allocation from observed outcomes/capital time.

[Current rollout](../research/moloch-throughput-capital-20261017/entry-strategy-deployment/production-rollout-summary.json), [current fill proof](../research/moloch-throughput-capital-20261017/entry-strategy-fill-proof/live-verification.json), [dated history](CURRENT_STATE.md), [task155](https://github.com/GodlyDonuts/predictions-cup-2026/issues/155).

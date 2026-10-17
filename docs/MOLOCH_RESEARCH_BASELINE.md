# Moloch research baseline

Updated **2026-10-17 03:59:35 UTC**.

Read [the current research handoff](MOLOCH_RESEARCH_HANDOFF_20261017.md) first. It separates deployed strategies, source-only components, missing economics, capital/fill evidence and next work. Older dated reports are history; previous baseline versions remain in Git.

Installed **a557c779/binary7d44e839/native7a74f8c3**, sole worker 607949 is **verified trading at 03:57:59 UTC**, healthy realtime/noSTOP; zero new post-upgrade fills at this observation. [Actual rollout](../research/moloch-throughput-capital-20261017/entry-strategy-deployment/production-rollout-summary.json). The active sizing/preparation fixes passed836 checks/optimized ARM64 release; unchanged compiled checks reused explicitly. All14 original bridge bytes/four original model records/eleven policies and original clocks/limits/history preserved.

Stopped ledger396960:338fill rows, cash16,092.385, owned79,347.690, combined attributed realized−139.840 (new−184.970/inherited45.130). Owned cost/(cash+owned cost)=83.14%; native modeled reservations3,741.075 are separate. Since cache release:14BUYs/39,152shares/32,685.115principal and3SELLs/1,082shares/715.920proceeds. [Exact aggregation](../research/moloch-throughput-capital-20261017/entry-strategy-deployment/capital-aggregation.json). Open holdings are not realized profit; whole-account dashboard/rank P&L remains unverified.

|Area |Current claim |
| --- | --- |
|Fundamental/capital |Eleven policies; larger V2 budgets/top-ups implemented. Repeat purchases/capital deployment verified and same-release recovery now verified. |
|Probabilities |Reviewed assumptions; no calibrated live correlated posterior. |
|Mode choice |Join/improve/bounded and actual outcome collection exist; empirical optimizer missing. |
|Market making |Economic qualification incomplete; optional gate OFF. Ordinary spread/microprice-proxy entries remain active with60s age exits. |
|Kalshi |Live reference feed,234 direct/3 analog mappings; qualified profitable lead-lag alpha unproved. |
|Account/rank |Components/reviewed sleeve exist; calibrated whole-account/rank generation incomplete. |
|Speed |Repeat buys verified; remaining plan expiries observed. a557c779 sizing/preparation fixes verified trading; actual latency/fill benefit not yet verified. Cadence work deferred. |

No paper is declared fully implemented. Strategy/acquisition economics are the priority. Infrastructure stops unless essential; improve mode choice/allocation from observed outcomes/capital time.

[Current rollout](../research/moloch-throughput-capital-20261017/entry-strategy-deployment/production-rollout-summary.json), [fill proof](../research/moloch-throughput-capital-20261017/native-upgrade-live-proof/live-capital-verification.json), [dated history](CURRENT_STATE.md), [task155](https://github.com/GodlyDonuts/predictions-cup-2026/issues/155).

# R27 first300s: observed improvement, high tail delay remains

The frozen first-fresh selection compares R26 12:13:33.595–12:18:33.595 against R27 **13:59:43.091–14:04:43.091 UTC** on October17. Both include every admitted native book, including explicit invalid records, under the unchanged10s policy. The new original packet closes at native490191/report14:05:06.450, thirteen raw members, SHA d988c4042c34299a3e31fc28c8915590184b4fa681ba02d9d79e11bd8fd5419d. The unchanged [reader](compare_r27_clocks.py) successfully verifies every member, raw seal, exact anchored/final prefix and all five original generation rows. [Full result and pins](r27-first-300s-comparison.json); [prospective protocol](r27-first-window-protocol.json).

| Metric | R26 | R27 |
|---|---:|---:|
| All native book events |213|389|
| Realtime / REST |190 /23|364 /25|
| Median receipt-to-processing |14588ms|8212ms|
| Realtime median |17467.5ms|8315ms|
| REST median |95ms|46ms|
| All-book p90 / maximum |31432 /64508ms|27160 /34041ms|
| Stale at processing despite source fresh at receipt |121 /213 (56.81%)|172 /389 (44.22%)|
| Receipt-to-processing over10s |119|167|
| Explicit invalid REST books |10|10|
| Temporal errors / missing fields / window gaps |0 /0 /0|0 /0 /0|

Observed median is43.7% lower overall and52.4% lower for realtime. The new window still has substantial27.16s p90 delay; this is not acceptable evidence of maximal-frequency execution or complete freshness. Event counts differ with live market activity. These are different live times, with one node-only Oregon generation difference and the old result already inspected; no randomized causal attribution or universal speed guarantee follows. No underlying original provider-body membership or financial replay is inferred from admitted-event seals. The inspected new report remains382 fills; no additional fill or profit is established by this comparison.

## Shutdown-handler callgraph correction

Independent source review at14:04 found an important R27 limitation: the exact FUNDAMENTAL_NOT_ELIGIBLE is raised by `Cohorts::bind_quantity`, which worker832 invokes **before** the physical_decision catch. The earlier explanation that check_bound/assess produced this same entry code was wrong. Current143 helper tests injected the error and did not exercise this callsite. R27 remains operating, but its claimed complete repair of the R26 stop is withdrawn. A minimal combined original-bind/physical-decision handler and actual-bind regression are being prepared while production continues. Original positive quiet/untouched/current-quote/book-absence/10s/unknown controls remain required; no general error swallow is proposed.

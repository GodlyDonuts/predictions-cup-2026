# R28 first300s: no measured speed improvement

The frozen selection compares R27 **13:59:43.091–14:04:43.091** with R28 **15:23:08.392–15:28:08.392 UTC** on October17. The first R28 progress was observed at15:23:08.971441; the original completed observer, separately pinned first capture and final capture authenticate selection before measurement. No later favorable interval was substituted. [Protocol](r28-first-300s-protocol.json), [unchanged wrapper](compare_r28_clocks.py), [root-rerun result and every original pin](r28-first-300s-comparison.json).

| Metric | R27 | R28 |
|---|---:|---:|
| Native book events |389|280|
| Realtime / REST |364 /25|256 /24|
| All-book median receipt-to-processing |8,212ms|11,054ms|
| Realtime median |8,315ms|12,475ms|
| All-book p90 / maximum |27,160 /34,041ms|30,598 /36,102ms|
| Realtime p90 |27,160ms|31,287ms|
| Realtime stale at processing despite source fresh at receipt |172 /364 (47.25%)|148 /256 (57.81%)|
| REST median |46ms|128ms|
| REST stale at processing |0|0|
| Explicit invalid REST books |10|10|
| Window gaps / temporal clock mismatches |0 /0|0 /0|

The observed timing is worse. R28 repaired a real shutdown path; this measurement does not establish an execution-speed gain. Different market periods, node-generation history, history size/cache state and concurrent reduced-priority R29 compilation prevent causal attribution. The old outcome had already been inspected: this is an observational diagnostic, not a confirmatory experiment. No new fill or profitability is established by book timing.

The wrapper verifies the exact original five→six generation prefix, all six generations before the new window, no seventh during it, unchanged15 bridge bytes and every original packet member/seal. Root reran it independently: all metrics and evidence fields match the agent's earlier run; only raw protocol/wrapper hashes differ because this Windows checkout has CRLF and the agent used exact Git LF. Their normalized LF hashes match exactly; the published aggregate retains the root's actual raw hashes.

Private final archive4,989,961bytes/SHAa6cdee921d9593a097d7e0dddf288e93843df8c42a34e0a8471209b206d7f559; first archive4,850,198bytes/SHAbbc76c2aad0315513c658ef63092bcd61da770d33e8eb0fc5f37716b22f77264; original completed observer1,311bytes/SHA5c1e1f110bc3e5b9069b963219e0cb593a55350ce4b6f42bbc23bf0057c4925d. The retained first failed metadata pin contained a copied65-character TTY hash; actual remote64-character SHA and downloaded bytes were independently checked before the successful run. The metric/window code was not changed.

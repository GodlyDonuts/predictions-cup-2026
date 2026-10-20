# Exact entity-layout cache reduces observation-building time

The live collection profile found observation construction in 43.58% of sampled main-thread stacks. `ApiEpisode.observe` rebuilt the same seven identity columns row by row, even when the ordered entity layout was unchanged. The checked change caches only those default identity columns as an immutable float32 matrix keyed by the exact ordered entities, then copies them into each new observation.

Prices, source ages, positions, references, masks, resource clocks and delivered data remain dynamic. Execution-acknowledgment, private-fill and trade-print rows still overwrite their contract identities from the current delivered rows. In particular, reusing an acknowledgment index for another exchange does not reuse its previous contract identity. Entity insertion or reordering rebuilds the matrix. Returned observations own their output arrays; callers cannot mutate the cached matrix through them. There is one retained layout per environment, not an unbounded history of layouts.

## Recorded-source conformance and timing

Frozen checked source `157c873a3ac6970a9fc975acff21d774bee66b5f` was compared with the exact `ApiEpisode.observe` method extracted from pinned source `9383f27ce4f5868c94a39328a2e38db878c81d72`. Both methods operated on the same already-inspected TRAIN environment, through the full 469-feature observation stack. The runner alternated call order, advanced original market time, changed local packet/builder state, excluded ten warm-up pairs from timing, and compared all fields, array bytes/dtypes/shapes, masks, state and entity order.

| Result | Observation |
| --- | --- |
| Complete observation pairs | 90, all bitwise equal |
| Timed pairs | 80 |
| Maximum output entities | 2,305 |
| Reference median | 8.64285 ms |
| Cached median | 7.51995 ms |
| Reference/cached median ratio | 1.1493 |

This is approximately 13.0% less elapsed time for the measured CPU observation call, or 14.9% greater observation-call throughput. It is **not** a measured 14.9% overall training speedup, GPU benchmark, multi-day evaluation or economic improvement. The live learner and existing collectors continued during the paired measurements; alternating order reduces bias but does not eliminate scheduler noise.

[Exact measurements and source pins](entity-identities-real-source-001.json), [original runner bytes](entity-identities-real-source-001-runner.py.gz). Decompressed runner SHA256 matches the report. No actor model, optimizer, GPU diagnostic or HTTP client was created by this comparison.

Twenty-three focused checks passed in 5.520 seconds. The complete action suite ran 386 checks in 70.416 seconds, passing with five CUDA-only checks skipped. Coverage includes exact matrix values, immutability, layout/order invalidation, receipt identity replacement, dynamic prices/ages/holdings, full named API observations and sealed reconstruction/training.

## Queued adoption

The old waiting controller 26996/6928 was cancelled through its existing interface and both processes were verified absent before replacement. Controller 17540/launcher27064 is verified live, waiting for the same complete GPU020 generation and all replacements. Trainer11404 remains on frozen544a3986 with unchanged unfinished work. [Queue004 evidence](population-021-queued-004.json).

The queued GPU021 source is157c873a. It retains public-trade delivery and the previous learning-utilization profiles. `episode_observation_digest=legacy` is explicit: the separately published packed-hashing candidate remains disabled until GPU byte conformance, capacity and timing are measured. Existing serial capacity/startup checks and fresh evaluations remain pending; this queued CPU optimization is not yet active training.

At the queue observation, GPU020 had179,381 decisions and196 committed updates. Seven completed development evaluations remained flat or negative. Full requested chronology, execution/valuation calibration and independent positive ROI remain incomplete. No production financial state, STOP, activation, account client, protected calendar or live collector changed.

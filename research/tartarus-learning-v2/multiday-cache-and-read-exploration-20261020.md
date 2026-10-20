# Multi-day replay and read exploration — October 20, 22:12 UTC

The original SIG semantic pass and bounded replay cache are complete. Endpoint-balanced read sampling is implemented and checked, with a small paired CPU experiment showing smaller losses. No independently profitable model or production deployment follows from these results. GPU027 continues on its original frozen source and sampling law.

## Original SIG history now usable as a bounded research cache

All 148,181 unique original envelopes passed semantic normalization. The output contains 5,062,890 book observations, 71,506 reported trade observations and 24,209 gap records. All 174 extracted originals previously passed their member hashes; the compiler rechecked the original file hashes at completion. It retained 87 explicit unlinked chain boundaries rather than inventing continuity.

The data spans October 15 07:25:04.423 UTC through October 19 14:13:49.249 UTC, across 237 exchanges. Fit ends exclusively October 17 00:00 UTC. This is partial original history with recorded missing intervals, not the entire competition or complete hidden exchange state.

The automatic packager completed and the existing SQLite replay builder produced 5,087,099 market/world records and 71,506 trade/execution records in an 11,893,026,816-byte database. Cache manifest SHA256 is `f34facef301a3005298008e959108f295acadd2e40ae67d401db52dab081daa1`. The package retains `trainingAdmitted:false`, missing external observations and incomplete coverage. Prices, quantities, original clocks and gaps were not changed by packaging or caching.

Evidence: [semantic completion](sig-multiday-semantic-complete-20261020.json), [cache completion and hashes](sig-multiday-replay-cache-20261020.json).

## External receipt compilation

Checked source `f3d54c1189e96870425c4bd0aeb30e73137cfeee` adds a streaming compiler for the existing original Kalshi and Polymarket REST receipt adapters. A local job is processing a frozen 3,929-entry archive index and SHA-pinned universes. It uses SQLite for deduplication and sorting, not a full in-memory history.

Snapshots with the same source filename must be exact byte prefixes of the largest recorded variant. A truncated newer gzip is rolled back completely before an older complete snapshot is considered. Changed bytes, conflicting filename histories, changed reference identities and independent-calendar captures fail. Original unsupported WebSocket frames remain preserved; this compiler does not reconstruct them or claim complete external coverage. Twelve compiler/adapter tests pass, including partial-snapshot rollback, orientation, receipt timing, duplicate removal, corruption and calendar/path guards.

The external job is still running. Its output is not yet part of the longer SIG cache. There are no account calls or new collectors in this compilation. [Frozen job evidence](multiday-external-job-20261020.json).

## Endpoint-balanced sampling

The inspected founder had 2,608 read targets, including a single SIG realtime-token target. Flat target sampling gave capabilities with large catalogs much more initial probability than singleton operations. In the exact eight-world rollout, it disconnected 78 times and never sampled token renewal.

The opt-in `endpoint_logmeanexp_v1` sampling law subtracts the logarithm of the currently eligible target count for each `(venue, endpoint)` category. At equal neural target logits, each available category gets equal total probability. Neural logits remain free to favor any target. The original action-legality masks, prices, quantities, deadlines, decision domains and account budgets remain intact. No forced trade or decision-count cap is added.

The sampling law is pinned in checkpoint metadata and the on-policy behavior contract. Legacy checkpoints default to their exact prior flat law. Changing an inherited policy's law requires explicit migration, fresh optimizer state, new trajectories and new scores. Actor inference, action-only evaluation, recurrent teacher forcing and evolution distance probes use the same law. Twenty-two focused sampling/population/evaluation checks pass; earlier action-only, historical-neural and sealed-replay checks also passed. CUDA tests were deliberately skipped while the single GPU learner remained active.

## Small paired measurement

The CPU experiment used the same GPU027 initial founder weights, the same two already exposed ten-minute training windows, paired seeds and the same recorded 10 ms compute-latency assumption. One case used ordinary execution assumptions and one stress assumptions. It ran no optimizer and changed no checkpoint parameter. This compares the sampling law, not learning progress or independent performance.

| Case | Flat marked ROI | Balanced marked ROI | Flat / balanced fills |
| --- | ---: | ---: | ---: |
| Ordinary | -4.0751125% | -0.084265% | 27 / 5 |
| Stress | -21.48275% | -0.3398375% | 31 / 9 |

These are still losses. Much of the difference comes with fewer fills; it does not demonstrate alpha. In the stress case the balanced policy sampled three token renewals, versus none in the flat case. However, neither balanced case sampled a SIG book read, and its contract-targeted actions still lacked a valid cached book. Book acquisition and linking delivered information to trading decisions remain unresolved. Two short cases cannot establish robust superiority, the human's complete-day reward, or future ROI. The GPU027 fifty-minute selection results are not directly comparable to this ten-minute study.

[Paired result and provenance](endpoint-balanced-paired-result-20261020.json). The next work is integrating verified longer three-venue information, improving useful book acquisition, and training a fresh chronological lineage. The October 21–25 independent calendar remains reserved and unopened. Current development weights and partial-history scores cannot be relabeled as independent qualification. No production financial state changed.

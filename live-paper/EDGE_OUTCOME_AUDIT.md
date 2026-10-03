# Executable outcome attribution

Run node live-paper/edge-outcome-audit.mjs against the existing restricted runtime. This reads the active frozen bank, current paper stats and the single collector's public-books journal. It does not start a worker, call the API, submit orders or change accounting.

Every complete public record is replayed through the publication scope/continuity/hash validator before joining recent labels to exact entry receipt/version and exit sequence/hash. The audit checks the fixed one-share, zero-fee, one-percent depth, latency, horizon and one adverse tick per side assumptions. Invalid references, changed costs, insufficient observed depth and inconsistent exact accounting fail instead of being reinterpreted.

The identity is: modeled net = executable bid movement minus entry spread minus two adverse ticks. Raw crossed return still pays the entry spread. The output separates warm development labels from those prepared after the frozen cutoff. Medians and positive counts describe only the recent label window retained in stats, not all labels or portfolio P&L. Hypothetical probes are correlated and conditional on fill/data availability. No maker fill assumption is substituted, and no fee is invented.

At 2026-10-03 01:21:16 UTC, the actual recent window included seven prospective labels across four markets. None had positive raw crossed return, and the median executable bid movement was zero. Median entry spread was 0.005 SUSQ and modeled net was -0.015 SUSQ per one-share probe. This tiny sample is diagnosis, not proof of an optimal strategy, profitability or a population estimate. Preserve negative and unavailable outcomes while gathering subsequent coherent observations.

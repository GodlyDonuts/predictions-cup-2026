# Historical sample validation throughput — October 21

The Polymarket binary reader now validates records in bounded NumPy chunks instead of individual Python iterations. It retains exact manifest/receipt/asset hashes, strict timestamp ordering across chunk boundaries, finite unit prices, valid flags and receipt indices, original asset/receipt scope, and unchanged causal binary-search lookup. No action, request rate, observation clock, source row, or reward changes.

A cold fit-only 60-second virtual wait profiled at 15.659 seconds before and 1.209 seconds after. Both admitted 99 SIG, 1,201 Kalshi and 1,765 Polymarket reads and received 2,959 historical responses. Profiling overhead makes this comparison unsuitable as a whole-training speed claim.

A separate unprofiled validation pass checked **all 24,557,593 records in 756 training assets** with both implementations and verified every original asset hash. Scalar checks took 10.786 seconds; vectorized checks took 0.509 seconds: **21.2× faster for that validation stage**. Source manifests and exact timings are in [the recorded benchmark](sample-validation-benchmark-20261021.json). This accelerates cold source initialization, not every already-warm request. Full training throughput improvement remains unmeasured.

Nineteen focused tests pass, including 300 scalar/vectorized acceptance comparisons, chunk-edge ordering, invalid prices/flags/indices, receipt scope, large integer bounds, source mutation, and causal delivery. Current frozen GPU population 037 remains unchanged while learning; this candidate can be admitted at a subsequent source boundary. Independent Oct21–25 data were not opened. This change makes no production financial operation and establishes no ROI.

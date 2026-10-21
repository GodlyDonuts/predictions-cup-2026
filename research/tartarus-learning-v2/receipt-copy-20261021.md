# Independent receipt state with shared immutable depth

Simulated realtime receipts formerly deep-copied the whole SharedLiquidity object, including every nested price-level tuple. `liquidity_snapshot.receipt_snapshot` now shares depth only when the exact Book and all its fields are recursively immutable primitive values. It independently copies the mutable depletion dictionary and invalidation state, preserving the original source-state clock. Unexpected fields, subclasses, mutable levels or custom state use the original full deepcopy protocol.

This preserves delayed-receipt isolation: later world book replacements, depletion and invalidation cannot change an already queued receipt, and two receipts cannot mutate each other's liquidity. No global Book deepcopy override is installed. Clocks, rate admissions, API actions, receipt ordering, prices, queue hypotheses, fills and rewards are unchanged.

29 focused receipt-isolation, realtime, snapshot-clock and sealed-episode checks passed. A three-trial alternating microbenchmark used the same 5,000 already-inspected original SIG books. Copied values and source-row digests matched exactly; median copy time was 0.373546 seconds for deepcopy versus 0.151158 seconds for the candidate, **2.47× for this component**. This is not an end-to-end learning speedup or profitability result. [Full benchmark](receipt-copy-benchmark-20261021.json).

Population030 continues on its original frozen source. The candidate subsequently reconstructed all6126 decisions,107 batches and eight worlds with matching original observation, clock, reward and financial hashes. [Complete parity proof](receipt-copy-cohort-parity-20261021.json). No current model, production binary or financial state is changed by this source publication.

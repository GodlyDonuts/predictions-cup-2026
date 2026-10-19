# Exact observation acceleration — October 19, 2026

The measured observation bottleneck is reduced without changing the action domain, simulated timing, visible features, source evidence or accounting. Source input remains the existing inspected development calendar; no independent testing data was opened.

A 10-second nonblocking out-of-process stack sample of trainer18300 collected233 valid stacks (16 missed reads).97 stacks included observation construction and27 ended in tensor conversion. This is sampling evidence, not exact CPU or CUDA time. The profiler was extracted from a standalone PyPI wheel outside the training environment; the active learner was not restarted or modified. [Profiler semantics](https://github.com/benfred/py-spy).

Immutable identity encodings, integer quantity transforms and delivered-book summary statistics are memoized. Ages, invalidation, lots, order quantities, masks and timestamps are recomputed from current state. Cache limits affect reuse only: cache misses compute the complete value. Arrays stored in caches are read-only and observations own their output matrices. Immutable Book identity invalidates book caches.

Single-world and batched observations now stage on CPU, pad all variable entity rows, and use the existing exact-dtype packed transfer to GPU. Float precision and action masks are preserved. This change is not hot-loaded into frozen experiments.

## Measured evidence

- Real source:237 delivered actor books,1,345 entity rows, all438 features. Against checked baseline893ece427f77435bcca052af87704af942c08791, eight alternating timing rounds of100 observations give **2.9433x median CPU observation speedup**: approximately7.7ms to2.6ms per observation.
- Six comparisons after virtual-time advances, book-cache rebuilds, and an eight-world CPU packing comparison preserve exact observations, masks and entity order. Time is frozen within each timing sample; these are engineering observations, not policy trades.
- A small eight-world CUDA transfer probe preserves every value, shape and dtype with212,480 peak allocated bytes. It does not qualify full-size policy capacity or measure end-to-end throughput.
-188 action-suite tests and12 realism tests pass. New regression cases cover cache eviction, read-only identities, age/invalidation/position changes, book replacement, output ownership and variable entity padding. Handoff fixture logs with negative PIDs are test fixtures, not actual process transitions.

See [CPU measurement](feature-cache-benchmark-001.json), [CUDA transfer check](feature-cache-cuda-transfer-001.json), and [reproduction](feature_cache_benchmark.py). Host load and the concurrent learner affect timings. Neither2.94x full training acceleration nor profitable ROI is claimed.

## Actual learning state

At19:24UTC the original015 learner18300 was still training:238,829 decisions and621 accepted updates, with seven of eight cycle0 member results saved. Member006 completed after a long dense evaluation; new optimizer progress resumed. Its result remains partial-history development evidence. The file-backed016 controller36564 still waits for the entire generation and selected offspring; it has not started016 or passed its full-size GPU capacity probe. The new observation acceleration must enter a separately checked frozen source revision before that successor can use it.

Full competition history, the requested multi-day evolution/test windows, complete API/lifecycle parity, official valuation and independently profitable learned performance remain unfinished. Repeated use of evolution data can overfit; final testing must stay isolated and occasional. Production controls and financial state were unchanged by this work.
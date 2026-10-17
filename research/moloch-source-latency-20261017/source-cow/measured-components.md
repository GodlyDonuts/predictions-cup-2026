# R27 measured source and lookup components

The checked candidate is source `4e33725f798ef5b30bb924ac9938e74df39b9d83`, optimized binary SHA256 `259ffd08c56e5a263f8e1bacb45276907a02afa56d60d08e27c2e520045184e4`. Its compiled financial engine identity remains `5b61d3cbb38eadf5170fafae5c91cb5b75858346b2771031084224bfa2c3d6d4`.

These are component measurements with exact output parity, not a measured whole-worker, restoration, order-latency or profit improvement. No SDK tests were rerun for this candidate. Root reported installation at 2026-10-17T13:37:38.521Z with sole PID654706 still restoring; that operational report is separate from the build packet and establishes no fresh-trading result here.

## Original evidence and qualification

The private final ZIP is 10,953,359 bytes, SHA256 `c92a8c66f9cb917886e5c7df0a1733872fa7039dacc4ecdbd15d477531efd0e9`. All303 original member hashes and lengths were checked against its original packet manifest. The root verification reports all263 candidate input files verified:62 exact raw matches and201 CRLF-equivalent matches. Relative to R26, only `moloch/native/src/source.rs`, `moloch/live-runtime/src/fundamental_policy.rs` and `moloch/live-runtime/src/worker.rs` changed. The source adapter is outside the unchanged financial engine identity; this is not a claim that every native source file is unchanged.

Actual ARM64 checks, all with zero failures:

| Target/filter | Passed | Interpretation |
|---|---:|---|
| Runtime library `fundamental_policy` |8|Reused unchanged library qualification |
| Runtime library `order_snapshot` |2|Reused unchanged library qualification |
| Runtime binary worker suite |143|Corrected final worker source |
| `fundamental_policy` integration `top_up` |9|Targeted integration filter |
| Exact target-hint integration filter |1|Already included in the9 above; not additional |
| Source adapter `source::` |6|Separate source-component qualification |
| Differential source harness |14|Separate harness qualification |

The production-candidate test subtotal is162 distinct checks (8+2+143+9), not163. Filtered-out tests are not credited. The optimized final worker build completed successfully at13:34:18.320533Z. Source/harness evidence is earlier, bound to the exact measured source and fixture hashes; it is not represented as a whole native-suite rerun. The never-deployed earlier qualification remains retained privately.

## Measured medians

| Component workload | Old | New | Scope |
|---|---:|---:|---|
| V1 REST book-only update |12.489796ms|0.304359ms|One timed step, two emitted books |
| V2 REST plus WebSocket book update |33.111048ms|7.177581ms|Two timed steps, three emitted books |
| Original acknowledged-ID linear lookup workload |1,059.157462ms|0.247270ms hot|Same ordered original rows;1,573 acknowledged IDs/15,299 orders |
| Same lookup with cold snapshot index |1,059.157462ms|24.542213ms cold|Includes conservative retention-bound check and first-index construction |

The source benchmark uses237 scopes,22,918 unique seeded trade keys, two warmups and20 rounds. Its timed region includes only the declared steps: it excludes base clone/setup, seed/prime, parity hashing, follow-up steps and owner/result destruction. Each case reports exact emitted-result parity and separately checked follow-up parity. The lookup benchmark uses two warmups and seven rounds; cold setup excludes the original input-vector clone. These are different workloads, not additive savings per dispatch.

The late insert-then-conflict path has **no measured speedup**: V1 medians11.657649→11.680577ms; V2 medians11.649184→11.719458ms. The first real mutation still incurs copy-on-write materialization; rollback/conflict behavior is retained. No universal speedup is claimed.

Source allocation counters count allocator-requested bytes, including new requested sizes on reallocations; they are not peak heap or RSS. Host scheduling/cache effects remain. Lookup timing excludes order rendering, financial Session replay, file pins, full liability policy, network and the complete worker.

## Public aggregate boundary

[aggregate.json](build-proof/aggregate.json) retains the numeric results, source/input/harness digests, parity flags and qualification totals. [artifact-pins.json](build-proof/artifact-pins.json) binds selected original logs, source and fixture files without publishing their payloads. Private account rows, original source bodies, binaries and financial fixture contents are excluded. The local documentation check reverified packet pins, successful test-summary counts and the target-filter overlap; it did not rerun Rust benchmarks or production.

No model/source clocks, risk limits, financial ledger, authority or account worker are changed by this documentation. Whole-worker speed, current health and profitability require separate observed evidence.

# Original book-version freshness

Checked October16,17:21UTC. This observer-only repair follows retained official
schema semantics; it does not relax an age limit or demonstrate recovered alpha.

The official `BookVersion` schema distinguishes the last applied engine event
(`sequence`) from the engine clock when the book was read (`at`). It does not
define `at` as last-price-change time. Realtime documentation says pushed books
are read at publication, may be newer than their batch's trades, and compare by
sequence **then timestamp**, including books on duplicate batch revisions.
The retained526,583-byte Oct14 OpenAPI has SHA256
`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`;
the independently retained896,836-byte production schema
`295ed8f5619f624f7fa83cde4b04cd40e4d1f6f5d2c59f568131494450c32496`
agrees. Exact JSON pointers are recorded in [evidence.json](evidence.json).
The public API-reference page could not be read through the web tool; these
definitions are archived primary evidence, not a claim of a newly fetched spec.

| Original evidence | Supported interpretation |
| --- | --- |
| Same sequence, newer genuine `at` | Newer engine read version, even if depth is unchanged |
| Identical sequence/time/depth, later receipt | Duplicate; retain original receipt/first availability |
| Identical sequence/time, conflicting depth | Source conflict; refuse and invalidate pending/current evidence |
| Lower sequence, later time | Older version; do not replace the higher sequence |
| Fresh scoped REST response | Positive target observation, potentially cached; retain all original clocks |
| Heartbeat, another market, silent target | No new target-book read-time witness |

Official [October2 changelog](https://sig.thesuper.market/docs/changelog?page=2)
documents book caching of up to one second and best-effort realtime drops,
including a lost final batch without a subsequent gap. Thus request start alone
is **not** a strict lower bound for engine capture time: a cached read can predate
the request. Any future REST read-witness design must account for caching and
server-clock assumptions. This qualifies the future-design wording in the
earlier [unchanged-book audit](../moloch-trade-trigger-20261016/unchanged-book-coverage.md);
it does not replace `asOf.at` with a client clock. Contiguous revisions are not a
wall-clock completeness heartbeat; original expiry/gap/resync requirements remain.

## Narrow source repair and measured limits

Native `source.rs` already orders books by both fields. The observer previously
discarded all duplicate-revision batches before examining books, then deduplicated
book sequence alone. It now compares original RFC3339 instants at nanosecond
precision, admits a genuinely newer book carried by a duplicate batch without
advancing that batch's coverage/revision, and rejects conflicting equal versions.
Decimal-equivalent representations of unchanged depth remain duplicates. Source,
receipt, observation and trigger clocks, sampling policy, fees, horizon bounds,
trade identities and physical authority are unchanged. There is no SIG client,
native/SDK/worker change, new capture, old-cohort replay or activation here.

The published [777-book probe](../moloch-trade-trigger-20261016/source-age-probe/report.json)
and [779-book successor probe](../moloch-preflight-service-20261016/measurement/age-report.json)
both show **zero** same-sequence/newer-source-time observations and accepted all
observed books. They establish no measured lost sample from this defect. They
also lack a producer append clock, so receipt-to-observer delay cannot be assigned
entirely to worker processing. No claim is made that unchanged books are generally
old, that the repair materially increases samples, or that a market has an edge.

Source commit `063fdd0719cde0613b0b0173ce79e840feac40d0` has LF producer SHA256
`2fa9b142b672f4a9076e185ac91be0b288945b0e1bba6afb0797594ed6678529`
(63,523bytes). **103/103 checks passed**, including nine new original-wire/version
cases; the existing default golden output remains identical. Reproduce from
`moloch/external-data` with the bundled Python:

```text
python -B -m unittest test_crossvenue_observer_input_trace test_crossvenue_observer test_crossvenue_economics test_crossvenue_toxicity test_trade_triggered_observer
```

Only a separately pinned future producer/cohort may use the new normalizer.
Historic input-trace replay must select its exact reviewed sampler version;
allowlisting a new producer hash while running it on old-cohort inputs would
reinterpret those cohorts. Root owns that verifier mapping and prospective
deployment. The useful next measurement is the count of actual original
same-sequence/newer-`at` and duplicate-revision/newer-book cases, joined to their
source/receipt/first-availability ages and independently valid trade triggers.

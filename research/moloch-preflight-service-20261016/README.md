# Cooperative service during physical preflight

The ordinary reconciliation path already processes the existing realtime source
between account reads. This candidate adds the same service before each logical
GET in physical order preflight, including ordinary and reviewed inventory/value
paths. It uses the existing stream, source adapter, bank, recorder and universe.
It adds no account client or requests and changes no financial policy or clocks.

The bounded [original-frame probe](../moloch-trade-trigger-20261016/source-age-probe/README.md)
measured 7.924s median receipt-to-probe delay across 777 books. It cannot split
worker queue/append delay from observer consumption delay. This candidate targets
one known unserviced interval; its actual latency benefit remains unmeasured.

An exact reserved quote is checked after service and before the GET. A changed,
canceling, retired or expired quote defers before physical intent creation.
Unrelated native-head progress and unchanged target-book updates do not veto the
quote. Stream, STOP and durable failures propagate, including failures whose text
resembles the private typed quote-invalidation error. No hook interrupts applying
a returned own-fill receipt or the native prepare/accept boundary.

The seven original typed receipts retain their bytes, timestamps and order.
Pagination, in-flight HTTP, SDK backoff and activation sleeps remain blocking;
this is not a complete asynchronous runtime or an asserted HFT speedup.

## Actual validation and deployment status

The preliminary source compiled on ARM64. The first final suite had 88 passes
and one failing fixture: its hook updated native books without updating the real
source adapter and selected a quote the native planner had not canceled. No
release or deployment followed that failure. A test-only correction uses the
actual broadcast normalizer and genuinely canceled native qid. Root reviewed
[source ordering](source-review.md), reran the complete89-worker group on ARM64,
and verified zero failures/ignored/filtered tests at15:39:51UTC. Four new checks
are a subset of89; unchanged prior groups are not counted again. Optimized
locked/offline release completed15:44:54UTC, binaryfb55f739/12,997,904bytes.
[Actual build evidence](deployment/build-validation.json), [test log](deployment/tests.txt),
[release log](deployment/release-build.txt), [248 source inputs](deployment/build-source-manifest.json).
Only worker.rs differs from the prior checked native/runtime source boundary.

Root froze private operator405be53c to actual artifact pins, independently ran
its actual-freeze check and46 pure guards, and staged manifestdda556e3 while
prior516085 traded. The [draft review](rollout-review.md) remains historical;
[actual final pins](deployment/artifact-pins.json) identify the later frozen operator.

The cooperative stop request15:47:09.896 preceded the census guarded graph236
STOPPED_OR_OWNER_LOST; old worker rewrote STOP/status15:47:10.507/.513. Root
checked original request/status/STOP and source guard, unchanged control/unit,
absent globalSTOP, zero unknown/no active/manualfalse queue, and absent old PID,
then canceled pending autorestart and recorded its independent owned-STOP witness.
Only that owned STOP was archived. [Owner review](deployment/stop-owner-review.json).

One verified start at15:48:53UTC produced sole518852/source2225b1ef/binaryfb55f739.
[Actual rollout summary](deployment/production-rollout.json) pins the preserved
private raw41,734,741-byte receipt separately from its public summary. Original
bank/head/financial prefix and all12 bridge bytes remain; no bridge, bank copy,
financial-history migration or principal recredit. Control changes only the
binary hash. Capital/daily windows/rate settings and ordinarySideEdge=enforce stay.
At [15:49:48](deployment/live-verification.json) matching process/noSTOP was
verified but fresh trading is **pending history restoration**, with no new ACKs.
Original194fills and−143.860000 realized remain; no latency/profit gain claimed.

The [old trade cohort seal](deployment/prior-trade-cohort-seal.json) records four
samples/four labels, eight verified gzip/JSON rows at15:49:48, with original clocks.
No economic outcomes were inspected; missing original consumption/availability
witnesses remain unavailable. Successor observer activation requires a separate
cohort bound to the actual new breadth file. Source/policy remain unchanged.

## Later actual readiness and measurement

Fresh trading was verified16:03:23UTC. At16:16:16 the new worker has14 new
ACKs, healthy realtime/no STOP and successor full status. The original30s probe
observes receipt-to-probe median7.924s→2.069s, with explicit observational limits.
[Actual proof and bounded comparison](measurement/README.md).

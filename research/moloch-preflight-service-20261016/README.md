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
release or deployment followed that failure. A test-only correction now uses
the actual original broadcast normalizer and the genuinely canceled native qid.
The corrected full 89-worker suite passed on ARM64 at15:39:51UTC with zero
failures/ignored/filtered tests; its optimized build is running. These89 are the
complete current worker group, including four new tests; unchanged prior library
and transport checks are not counted again.

The existing production worker remains PID516085/source870ce889/binaryd2ad93b5.
At [15:33:55 UTC](deployment/prior-live-verification.json), it was fresh trading,
healthy realtime/no STOP, with 50 successor ACKs and 194 cumulative fills.
Attributed realized P&L was -143.860000; activity is not profit. Full attributed
capital95,579.915000 and the original bank/twelve bridges remain unchanged.

The [15:35:21 observer snapshot](deployment/prior-observer-verification.json)
has three eligible trade-trigger samples. Its cohort is still active/unsealed;
no economic outcomes have been inspected and no model is qualified.

The [reviewed operator draft](rollout-review.md) permits only a worker source
change and binary-hash-only control update, with all twelve existing bridge
bytes retained. Root independently reproduced its 46 pure synthetic guards.
Final artifact pins remain unset, so it cannot stage, stop or promote anything.

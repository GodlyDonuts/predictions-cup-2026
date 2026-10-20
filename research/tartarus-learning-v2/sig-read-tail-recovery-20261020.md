# Interrupted SIG historical read journal recovered

At 08:26:10 UTC the selected SDK journal was repaired under the original execution-owner OS lock and the SDK's exclusive create-new queue lock. The original execution STOP, network-disable marker, control, selected manifest and checkpoint were pinned throughout.

All 54 completed records verified against the selected checkpoint and hash chain through sequence 345526. The 398,335-byte final fragment exactly matched the beginning of the next admissible READ admission: no write binding, no network start, and unchanged intent/key maps. The last complete state had no active request, no unknown acceptance and no manual-review state.

The complete original 53,989,376-byte journal was compressed, synced, decoded and hash-verified before any mutation. Only the uncommitted suffix was removed. The retained 53,591,041-byte prefix was rehashed afterward and matches the original complete prefix exactly. All 2,452 intent bindings and 2,452 idempotency keys remain in the unchanged completed state. The archive is retained under the incident path in the [actual recovery proof](sig-read-tail-recovery-20261020.json).

The checked helper rejects incomplete writes, network-started reads, corruption, a non-quiet prior state, modified controls and any fragment not byte-identical to the reconstructed read admission. Three focused tests pass. Its first read-only invocation identified an incorrect assumed owner-lock path before mutation; the corrected checked version uses the actual pinned control's `capture.lock`. Deployed helper SHA256: `f1dc4c6f01e825936f4d2087f6541965ffee7a2fe1aea121874ed674e62e1ee1`.

This repairs journal readability; it does not claim the remaining historical acquisition is complete or running. No account requests, financial orders, activation changes, ledger resets or production restart occurred. The 947,549 previously indexed trades remain preserved. Storage growth and a continuation that reuses validated pages must be addressed before resuming the remaining 79 exchange scopes.

GPU023 remains separate on the laptop. A CPU-only isolated initializer study also found a 9 ms median WAIT deadline under the existing prior, with 50.56% below 10 ms. A candidate initialization gave 575 ms and 8.55% respectively. These are 200,000 seeded Beta draws using the policy's `softplus + 1` parameterization, not a full-policy timing comparison or ROI result. No running learner configuration was changed by that study.

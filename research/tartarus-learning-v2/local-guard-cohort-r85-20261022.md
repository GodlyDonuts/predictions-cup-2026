# R85: recover a cohort after a proved local rejection

## Production incident

R83 worker1063142 stopped at 04:59:15 UTC on October22 with `FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT_REQUIRED`. This happened before the planned R84 deployment. Its Delaware1075 NO entry, native20937, had a durable intent and attempt, but the final SDK guard rejected it locally after its physical token expired. The original SDK proves that no HTTP request was sent. It has no venue acknowledgment or fills, and its original native reservation is already retired. [Original incident and SDK verification](r85-stopped-incident-20261022.json).

The bug was an inconsistent pair of conditions: cohort abandonment selected any verified no-network attempt, while its original-row parser recognized only `aborted_before_admission`. The newer `local_final_guard_no_http` record therefore reached a path that could never find its proof, stopping the whole worker.

## Repair

Both original no-network record types now pass through the same archival lifecycle. Local final-guard records retain their distinct SDK verification factory, original request, permit, timestamps, hashes and source archive. They are not relabeled as pre-admission aborts or never-attempted orders. The original attempted ID cannot be resubmitted.

Preparation requires fresh flat-account/open-order/cash facts, the exact committed native retirement and an opaque verified SDK proof. Applying the archival plan changes cohort eligibility without changing cash, holdings, fills or original costs. Restart must verify the original SDK evidence again before another entry can be considered. An altered archive invalidates the plan. The same record handling covers eligible top-up retirement.

The combined candidate includes the previously qualified [R84 source/preflight report optimization](source-projection-r84-20261022.md). Its native library and engine identity are unchanged from R84; only the runtime policy and its regression tests are additional changes. R84 was never started in production. One engine-upgrade approval from original R83 is required, preserving every prior approval and financial record.

## Evidence and current limits

Three new real-SDK regression checks run against the old R84 runtime, whose cohort parser matches R83. Two reproduce the exact production error; the negative-case test passes. The fixture binds a localhost listener and confirms zero HTTP connections. [Before-fix regression](r85-cohort-regression-before-20261022.json).

The fixed runtime passed all149 cohort checks, including the three new local-guard regressions. All244 worker checks also pass. Release binary `5222f0f999faa0e354178a9cdd7d8dc5b0201626875f40f0e5174a11b264e4e6` is qualified and loads its CLI. [Exact compiled source, libraries and checks](r85-local-guard-cohort-qualified-20261022.json). It has not yet replaced production. The STOP remains in place until exact source publication and incident-specific installation. The deployment accepts only the original 04:59:15 failure, original stopped process identity and independently verified no-HTTP evidence. It does not treat an arbitrary STOP as restart permission. Historical reconciled exposure18533 remains subject to normal native-ledger verification at startup.

The final pre-stop SELL736 NO1051 executed at04:51:16.871 and realized3.68 net. The stopped-boundary audit verifies every new official receipt and remaining FIFO. [Final R83 audit](r85-stopped-turnover-20261022.json).

At the stopped boundary, the original ledger has535 fill receipts, cash14,787.975, original remaining cost81,399.705 and attributed realized607.765, including unchanged inherited76.52. The last completed15-minute measurement still contains only one distinct first-filled order. Whole-account profit and the one-filled-order/minute objective are not established. The latest separate official mark remains94,293.565 at the04:39 receipts.

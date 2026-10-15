# Actual collateral continuation: source integration

This is source work for task #143, not evidence that collateral accounting is
enabled in production. The mixed worker defaults `collateralAccounting` to false
and rejects opt-in before acquiring its lease or contacting the account, pending
complete ordinary passive-fill integration and parent verification. Existing
controls therefore do not silently create a new accounting baseline.

The primary schema is the saved official OpenAPI
`moloch/api-spec-ioc-20261014.json`, SHA256
`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`:
`AllExecutionEconomics`, account/collateral, and tournament portfolio settlements.
Sequential receipts provide no documented account-wide snapshot isolation.

## Concrete interface

`collateral_feedback::RetainedReceipt::open` derives an original receipt filename
from the configured worker root and actual Receipt digest. The two supported
stores are `receipts/{digest}.json` and `package-receipts/receipt-{digest}.json`.
The opaque value holds the original descriptor, byte hash and Receipt. The
recorded-digest constructor is for hashes already admitted in native fills or
the original dispatch record, not arbitrary evidence-selected paths.

`Boundary` borrows the existing sole `Transport`, held `ExclusiveLock`, original
ordinary/package dispatch logs, configured root and configured lock path.
Descriptor identities come from the held files. Preparation streams the original
hash chains and compares their exact heads; global unknown/active/manual-review
state or any unresolved physical liability prevents a transition. Never-sent
ordinary reservations are allowed without releasing or changing them.

`prepare(&Session, &Boundary, Operation, processed)` returns an opaque `Transition`.
`Transition::commit(&mut Session, &Boundary)` rechecks the bank/report/head,
queue/log/lease identities and original receipt bytes, then commits the native
event itself. Call prepare and commit synchronously, without an intervening
network request, source event or planning event. A durable-commit failure remains
fatal; the worker never downgrades it to an observation postponement.

The native replay supplies `collateral_snapshot_receipts()`,
`collateral_accounted_fill_count()` and `collateral_settlement_recorded(id)`.
These recover actual admitted evidence after a crash; a mutable sidecar does not
become accounting authority.

## Financial transitions and boundaries

- Baseline requires actual scoped account/collateral receipts, zero existing
  debt and exact seeded opening cash. It never rebases the funded bank.
- ALL execution uses original dispatched POST economics and complete owned fill
  tapes. ACKs never create fills. The original branch requires full orders whose
  execution occurred inside the original POST interval; it cannot represent a
  later ordinary passive fill.
- Ordinary cash sync separately covers terminal ordinary partial/late fills with
  explicit null ALL metadata. Original ACK membership, original complete tapes,
  native fill IDs/quantities/prices/fees/clocks and scoped cash are required.
  Components, debt and available advance remain unchanged. Net agreement cannot
  establish gross borrow/repay history: cumulative gross flows become unknown,
  while separately verified flows remain distinguishable. This branch depends
  on the independently implemented native ordinary-cash-sync validator.
- Settlement uses original complete tournament settlement receipts and explicit
  row IDs. Nullable side/size, corrections, unsupported package-protected
  inventory and inconsistent attribution stay blocked. No synthetic sale or
  historical fill is created.
- Checkpoint advances only observation receipts with no unaccounted fills or
  cash/debt/component/power/available-advance changes. The original interval up
  to 120 seconds preserves its prior report shape. The separate native extension
  admits an unchanged idle interval up to 24 hours only with the exact retained
  before receipts; gross borrowing/repayment during a longer unobserved interval
  becomes unknown. Ordinary cash sync uses the same bounded 24-hour original
  interval. Typed ALL execution/settlement retain their 120-second window.
  Baseline age is never authority to manufacture a fresh causal before snapshot
  for an old execution.

`authority::reconciled_expected_cash` adds the explicit collateral cash adjustment
only when `pendingNativeFillEconomics` is false. Wallet cash remains unlevered
sizing cash; observed future advance does not fund an initial order. Cancellation
retains its independent authority and known-order reconciliation remains live
while new submissions wait.

## Worker staging and checks

The staged hook runs after ordinary/package reconciliation, at most once per
30 seconds. It uses the same Transport/queue for two account/collateral reads and,
when attributed inventory requires it, the bounded paginated settlement read.
The existing read cap, pacing and Retry-After remain in force. Original dispatch
and native fill hashes select the retained ACK/tape files. No second client or
collector exists. Expensive reads do not refresh the older owned-order
reconciliation confirmation clock.

Missing/ambiguous evidence postpones the transition without changing the bank
and defers new submissions. Actual cancellation and known fill reconciliation
continue. Mixed fills and settlement without an original intermediate cash
snapshot are not split into invented transitions. Intervals beyond the admitted
bounds, still-open partial orders, nonnull ALL reports outside their supported
window, ordinary pair redemptions without the required typed ALL evidence,
missing original POSTs and incomplete settlement coverage remain explicit limits.

Runtime fixtures use actual native Sessions, account queues, original dispatch
journals and retained receipts, with a loopback-only Transport and disabled
credentials. They exercise replay, borrowing without Wallet profit, original
fill-tape coverage, unrelated unknown writes, file replacement, same-length
receipt tampering, source symlinks, settlement, reconciled cash, and a terminal
late partial fill. Worker controls test default-off and opt-in refusal. Native
checkpoint tests cover unchanged cash, source recovery and forbidden economic
changes/gaps. Local `git diff --check` passes; Linux compilation/tests and any
promotion are the parent execution owner's responsibility. No test or deployment
success is asserted here before that handoff is verified.

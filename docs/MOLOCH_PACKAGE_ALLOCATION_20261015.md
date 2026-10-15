# Bounded source-bound package allocation

This module ranks **minimum statewise incremental value**, after the existing
scanner buffer and explicit reviewed write/inventory-risk costs. It is not a
calibrated expected-value model or proof of real profitability. No ranking result
creates a native reservation, order intent, SDK permission, fill or cash credit.
The separate own-fill-conditioned execution model remains empirical and currently
withholds confidence without adequate actual coverage.

## APIs and exact production sequence

`Budget::from_receipt(bank, activation, &original_cash_receipt, remaining_writes,
now)` requires an original scoped `/account?tournamentId=...` HTTP200 receipt,
ordered clocks and a request started within20seconds. It uses the smaller of
actual gross official cash and native wallet cash, less **all current actual
reservations**, and maximum committed less owned cost/reservations. Available
advance, expected SELL proceeds and an assumed release of unsent quotes add no
funding. It pins the actual durable bank head. The sole worker supplies remaining
writes from the existing SDK account queue; this budget does not reserve a queue
slot or supersede its rolling cap/backoff/uncertainty guards.

`ReviewedCosts::verify(input, trusted_review_hashes, now)` requires an explicit
root/operator-reviewed immutable policy digest, exact certificate digest,
nonnegative write/risk costs and an unexpired source reference. Zero costs are
accepted only through that explicit reviewed policy; zero is not inferred as a
venue fact. Actual entry fees must independently be known zero in the existing
bank/scanner policy. The digest binds policy bytes; it does not authenticate a
settlement source, prove a holding duration, or grant execution authority.

1. Construct verified `CertifiedComponent` objects using the existing canonical
   registry plus independent settlement certificates and original rule receipts.
   No title-derived component or new HTTP client is introduced.
2. Build at most32 `ReadHint::scan(...)` results from existing current native
   books. Hints preserve the bank head, certificate/source identity, full worst
   wire-limit funding and positive **additional-instruments statewise floor less
   full cost/buffer/reviewed costs**. They may coexist with native never-attempted
   quotes. Their reservations remain real and reduce available funding. Opposite
   held outcomes that would auto-net are excluded. Hints are read scheduling,
   not opaque funded proposals, and cannot enter stage/send.
3. `select_hints(...)` chooses at most one current hint. The worker may refresh
   only that bounded source cohort through its existing Transport. Keep urgent
   cancellation/reconciliation and covered inventory exits ahead of this work.
4. Recheck actual physical liabilities. Retire only the **chosen** component's
   dispatcher-proven never-physical native quotes through the existing path.
   Do not retire unselected components, cancel actual orders to make room, clone
   a Session or count predicted freed reservations as available cash.
5. The retirement changes the durable head. Discard the old hint/budget. Rebuild
   the original scoped cash budget if its actual source clocks remain fresh,
   otherwise obtain it through the same existing worker. Recompute the chosen
   actual opaque `Admission` or portfolio `Plan` from the real current Session.
6. `Eligible::admission` / `Eligible::portfolio` borrow the genuine opaque object.
   They reject changed head, mismatched bank/run/certificate, changed current
   books/holdings, missing gross funding or nonpositive statewise delta. A legacy
   admission must still be flat with no active ordinary component quotes and
   exact current raw book hashes. A portfolio Plan must pass its own current-head
   lease. The floor-hedge counterexample is rejected despite positive
   `floor(after)-floor(before)` or liquidation-relative utility.
7. `select_plans(...)` returns at most one borrowed proposal plus diagnostics.
   Drop the borrowed selection before moving the owned proposal into the existing
   package stage. All original activation waits, durable intent/attempt/source
   evidence, fresh physical preflight and ProductionAuthority/SDK checks remain
   mandatory. A changed bank invalidates the budget/selection and requires
   recomputation; it is never a retry or renewal of a physical package.

At present the production loop picks a certificate round-robin before recomputing
an opportunity. Root owns its wiring and immutable settings. These APIs enable a
bounded source-only hint pass followed by selection of a real recomputed proposal;
this source module alone does not claim the production ranking has been wired.
Missing canonical/independent evidence produces no candidate, never a synthesized
relationship or an inferred positive floor.

## Objectives and scheduling limits

Unknown holding duration uses **CapitalEfficiencyOnly**: net universal gain divided
by full additional gross capital. It makes no capital-time, annualized return or
rank-optimality claim. A settlement date is not treated as cash-release finality.
A reviewed horizon is a source-bound operator ranking assumption, not a guaranteed
release deadline. Time ratios are compared only when **every eligible funded
candidate in the current tier has the identical reviewed comparable-group ID**.
Mixed groups or missing horizons use capital efficiency for the entire tier,
avoiding a non-transitive mixture of time and non-time comparisons.

Covered whole-unwind Plans have a separate first-priority tier, requiring positive
`min_state(cash_received - payout_surrendered)` after buffer/reviewed costs. Their
full BUY funding is zero. No denominator of one unit is invented, and full SELL
proceeds are never labeled profit: only the universal incremental difference is
ranked. Selecting an unwind does not credit its cash before official fills.

All multiplication is checked integer arithmetic. At most32 input proposals are
accepted, one physical selection per pass; overlapping reviewed risk groups cannot
be simultaneously allocated. Equal scores rotate by stable certificate+mode key.
A bounded eligible-wait counter eventually serves still-positive budget-fitting
candidates. Fairness never bypasses stale heads, expired sources, missing costs,
nonpositive value, missing writes or funding/capital limits. Continuous covered
exits can defer entries: safety/management priority is explicit, not global
rank-optimality. Fairness memory is cold on restart and is not a durable execution
or retry journal.

## Verification and limits

Focused tests use real native Sessions, original scoped cash Receipts, existing
independently reviewed certificate fixtures, actual opaque Admission/Plan objects,
completed actual-fill/terminal package histories and current full-depth books.
They check worst-limit versus average-depth funding, negative statewise hedge
rejection, changed head, reviewed costs/horizon groups, bounded fairness/write
budget, nullable/stale/wrong-scope cash, actual reservations and zero-funding
covered unwind economics. Source is handed to root for combined ARM64 checks;
no account/VPS requests, credentials, production changes or local test success are
claimed by this source-only subtask.

## Immutable allocator configuration

Root may pin `Option<package_allocation::Settings>` as `packageAllocation` in the
existing immutable Control. Fields are explicit `reviews: Vec<ReviewInput>`,
`trustedReviewSha256: Set<String>` and `maximumWait` in1..32. At most32 reviews are
accepted; certificate bindings must be unique. Every configured review's exact
canonical digest must appear in the trusted set, and that set must equal the
configured digests (unused approvals are rejected). Every row's cost/source and
horizon/group shape is validated, including rows not selected this pass.

Call `settings.validate()` for configuration diagnostics before using it.
`settings.costs_for(certificate_sha256, now) -> Option<ReviewedCosts>` returns None
for missing/expired reviews or any invalid overall settings. Invalid rows never
partially approve another row. Past expiry is a lookup withholding condition;
configuration validation does not extend or renew it. No discovery result adds
a trusted hash, no absent review supplies zero costs, and missing required costs
or settings fields fail serde parsing. Optional unknown horizons remain None.
An empty review vector plus empty trusted set is valid and selects no candidate.
Root supplies these approvals from reviewed immutable configuration, rather than
calling `digest` to approve an arbitrary dynamically observed proposal.

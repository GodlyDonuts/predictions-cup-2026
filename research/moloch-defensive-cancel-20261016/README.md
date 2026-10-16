# Defensive ordinary BUY cancellation: source feasibility

**Feasible as a bounded runtime-only extension of the existing known-order CANCEL
path; no native financial-engine change is required.** Preserve the exact current
side proxy, entry-fee ceiling and native minimum threshold. This would remove a
remaining resting commitment when fresh evidence no longer satisfies that existing
criterion; it does not establish calibrated toxicity or profitable avoided fills.

This review inspected source only, including primary checkout
`6607dc7ee957fbca49ff41871d07cb8834ef7004`. No current outcome/cohort, account,
SSH, live service, control or source policy was read or changed. The execution owner
decides implementation and checked activation. This file supplies no authority.

## The working route already exists

[ProductionAuthority::prepare_cancel](../../moloch/live-runtime/src/authority.rs#L91)
requires an actual live bank/run, genuine native→physical order binding, nonretired
qid, exact bodyless `DELETE /orders/{actual}`, original run and `{qid}-cancel`
permit. It does **not** require a native `cancelEffectiveAt`. Physical checking
retains control/binary/STOP/owner/request guards and four fresh complete account
receipts. Cancellation needs neither a live entry window nor a streaming gate;
this existing removal authority must not be narrowed by the new optional signal.

The [worker cancellation block](../../moloch/live-runtime/src/worker.rs#L1244)
currently selects native cancellations, expiration handling and reviewed-policy
cancellations. It writes durable `cancel_attempt` before the existing
[Transport::cancel](../../backend-rust/transport-service/src/lib.rs#L494), records
the actual receipt/`cancel_ack`, then returns. SDK admission uses the same controller,
stable cancellation binding/key and shared100-read/24-write budgets. No second
client, new rate budget, synthetic native cancellation or added financial event is
needed to nominate an additional qualifying qid.

The selection helper cannot simply call
[ordinary_side_edge::assess_original](../../moloch/live-runtime/src/ordinary_side_edge.rs#L71)
with an ACKed quote. That entry-only function deliberately requires `filled==0`,
nonretired state, no native pending cancel and original final REST preflight scope.
Cloning a partial quote and setting `filled=0`, or wrapping a native book as a fake
REST receipt, would be invalid. Add a separate resting assessment with shared
arithmetic and explicit genuine remaining-quantity/ownership evidence.

## Smallest sound selection contract

1. Start with original durable ordinary BUY intent/attempt/ACK, exact bank actual
   binding and a currently nonretired native qid with positive
   `quantity-filled`. Require no original dispatch `closed`, no confirmed cancel
   ACK and no unresolved earlier cancellation outcome. Never invent an actual ID
   for an unacknowledged/unknown new order. Full-filled/retired orders need their
   existing terminal reconciliation, not another cancel.
2. Exclude SELL, every package qid/protected package component, reviewed native
   metadata, original `intent.fundamental`/package tags and current managed scopes.
   Original attribution must survive review expiry/restart; a missing current
   manager alone cannot turn an old reviewed order into ordinary speculation.
3. Preserve the fully covered opposite-BUY exemption using actual opposite FIFO
   inventory after other outstanding BUY/SELL encumbrances. For the narrow initial
   **unhedged-only** version, also defer mixed/partial opposite coverage to existing
   inventory management: deleting a whole mixed order is not cancelling only its
   uncovered slice. Do not infer hedge coverage from side labels, an absent open
   list member or a retired-but-physically-unclosed quote.
4. Read the current valid native book through the existing
   `Engine::live_package_books()` readonly getter or typed
   [native_books adapter](../../moloch/live-runtime/src/package_admission.rs#L21).
   The historical accessor name grants no package authority. Use exact scoped
   source/receipt/request clocks, current native policy age, genuine sequence/hash
   and bank head. An invalidated/gapped/stale/future/missing book cannot trigger
   defensive cancellation. Do not fetch all237 markets or create a book receipt.
5. Require genuinely post-ACK source and receipt clocks, not merely a newly run
   classifier on the old submission snapshot. Pin qid/actual ID, current quote and
   remaining quantity, book identity, original policy hash and dispatch/native
   heads. Baseline source/receipt-after-original-POST-ACK proof is available from
   the original ACK receipt even after restart; the latest diagnostic file is
   not an immutable per-order baseline. Repeated observations of identical book
   evidence cannot mint additional cancellation attempts.
6. Reuse the same YES-floor/NO-complement microprice arithmetic and original
   `fee(limit,1)`/`minimumEdge`; nominate **only an authenticated BelowMinimum**.
   Unavailable/NotApplicable is not BelowMinimum, so it must not become a mass
   cancel on a feed outage. Preserve original one-share proxy meaning: it is not
   full-depth liquidation value, a fill probability or expected after-cost gain.

A minimal new pure `defensive_cancel.rs` could return an opaque nomination bound
to those originals, with `validate_current(bank,log,scope,managed,now)` and a validity
deadline derived from original book/quote clocks. `ordinary_side_edge.rs` would
share its exact arithmetic/validation core while retaining the entry-only API.
Root-owned `worker.rs` would consume at most one nomination through its existing
cancel path. A separately reviewed optional activation should preserve default
compatibility; this report does not change the current setting's documented meaning.

## Required lifecycle, freshness and scheduling guards

Use the current four genuine global receipts before sending. During/after those
reads re-read qid, binding, partial-fill totals, ownership, hedge/package/manager
classification and signal clocks. A newly consumed source can invalidate or reverse
the signal. Bound the physical permit by the original signal lifetime as well as
the existing cancellation permit; SDK queue waiting must not renew stale evidence.
An ordinary signal expiry is a deferred optional action, not a fabricated send or
a reason to stop SELL/reconciliation. Fundamental IO/control/durable failures retain
their existing failure treatment.

Known-order cancel is not permission to clear account uncertainty. The same live
controller must be nonpoisoned/quiet with no global unknown acceptance or active
write, and original new-order/cancellation attempts must be resolved. Do not infer
retry permission from a quiet snapshot or a missing response: a prior durable
`cancel_attempt` without a confirmed outcome requires original SDK/known-order
reconciliation. `DispatchLog.cancel_acknowledged` suppresses confirmed repeats but
is not by itself an index of unresolved cancel attempts. If that recovery boundary
cannot be demonstrated with existing originals, add a narrow durable lookup/index
rather than sending a replacement cancellation or inventing a fresh key.

A successful DELETE/`cancel_ack` **does not** retire the native quote, release
reserved cash, label zero fills or claim cancellation before a racing fill. The
existing [pending reconciliation](../../moloch/live-runtime/src/reconciliation.rs#L32)
keeps every ACKed-not-closed order, marks cancel pending, obtains exact order plus
complete subsequent fill evidence and applies actual fills before terminal closure.
Only `actual_order_terminal` with complete matching totals releases the remainder.
Existing definitive400/404/409 cancel rejections use exact GET/full-fill recovery;
uncertain outcomes preserve durable queue uncertainty and fail closed. No blind
write retry is added.

Keep current mandatory cancellations/reconciliation and urgent covered SELL/hedge
management ahead of this optional defense. Do not put an unbounded defense scan
ahead of all owned exits or buy four new receipts for every below-threshold book.
One optional known-order cancel per dispatch pass, using the existing write budget
and fair cursor, is sufficient initially. It replaces a new speculative submission
for that pass. A higher cancel rate is not evidence of better trading.

## Evidence required before claiming benefit

Meaningful actual-Session/SDK tests must cover both sides/rounding/threshold equality,
unavailable/gap/future source, actual ACK bindings, partial fills during four reads,
full fill before send, opposite hedge/partial coverage, package/review protections,
same-key restart/unknown cancel, genuine cancel/fill race, and reservation release
only after full terminal proof. Ordinary threshold/price/quantity and the native
engine must remain unchanged; no mass-cancel test is a profitability test.

Prospectively retain original nomination/book/quote/policy/source clock, all actual
cancel attempt/ACK/terminal/fill times, remaining exposure and actual exit outcomes.
Report cancel latency, racing fills, missing signals, opportunity/queue loss and
capital-time alongside original maker episode markouts. Current raw book depth can
contain our own order, so the proxy can be self-influenced; silently switching to
own-volume-adjusted microprice would be a different declared criterion. Cancelled
orders cannot be credited with hypothetical avoided fills or profit. Sparse prior
negative markouts motivate the test but cannot qualify this defense economically.

# Same-binary redemption and the remaining accounting boundary

Task [#132](https://github.com/GodlyDonuts/predictions-cup-2026/issues/132), Viserion/same-binary-redemption-20261015. This is an SDK scope correction, prospective within-tape fill normalization and a read-only native accounting audit. It neither activates baskets nor changes native accounting. The parent execution task remains [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120).

## Primary evidence

The saved official SIG OpenAPI document `runtime/moloch/api-spec-ioc-20261014.json` has SHA256 `6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`. It is an ignored source receipt, not a committed account response. These are exact JSON pointers into that document:

| Pointer | Documented behavior |
| --- | --- |
| `/paths/~1orders/post/description` | Engine-routed opposite BUYs reduce/cross the position at FIFO lot cost and credit one currency unit per netted share. Fully backed SELLs close conventionally; other SELLs can be canonicalized into complement BUYs. |
| `/paths/~1orders~1multi-leg/post/description` | Duplicate exchange/scope legs are rejected when `relationshipConstraint` is supplied. Without that constraint, duplicates are explicitly not rejected for that reason. `allOrNone` fills every leg completely within its limit in one atomic engine execution, or nothing executes/rests. |
| `/components/schemas/AllExecutionEconomics/properties/redemptionCredit/description` | Complementary-share cash credit is reported separately from notional and collateral. The containing object can be null. |
| `/paths/~1portfolio~1collateral/get` | ALL collateral concerns eligible relationship components. Zero availability has several possible causes; it does not disprove intrinsic binary redemption. |
| `/paths/~1account/get/description` | `balance` is scoped cash; `buyingPower = max(balance,0) + availableAdvance`. These quantities are distinct. |
| `/paths/~1orders~1{id}~1fills/get/description` | Each fill page is **newest first**; lifecycle totals cover the whole order. |
| `/components/schemas/Fill` | Price is the traded outcome's price; NO quantities are negative. Realtime prices use a different YES denomination. Fill ID is a trade identifier shared by counterparties; engine/legacy identifiers are separate spaces. |

The October 15 empty canonical relationship/constraint/component census establishes an available but empty registry at that observation. It does not establish that complementary buys cannot net, that ALL is disabled for every future portfolio, or that an unobserved basket would be accepted.

## SDK change and its limits

The strict two/three-leg buy-only adapter now permits one YES and one NO leg on the same scoped exchange **without** `relationshipConstraint`. It still rejects repeated exchange/side BUY legs as a conservative adapter rule, requires distinct exchanges for constrained packages, and retains exact tournament, tick, quantity, idempotency, gross cost and aggregate permit bounds. It does not subtract a prospective redemption from funding. ACK parsing still requires separate physical order IDs, every leg fully filled, durable original responses, and independent complete fill reconciliation. ProductionAuthority still rejects basket dispatch.

Repeated same-exchange support is not evidence of an opportunity. In an ordinary uncrossed binary book, buying both outcomes immediately costs `askYES + (1-bidYES) >= 1` before costs. Opposite BUY to close a held YES position is economically a covered YES sale at the complementary price. It is a useful execution/accounting primitive, not a newly demonstrated edge. Passive legs filled at different times have inventory risk; atomic immediate execution does not supply missing spread profit.

## Native audit: redemption already exists

`Wallet::buy_fill_exact` in `moloch/native/src/lib.rs` debits exact fill notional and fee, appends the purchased lot, consumes `min(YES,NO)` FIFO shares from both sides, credits one unit per pair, and recognizes redemption less both lot principals. Its cloned state is audited before replacing the wallet. `engine_live.rs::apply_live` uses this path, and a BUY calls `consume_inherited` on the opposite side at effective close proceeds `1-fillPrice`. Inherited gross remains separate; all execution fees remain in new trading realized under the existing attribution convention. There is no missing basic redemption implementation to patch.

For a canonical buy fill of quantity `q`, own-outcome price `p`, explicit fee `f`, and opposite holdings `h` before execution, let `k=min(q,h)`, and `B` be the exact FIFO principal of the `k` opposite shares consumed. Without ALL advances/repayments:

- cash change = `-q*p - f + k`;
- surviving new-side quantity = `q-k`, with per-share principal `p`;
- realized change = `k*(1-p) - B - f`;
- previously owned opposite quantity falls by `k`; a crossing remainder keeps its own acquisition clock.

For inherited shares, the inherited gross portion is `kInherited*(1-p) - inheritedFIFOPrincipal`. Opening attributed principal remains constant. New trading realized is total realized less inherited gross; never import old account profit. A fully backed SELL uses `q*p - consumedFIFOPrincipal - f`, without a redemption credit. Never credit both the derived one-unit redemption and `all.redemptionCredit` a second time. ALL advance/savings/repayment cannot be treated as profit; supporting positive ALL liability requires a separately checked liability/cash adapter. Nullable ALL does not mean zero netting.

## Concrete FIFO hazard: receipt ordering

`moloch/live-runtime/src/feedback.rs::fills` validates complete owned tapes but emits the official rows in their supplied order. The schema says that order is newest first. The native wallet appends lots in event-admission order. This can reverse venue FIFO even when all identities, sums and accounting identities validate.

Counterexample with one exchange and zero fees: a YES order fills one share at .20, then one at .40. A newest-first page produces wallet lots `[.40,.20]`. A later one-share NO BUY at .50 redeems the .40 lot: native realized +.10, remaining YES basis .20. Chronological venue FIFO instead redeems .20: realized +.30, remaining basis .40. Cash and total capital identities agree in both cases, so those checks alone miss the attribution error.

The observed three new-entry production clusters had one price each (385 YES shares total). Their equal prices avoid this particular basis difference; that is not proof that a future varied-price tape is safe. Receipt ordering can still affect acquisition ages. No opposite production fill or current realized misstatement is claimed by this audit.

The parent authorized a narrow prospective correction: after validating a complete tape, `feedback::fills` stably sorts events oldest first using the parsed execution timestamp at nanosecond precision, preserving the original receipt, IDs and digest. Equal timestamps at different prices fail with `ACCOUNT_FILL_ORDER_AMBIGUOUS`; no unverified trade-ID tie ordering is invented. Equal-price ties are economically equivalent for this same-side zero-fee tape. A regression uses the two-price example and the existing native wallet to assert exact .30 realized/.40 remaining basis after redemption. Another regression explicitly demonstrates that two independently normalized tapes can still arrive in reverse execution order.

Sorting each complete tape oldest first does not establish global execution order across independently reconciled orders. A delayed earlier fill from another owned order can still arrive after a later opposing fill. Fill identifiers also need an explicit engine/legacy namespace; numeric IDs alone are not a universal ordering or globally unique key. No previously admitted event is reordered by this adapter change.

## Can the portfolio fill feed supply a future barrier?

The same primary schema's `/paths/~1portfolio~1fills/get` and `/paths/~1tournaments~1{slug}~1portfolio~1fills/get` expose newest-first scoped fills across all orders. Parameters are exchange/market filters, limit (maximum 200), opaque cursor and explicit tournament context; there is no documented `from` or `throughSequence` filter. Responses include `coverage.complete=true` because cursor pagination exposes complete history, and nullable `coverage.projectedThroughSequence`, the latest engine sequence reflected by the reporting projection. **A page with complete coverage can still have `pagination.hasMore=true`.** These are useful ingredients, but no snapshot-bound cursor or fixed common checkpoint across moving pages is documented here.

A future shared-worker design can collect an overlapping newest-first suffix once per cycle through the existing queue, require a non-null engine checkpoint and a contiguous retained prefix ending at a proven prior anchor, and stage owned fills before any economic admission. This needs explicit namespace/order semantics and a verified way to bind every gathered row and participating order to that common barrier; the schema alone does not prove that a changing checkpoint across pages is a snapshot. Do not declare coverage from the first page, stop at an arbitrary timestamp, or confuse the reporting checkpoint with a fill ID. Null `orderId`, unknown owned bindings, conflicting duplicates, missing pages/anchors, or an older unseen fill on an exchange after admission must block new opposing/package execution and request reconciliation. They must not silently rewrite lots or clear unknown acceptance. Individual known-order GET plus its complete later tape remains the terminal liability proof.

## Required evidence and proposed next patch scope

Before native basket authority or opposite-order execution is enabled, bind exact tournament/account context, original durable package/intent, physical order IDs, canonical side/action/quantity/limits, and original private receipt hashes. Every fill needs its scoped order identity, source namespace/engine epoch, trade ID, positive absolute quantity, own-outcome price, execution clock at original precision, receipt/processing clocks, explicit verified fee convention and complete lifecycle coverage/totals. Reconcile all relevant owned-order tapes at an authoritative common execution barrier before deciding FIFO. ACK notional, position marks, `moneyEarned`, and nullable ALL are insufficient substitutes for that tape.

Proposed follow-up, requiring parent approval before edits:

1. Implemented candidate under the parent's explicit scope extension: `feedback.rs` and focused tests normalize new complete tapes oldest first and reject unequal-price timestamp ties, retaining original receipts. This is not a global FIFO solution or a namespace migration.
2. A new owned-fill staging/reconciliation helper and tests: gather all participating order tapes, deduplicate composite source/order/trade identity, prove coverage through a common barrier, and only then emit new canonical ordered events. Existing known/unknown acceptance and single-worker admission remain authoritative. A package must not trade before this integration is checked.
3. Native regression tests, initially without behavior changes: exact and crossing complementary redemption; varied-price FIFO; out-of-order receipts across two orders; positive fees charged once; inherited plus new-lot attribution; duplicate replay/conflicting IDs; all-or-none zero execution; final cash/positions/realized/basis parity with independent venue receipts. Check explicit acquisition clocks on surviving lots.
4. Preserve the historical live journal prefix, manifest, receipts, attribution and sealed report. Do not sort or rewrite admitted past events or silently change their replay semantics. First run an offline discrepancy audit; any required repair needs a separately authorized source-bound successor/migration with old/new reconciliation. The current SDK fix alone requires no ledger migration.

No native/worker changes, account requests, production tests or orders were performed for this task. Rust verification of the candidate is handed to the parent Linux toolchain; local Windows has no `cargo` command.

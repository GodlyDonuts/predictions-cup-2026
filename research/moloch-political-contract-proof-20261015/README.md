# Political contract proof after the empty ALL census

Task [#123](https://github.com/GodlyDonuts/predictions-cup-2026/issues/123), October 15, 2026. Research only: no account requests, production changes or trade authority. Parent observed HTTP200 empty relationships, constraints and collateral components in the sole production worker at09:05:13Z. That finding is parent-reported here, not independently re-read by this task.

## What the source evidence actually supports

The October5 Michigan review preserves **different canonical contract leaves for one exact contest**, rather than title matching: markets305/306, exchanges994/995, root contract IDs306/307; race76735, stage118938, MI, General, State office, election2026-11-03, resolutionType`Party Winner`, winnerName Democratic/Republican Party. Its review expired October5. The original raw HTTP receipts are absent on this PC; the committed derived review is historical evidence only.

The saved official OpenAPI (`runtime/moloch/api-spec-ioc-20261014.json`, SHA256`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`) documents `/markets/{id}/nodes` as the resolution tree. `/components/schemas/MarketTreeContractNode/properties/contract_details` is an opaque stored object whose shape varies by contract type. It does not promise a typed election winner ID, the affiliation/fusion resolver, per-contract rule version, refund coupling, or a cross-market exclusion guarantee. The market schema supplies tournament contexts, lifecycle and effective settled outcome; a scheduled date is not settlement finality.

The [Michigan Department of State's official candidate listing](https://mi-boe.entellitrak.com/etk-mi-boe-prod/page.request.do?electionType=GEN&electionYear=2026&page=page.miboePublicReport), read October15, identifies one Governor/Lt.Governor position and separate Democratic and Republican tickets, plus Libertarian, U.S.Taxpayers and Green alternatives. This supports ordinary candidate exclusivity and **rejects D/R exhaustiveness**. It does not define SIG's interpretation of party affiliation.

The historical saved [Alaska predicate review](../crossvenue_screen_2026-10-02/alaska_complement_check_2026-10-02.json) expressly records fusion/affiliation overlap: one winner can make multiple party contracts YES. It is Alaska evidence, not a current Michigan rule snapshot. Therefore neither a common race ID nor different party strings alone certifies that both Michigan YES predicates cannot resolve true.

The freshly read [official settlement guide](https://sig.thesuper.market/docs/settlement-and-payouts) describes ordinary1/0 payouts, administrative overrides and cancellation/N/A refunds of remaining refundable cost. It does not guarantee coupled refunds for two separate markets. These exceptions must appear in the payoff model.

## Measured economics and the dangerous missing state

The reproducible historical scan covers October14 19:51:04.837–20:00:51.485Z:117 title-discovered pairs,1,550 cofresh observations across61 races. These are correlated book observations, not1,550 independent trades. No positive immediate roundtrip was established. Historical best MN-05 limits0.080+0.875 cost0.955; Michigan limits0.105+0.885 cost0.990. None is a current quote.

For one Michigan dual-NO package at those historical costs:

| Settlement scenario | Total payout/refund | Return after0.990 cost |
|---|---:|---:|
| Only D or only R predicate true |1.000 |+0.010 |
| Neither predicate true |2.000 |+1.010 |
| Both predicates true |0.000 |−0.990 |
| D-market refunded, R-NO loses |0.105 |−0.885 |
| R-market refunded, D-NO loses |0.885 |−0.105 |
| Both refunded at new purchase basis |0.990 |0.000 |

Refund examples assume new fully remaining lots with known refundable basis. Never copy these amounts onto inherited unknown-basis lots. Even eliminating fusion overlap leaves asymmetric-void losses; a blanket1.000 all-state floor is false.

Under the **assumption** every ordinary package pays1 and an exceptional package pays0, break-even exceptional probability is1−cost:1% for0.990 or4.5% for0.955, before fees/operational costs. This is sensitivity arithmetic, **not an estimated exception probability or a positive-EV certificate**. `results.json` preserves exact millionth calculations and source hashes.

## Specific adapter evidence required next

First inspect the sole worker's existing private source cache for `GET /markets/305?tournamentId=…`, `/markets/306?...`, `/markets/305/nodes?...` and `/markets/306/nodes?...`. Use source receipt clocks/hashes and exact tournament ID`bda92870-621e-47b0-bc3c-3602c5c26f55`; do not create another account client. For the larger historical candidate, inspect338/339 and their nodes before spending reads. Their apparent discount deserves priority over Michigan's1% conditional margin, but is not proof.

An independent ordinary-resolution certificate would need all of:

1. Fresh exact market→exchange→contract mapping, simple unsettled roots, equal canonical race/stage/office/date/jurisdiction and distinct outcome predicates.
2. Source-bound actual contract resolution text/version, designated result source and finality standard; candidate-to-party mapping including every permitted fusion, affiliation, write-in, replacement and exceptional state. A Boolean exclusion statement must be justified by these sources, not an operator's `verified:true` flag.
3. Separate source-backed N/A/refund vectors for each leg, actual refundable basis, and explicit administrative-rule risk. If independent voids remain possible, record the lower floor/loss instead of discarding those states.
4. Fresh executable depth, full funded reservation, account eligibility, inherited exposure, exact fees and economic-return requirement. An ordinary conditional basket may be assessed as a risk-bearing investment only with independently supported probabilities; it cannot enter the guaranteed-arbitrage lane.

The supported market/node schemas provide item1 but not all of items2–3. No current certificate was fabricated. A public UI Info snapshot may supply missing exact resolution text, but an unsupported internal website API is not an acceptable substitute for the official participant interface.

## Venue atomicity is available even with no ALL registry

The saved official `/orders/multi-leg` schema makes `relationshipConstraint` **optional**. `executionMode:"allOrNone"` requires every leg to fill its full quantity immediately within its limit, atomically; failure returns `AllOrNoneNotFilled` and neither executes nor rests. This can remove legging risk without requiring a canonical ALL UUID. It does not remove settlement/refund risk or establish an edge.

The default multi-leg mode only gives transactional order admission and may partly fill/rest. Never call it an atomic full-fill basket. Preserve one package idempotency key, ordered exact legs, explicit tournament and execution mode across retries; ambiguous responses require reconciliation. The execution owner is implementing this separately; this research submits nothing.

The cached237-market catalog at October14 22:56:31Z has no `isComposite` or `isMultiOutcome` markets, so immediate local AND/OR-tree opportunities were not found. That dated catalog does not prove future absence.

## Reproduction

Run `node --test research/moloch-political-contract-proof-20261015/audit.test.mjs` (5/5 pass), then `node research/moloch-political-contract-proof-20261015/audit.mjs NEW_OUTPUT.json`. The audit makes no network calls and refuses overwriting output. Tests reject title-like identity mismatches and duplicate party predicates and verify asymmetric refunds and overlap stress. Passing these tests establishes the arithmetic and source-field checks, not actual profitability.

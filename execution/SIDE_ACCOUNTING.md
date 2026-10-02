# Side-aware fill projection

`side-accounting.mjs` is a pure prerequisite for future NO/package reconciliation.
It is separate from the frozen YES execution core and has no dispatch, permit,
ownership, position adoption or account reconciliation authority.

The checked-in official `api-guide.txt`, realtime section, specifies YES-denominated
prices in account-batch fills: a NO fill at YES price 0.84 has effective cost 0.16.
`openapi.json` specifies outcome-signed quantities in REST order/portfolio fills
and side-relative canonical order limits. The REST NO fill price convention is
not specified there. This module rejects REST NO projection rather than extending
the realtime convention without evidence. Fresh official clarification remains
necessary before REST NO execution reconciliation can be implemented.

Inputs are explicitly scoped, normalized fill evidence, not raw batch payloads.
An upstream adapter must supply verified order/exchange/market/tournament bindings
and signed quantities; realtime signing must not be guessed from the REST schema.
Only already canonical BUY bindings are supported. SELL canonicalization, netting,
position lots, placement acknowledgement and aggregate reservations are separate
requirements and are not implemented by this projection.

Decimal complements, principal cost and remaining hard-limit reservations use
integer micros. Identical repeated fill deliveries deduplicate; conflicting IDs,
changed/missing history, scope/sign/time errors, overfills, exceeded limits and
mutated prior projections reject. A complete page-history flag cannot regress,
but is caller evidence, not proof of complete exchange reporting. Projection
hashes detect accidental changes; they do not confer execution authority.

Run `node --test execution/side-accounting.test.mjs execution/basket-risk.test.mjs execution/core.test.mjs execution/transport.test.mjs`.
These tests establish accounting behavior, not strategy profitability.

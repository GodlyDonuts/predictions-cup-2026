# Capital mechanics checked October22

Sources checked directly: [official FAQ](https://sig.thesuper.market/docs/faq) and the embedded OpenAPI document in the [official API reference](https://sig.thesuper.market/api/v1/docs). These are venue facts, not proof that every operation below is integrated into Moloch's active strategy.

The FAQ requires cash accounts and prohibits margin and leverage. Position and order affordability depend on available SUSQies; balances cannot become negative. There is no verified borrowed buying power to activate.

The current `POST /orders` description provides these useful mechanics:

- Buying the opposite outcome against an existing position nets shares and credits1 per netted share. Cash movement can therefore differ from the order's `totalCost`. Accounting must reconcile actual fills, original FIFO and actual account cash rather than treating the gross purchase field alone as cash movement.
- An uncovered, opposite-side or oversized SELL is canonicalized into the complement BUY. It is cash-funded exposure, not borrowed short selling. A fully backed SELL closes conventionally.
- Resting quotes can exist on both sides. Self-crossing remainder is cancelled after other eligible counterparties are considered.
- Resting orders do not escrow cash. Across exchanges their nominal commitments may exceed cash, but actual buys remain cash-funded and unaffordable quotes can shrink or cancel. This does not multiply funded capital.

For a genuinely complementary same-exchange position, buying one YES for0.43 and one NO for0.54 and successfully netting them produces0.03 before fees. This arithmetic is not an observed current opportunity, simultaneous-fill guarantee or forecast of returns. Separate party contracts cannot be substituted without resolving their different settlement and void conditions.

The currently served OpenAPI path inventory contains no collateral, relationship, basket, mint, merge, redeem or borrow route. This is a statement about the public participant interface checked today, not proof that internal platform facilities never exist. Older retained collateral receipts showed no components or available advance. Earlier research describing generic Advanced Logic Linking must not be presented as a usable Cup leverage facility.

Next economic implementation decision: compare an existing holding's executable conventional sale with its actual opposite-side netting alternative, accounting for live depth, fees, cash admission and canonical receipt semantics through the sole worker. Do not assume the existing implementation already supports a new operation merely because the venue does.

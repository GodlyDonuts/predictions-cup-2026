# R82: acquire SELL execution facts after native reservation and activation

R81's measured15-minute window completed03:06:17 UTC with **two distinct first-filled orders**, both buys, and zero new realized gain. Its nine-minute gap between first fills fails the one-per-minute target. Original receipts and remaining FIFO match. [Cadence](r81-cadence-window-20261022.json), [financial audit](r81-turnover-window-20261022.json).

SELL20707 on1077NO was bound at02:54:32 and expired before SDK admission. The original SDK proof establishes zero HTTP attempts; the same key was retired, not retried. Its recorded policy/risk preparation took2,211ms after the prior book had already been used for planning and activation. [Original dispatch/proof](r81-sell-expiry-20261022.json), [timing](r81-first-dispatch-trace-20261022.json).

R82 changes only worker scheduling. New reviewed SELLs first fetch one official planning book and validate owned inventory/value. They create the genuine native reservation and wait for its original activation. They then fetch the complete seven-receipt execution packet through the existing serialized account worker and submit the exact unchanged reservation through current ownership, FIFO, account, contract, depth, request and SDK checks. Existing reserved-order recovery still uses its original current-facts path. This removes the old full packet before reservation and avoids spending its freshness lifetime on planning and activation; new qualifying exits use one planning read plus seven final reads.

Original prices, quantities, costs, expiry/activation clocks, review/source deadlines, daily windows, capital limits and shared read/write limits are unchanged. The native engine and runtime libraries are byte-identical to R81. A current book that no longer supports the original limit still prevents submission. No synthetic fills or replacement of uncertain orders is introduced.

All240 worker checks pass. The added regression uses actual native passive and executable SELL reservations: an expired planning book cannot authorize them, seven genuinely newer receipts permit the same covered reservation, and original quote/FIFO/dispatch remain intact. Original capability expiry still rejects authorization. The optimized binary loads. [Qualification](r82-exit-final-facts-qualified-20261022.json), [reproducible build](tools/build-exit-final-facts-r82-20261022.py).

At qualification this candidate is **not deployed**. R81 remains the sole live worker. Actual fill-rate improvement, complete buy/sell profitability and account recovery remain unproved.

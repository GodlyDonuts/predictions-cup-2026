# ALL component-debt accounting foundation

`moloch/learning_actions/collateral_ledger.py` implements integer accounting for **accepted simulated fills**. It is not yet connected to the active training environment. GPU017 and CPU002 continue using their checked frozen sources. This addition does not establish complete ALL execution parity, profitability or a production deployment.

The source contract is the locally saved official SIG OpenAPI from October19,2026, SHA256`fbdf23e54d317392f9fb4a659b9511a01fad065e7a384f28091ef04894a732c9`, specifically `GET /account`, `POST /orders`, `PortfolioCollateral` and `AllExecutionEconomics`. The public docs URL was inaccessible to the browser reader during this work; no fresh live-schema verification is claimed. The saved source remains the explicit version under review. Original API: <https://sig.thesuper.market/api/v1/docs>.

## Implemented accounting

Each component carries an explicit scope, source hash, original SIG relationship identities, disjoint covered exchanges, complete allowed binary joint outcomes and a lending-eligibility flag. The source hash pins caller evidence; it does not authenticate it or prove that an outcome list is exhaustive. No external price, cross-venue correlation, portfolio mark or model forecast enters the payout floor. Overlapping components are rejected to prevent counting collateral twice.

Signed net positions retain YES/NO economics. Fully backed sales close conventionally; uncovered sales become full complementary purchases. Opposite purchases track redemption credits separately from nominal execution notional. Component principal remains separate from cash and is subtracted from marked equity. Buys use own-group headroom first, cash next and other eligible components last. A floor reduction repays the traded component's excess debt. Negative cash can remain a liability; buying power uses nonnegative spendable cash plus unused eligible advances.

The saved API does not specify the allocation priority among multiple other components. The primitive requires that priority explicitly whenever other-group funding is needed; it does not invent an official priority or silently assign debt. Failed funding/calibration checks leave cash, positions, debt and immutable event history unchanged. Repeated identical fill IDs replay their original result without drawing another loan; changed payloads conflict.

All internal amounts are integer micro-SUSQies; fields named after API economics are **not raw API currency JSON**. Fees are absent in this foundation, consistent with the saved competition description. FIFO attribution remains the existing execution ledger's separate responsibility; no borrowed principal is a realized trading gain.

## Verification and remaining integration

Twelve focused tests pass. They check scoped topology validation, both position sides, explicit other-group funding, own loans before cash, cash before other loans, negative-cash affordability, repayment, redemption, full complementary canonicalization, atomic rejection, idempotency and debt-adjusted equity. A500-step seeded synthetic fill path checks equity conservation at each execution mark and debt/cash identities, including rejected operations. A separate100-order scenario feeds the existing API episode's actual simulated fills into a zero-component ledger and checks exact cash/net-position agreement with its original FIFO execution after every dispatch. These are artificial fixtures, not exchange calibration, historical-market profit or full API parity.

The simulator still needs an explicit versioned bridge for actual FIFO fills, atomic baskets/rollback, partial passive fills, account/collateral reads and scoped ACK/private observations. Actual historical component topology, eligibility and exhaustive allowed states must be verified before enabling funding. Order admission/no-op/shrink rules, post-trade eligibility, unknown-floor handling, cash/repayment ordering calibration, settlement/refund/release/write-off, changing component graphs, complete endpoint pagination and official valuation remain unfinished. A post-fill accounting calculation is not proof that the exchange would accept an order at that size or allow a loan to fund its own creation. The existing floor-only simulator hook is not reclassified as this implementation.

No account request/client was added, no independent test was opened and no production bank, STOP, activation, risk cap or financial ledger changed. The requested multi-day three-venue corpus and independent profitable ROI remain missing.

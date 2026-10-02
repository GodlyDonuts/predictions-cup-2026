# Canonical execution and FIFO attribution

`canonical-fifo.mjs` is the pure accounting component for paired YES/NO execution. It is not connected to dispatch and grants no execution authority. Original frozen YES-core and side-accounting source files remain unchanged.

The official order schema describes backed SELL closes, otherwise complement BUY canonicalization, and opposite-signed BUY netting. The projection binds the requested quantity/side/limit, canonical acknowledgement, unique signed fills and complete pre-order signed position to one attributed owner/exchange. Unattributed holdings cannot be adopted. Backed sells close FIFO lots; opposite buys close existing lots at complementary proceeds, then open any residual side. Cash, surviving basis and realized result must conserve exactly. Negative results remain negative. Duplicate deliveries deduplicate; an applied order ID cannot consume lots twice.

Use an immutable pre-order wallet for every projection of that order. A partial projection reports remaining hard-limit reservation; subsequent evidence must be projected again from the original wallet, rather than applied incrementally to its earlier projection. Applying a returned wallet records the order ID and blocks a second application. The production host still needs durable baseline/projection persistence, monotonic fill history and official reconciliation before accepting a result.

Only the documented realtime account-batch YES-denominated price convention is supported for NO fills. REST NO price convention remains unresolved/rejected; this module does not infer it. Exact acknowledgement principal must agree with all bound fills. Non-null ALL collateral economics remain unresolved because nominal principal, buying power and gross cash are different quantities. Cash deltas here are nominal projections, not verified API balance changes. This distinction matters for eventual paired execution.

Oversized or flat SELL requests can be projected as canonical complement buys to detect the resulting exposure; this does not authorize naked sells. A bounded paired controller must independently reject unintended exposure and enforce its own limits. Samples and tests establish accounting behavior, not actual fills, live compatibility or profitability.

Primary schema: `openapi.json`, `POST /orders` and realtime account-batch price convention in `api-guide.txt`. Run `node --test execution/canonical-fifo.test.mjs execution/side-accounting.test.mjs execution/no-order-economics.test.mjs`.

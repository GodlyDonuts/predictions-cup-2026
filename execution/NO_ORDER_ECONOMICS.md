# Order-specific NO economics

`no-order-economics.mjs` verifies one terminal canonical NO BUY of at most six shares. It does not modify the frozen YES executor, submit orders, adopt holdings, or establish a global REST convention. SELL/complement netting, partial orders and subsequent order lifecycles remain separate work.

The official cached OpenAPI defines canonical acknowledgement prices as side-relative and REST fill quantities as outcome-signed. It leaves REST fill price basis unspecified. This verifier enumerates side-relative and YES-denominated interpretations, requiring exactly one to satisfy the hard buy limit, acknowledgement principal, isolated gross-cash decrease and signed-position delta. A half-price ambiguity rejects rather than inventing a convention.

Required evidence is a matching complete order-specific fill history, before/after scoped account snapshots with no open orders, and an independent complete account activity window excluding unrelated fills, settlements, fees and transfers. Before collection ends before preparation; after collection begins after the last fill. Cash must be exact gross cash, never collateral, marks or rounded balance. Aggregated money requires exact decimal strings; numeric-double uncertainty must be resolved upstream rather than rounded here. Both snapshots must be recent. External NO inventory remains external; existing YES inventory and netting reject.

Callers must retain source snapshots and establish those completeness/cash semantics from actual official responses. Supplying flags or passing synthetic tests is not that proof. No actual terminal NO order has yet been verified with this package, so competition package execution is not enabled by it.

Run `node --test execution/no-order-economics.test.mjs execution/side-accounting.test.mjs`.

# Relationship payoff analysis

This library implements the first deterministic payoff and executable-cost layer from Paper 2. It runs inside the existing Rust system. It creates no service, collector, account session, orders, collateral loan or automatic activation.

```powershell
cargo test --locked --manifest-path moloch/relationship-arbitrage/Cargo.toml
```

All cash, prices and payouts are integer millionths of a SUSQie. Quantities are whole shares. The crate has no dependencies. Its input contract is deliberately stricter than an active official relationship: callers must supply source-bound, independently verified terminal settlement and refund semantics. The source hashes identify receipts already verified and retained by the sole worker; this library cannot authenticate external content from a hash string.

`compile` accepts one to three exact contracts and at most sixteen declarations. It enumerates ordinary Boolean states admitted by verified mutually exclusive, exhaustive, complement, implication, AND, OR and NOT logic. It independently enumerates every declared exceptional payout for every contract, including numerical payouts that happen to equal ordinary YES or NO. Exception states do not inherit ordinary logical constraints. This is conservative for cancellations that are known to be coupled; a later verified joint-exception compiler could safely tighten that bound.

`Compiled::evaluate` checks a proposed two- or three-instrument long-only basket and quantity against synchronized full ask depth, exact entry fees, buffer, real cash, gross package cap and hard orphan cap. `scan` enumerates every such distinct basket for an explicit bounded quantity grid and returns positive candidates in deterministic surplus order. A price-constraint violation alone cannot yield a certified plan.

`portfolio_floor` and `marginal_floor` evaluate the whole current component portfolio. The incremental floor of a completing share can exceed its standalone floor. This is a structural reservation-value ingredient, not a fair-value forecast, liquid profit or collateral release estimate.

See [the integration and evidence contract](../../docs/MOLOCH_RELATIONSHIP_ARBITRAGE_20261015.md). Production dispatch, fresh account reconciliation, physical position netting, durable package intents, actual partial fills, cancel races and unknown acceptance remain responsibilities of the existing execution owner and worker.

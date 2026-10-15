# Owned-order reconciliation read planner — October 15

Task [#126](https://github.com/GodlyDonuts/predictions-cup-2026/issues/126) supplies a pure helper and direct-inclusion Rust tests. It does not edit the worker, SDK, financial engine or account queue, and makes no network request. Parent [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120) owns integration and deployment. Candidate correctness requires Linux compilation/tests before checked main publication.

## What can be saved

The existing worker obtains an individual order and then a complete fill tape for every pending acknowledged order. A fresh complete official open-order collection already contains the same `Order` schema as the individual order endpoint. A validated open member can therefore replace that individual order read. **Every owned order still receives its complete official fill tape on every cycle.**

For N pending orders and K validated open members, the original reconciliation uses 2N logical endpoint operations. A new aggregate plus the planned reads uses 1 + 2N − K. With 20 matched open orders this is 21 rather than 40 operations, including all 20 complete fill tapes. The tests model a 60-second interval between cycles; there is no short deferral timer that expires before the next cycle. Reusing an aggregate already obtained in the same cycle can save its additional operation. Fetching an aggregate solely for one order cannot improve the two-operation baseline.

These are endpoint-operation counts, not measured requests per minute. Open lists and fill tapes may paginate through several physical HTTP requests, latency varies, and stale or inconsistent data adds fallbacks. The shared admission queue, pacing and API limits remain unchanged. The current helper is not evidence of achieved production speed.

## Official contract and its limit

The official [API reference](https://sig.thesuper.market/api/v1/docs), cached locally as `runtime/moloch/api-spec-ioc-20261014.json` and inspected on2026-10-15, defines the same `Order` response for list members and individual GETs. While open, positive `quantity` is executable remainder; `quantityFilled` is null. Closed quantities can reflect reductions and cannot be treated as the original admission. `coverage.complete` describes untruncated cursor coverage. `projectedThroughSequence` is a reporting checkpoint and can be null. Neither field proves synchronization to the current engine head.

Consequently, the planner never concludes that no newer fill exists, never postpones a fill-tape request and never advances a fill-confirmation watermark from the open list. A lower positive remainder can reflect a fill or a reduction; it changes no ledger admission, cash or inventory. A remainder greater than admitted quantity minus already credited fills requires exact fallback.

## Integration contract

The new module is `moloch/live-runtime/src/reconciliation_plan.rs`. The independent test target includes it directly; adding `pub mod reconciliation_plan` to the existing library and editing the worker are left to the parent.

1. Build `OwnedOrder` from each actual bound native order. Native quantity is the actual ACK admission after any accepted resize. Supply current official credited fill quantity and cancel/unknown flags. Include global unbound uncertainty in `unresolved_acceptance`. Never derive admission from an open remainder.
2. Obtain and privately retain `Endpoint::Orders { status: "open" }` using the existing serialized transport. Preserve full original endpoint, receipt clocks, merged pagination/coverage and rows. Do not synthesize an `/orders/{id}` receipt.
3. Call `plan` with the receipt, current pending orders, previous optional `FillWatermark`s, previous aggregate watermark, current clock and `Limits`. Default maximum aggregate age is20seconds; the helper refuses a larger limit. The SDK's earliest request clock and minimum projection sequence across all pages remain authoritative inputs.
4. For `OpenMemberNeedsFillTape`, call `member_is_fresh` immediately before the request. If its deadline passed while earlier orders were processed, use an exact pair. Otherwise request the COMPLETE `OwnedFills` tape after the aggregate receipt. Run `validate_member_fills` before applying events: scope/order validation in existing `feedback::fills` remains mandatory. If the new guard fails, fall back to a fresh exact order and a new later complete fill observation.
5. Apply ordinary official fill feedback and durably journal it. Do **not** call `feedback::terminal` from an open-member branch. Do not close the dispatch log from absence, remainder, list membership or this planner. A full-fill race may retire the native order through actual fills, but dispatch terminal closure waits for exact official reconciliation on a later cycle.
6. For `ExactPairFallback`, keep the existing OwnedOrder → later COMPLETE OwnedFills → `feedback::fills`/`feedback::terminal` path. Missing open-list membership, expiration, cancellation, zero remaining quantity, unknown acceptance, malformed terms, stale/incomplete lists or regressing projection/receipt clocks cannot retire an order by themselves.
7. Only after validated complete fills are durably applied, `fill_watermark` can retain that tape's hash, actual credited count and checkpoint. Prune watermarks for orders no longer pending. The type is intentionally not deserializable: after restart rebuild from fresh normal reconciliation. An aggregate observation remains a separate projection watermark.

The plan reports a gate for foreign order IDs, unknown acceptance or invalid aggregate evidence. That flag cannot clear existing stops or authorize a submission. Preserve all existing fresh cash/holdings/open-order/contract/book and physical-dispatch authority checks. The optimization only changes the source of an affirmative **open** observation; actual fill evidence and terminal liability release remain independent.

## Verification scope

The candidate target is:

```text
cargo test --locked --manifest-path moloch/live-runtime/Cargo.toml --test reconciliation_plan
```

Tests cover all YES/NO buy/sell pairs; a realistic 60-second/20-order schedule; unchanged retained fill watermarks; reduced remainder without inferred fills; missing/foreign/duplicate orders; exact canonical terms; full pagination and minimum projection guards; clock rollback/future/staleness; unknown acceptance/cancel/expiry; per-order hint expiry in a large cohort; complete tape ordering; partial/full-fill races; count consistency; duplicate source rows; and seven-fractional-digit RFC3339 clocks. Tests calculate endpoint-operation savings, not a market edge, actual throughput or P&L.

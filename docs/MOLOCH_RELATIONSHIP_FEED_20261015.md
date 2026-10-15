# Official relationship and collateral feed

Task [#123](https://github.com/GodlyDonuts/predictions-cup-2026/issues/123), under production owner [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120). Session `Viserion/relationship-feed-20261015`. This is the participant-readable feed layer for Paper 2's portfolio-logic strategy. It extends the existing Rust transport; it does not create another account client or deploy a strategy by itself.

## Source and limits of verification

The targeted schema audit used the saved public OpenAPI `runtime/moloch/api-spec-ioc-20261014.json`, SHA256 `6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`, whose title is `Super Market API`, version `1.0.0`. The public reference is [SIG API v1](https://sig.thesuper.market/docs/api/v1); current account limits are described in the [official changelog](https://sig.thesuper.market/docs/changelog). On October 15 this task's Windows host could not resolve SIG's public hostname and the web tool could not open the reference. Consequently this task does not claim a fresh endpoint connection or current Cup graph availability. The production owner must verify the current schema and receive scoped responses through the sole worker before relying on the feed.

The OpenAPI's SIG participant-access section explicitly includes relationships, portfolio and `/account`. The separate `/dmm/relationships` mutation, deletion and relationship-resync operations require admin authority and are excluded from this SDK's endpoint enum. A public API description does not establish a positive collateral floor, lending availability, or accessible relationships in this tournament.

## Fixed scoped read surface

| Rust endpoint | Official read | Bound and validation |
|---|---|---|
| `Relationships` | `/relationships` | Explicit tournament UUID; optional market **or** exchange filter; optional known type; depth 1–4; limit 200; existing configured maximum pages; bounded encoded cursor |
| `RelationshipGraph` | `/relationships/graph` | Explicit tournament UUID and known market; depth 1–4; no invented pagination |
| `RelationshipConstraints` | `/relationships/constraints` | Explicit tournament UUID; optional market/canonical relationship UUID; violations-only flag and optional exact decimal minViolation 0–1 |
| `Collateral` | `/portfolio/collateral` | Explicit tournament UUID; every returned component must echo that tournament and name only known scoped exchanges |
| `Account` | `/account` | Explicit tournament UUID; normalize only cash, available advance and buying power; profile information is not copied to strategy reports |

All requests use `Transport::request` → `perform` → the existing serialized durable account admission queue. There is no new HTTP client, account writer, key, bypass or parallel request path. Existing retry/backoff, response-size limit, unknown-write acceptance, order permissions and nullable order-ACK `all` handling remain unchanged. Relationship reads spend the same account budget as every other read.

The SDK builds its own URL. `constraint.evaluationEndpoint`, market titles, cursor contents, suggestions and all other response text are untrusted data. A supplied evaluation URL is retained for evidence but is never fetched. Cursor bytes are percent-encoded; no response URL can redirect authentication.

## Normalization contract

`relationships.rs` exposes serde types `Relationship`, `RelationshipType`, `RelationshipStatus`, `RelationshipSource`, `Node`, `NodeRole`, `Direction`, `DirectionKind`, `Expression`, `RelationshipList` and `Graph`. `parse_list` and `parse_graph` check exact exchange-to-market mappings, canonical UUIDs, positive versions, unique IDs and graph reference closure. Documented null metadata remains null; missing required nullable fields are rejected rather than silently interpreted as an explicit unknown. Inactive relationships remain diagnostics. Unknown types/statuses/roles are rejected rather than assigned invented semantics.

The canonical types are `monotonic`, `complementary`, `mutually_exclusive`, `implication`, `boolean_and`, `boolean_or`, `boolean_not`, `conditional_if` and `boolean_expression`. Direction is separately represented as `antecedent_to_consequent`, `condition_consequence_to_output`, `inputs_to_output`, `symmetric` or `ordered_members`. Boolean AST references are bounded to 32 levels/4,096 nodes and must refer to the relationship's declared exchanges. An empty AND/OR is rejected.

These enums alone are **not truth-table or settlement certificates**. The audited schema does not establish a complete monotonic ordering convention, a material-implication truth table for `conditional_if`, or cancellation/refund behavior. `trading_semantics_ready()` means only active status plus complete structural node metadata. It cannot authorize a trade. The portfolio solver must independently validate the relation's direction/roles, exact contract resolution evidence, all permitted joint outcomes and N/A/refund states. The `isExhaustive` flag is retained and must never be guessed from a title or the number of nodes.

List pagination is completed through the existing request loop. Duplicate relationship UUIDs across pages, empty intermediate pages, repeated cursors, missing final cursor termination and configured page exhaustion fail closed. A merged receipt retains the first request-start clock and last response clock, representing a collection interval rather than a simultaneous graph snapshot. It must be refreshed and version-checked before execution.

## Economics remain separate

| Value | Meaning | What it does not establish |
|---|---|---|
| `grossCashUnits` | Scoped account `balance`, nullable | Buying power, realized profit or historical lot cost |
| `availableAdvanceUnits` | Engine says eligible components can still lend this amount | Cash or earned return |
| `buyingPowerUnits` | Scoped `max(balance,0)+availableAdvance`, nullable with balance | Net wealth or a permission to multiply funded risk limits |
| `outstandingLiabilityUnits` | Sum of reported component `outstandingAdvance` | A second source of assets or realized income |
| `guaranteedPayoutFloorUnits` | Engine's currently reported component floor, **nullable** | An incremental new-package floor, immediate cash payout, or N/A-safe strategy by itself |
| Constraint `bound`, `lowerBound`, `upperBound` | Current engine-evaluated price inequality | A guaranteed portfolio payout |

Financial values use exact signed millionths and checked addition. No float is used to size money or a payout. Constraint residuals may have more than six decimal places, so they remain their original `serde_json::Number` lexemes; floating conversion is confined to finite/range diagnostics. A constraint's corrective trades are evidence, never automatic dispatch instructions.

The official collateral **census** does not separately report a repayment cash flow. The order ACK's nullable `all` object does: `parse_execution_economics` separately normalizes full notional, effective entry cost, collateral savings, own-component floor/advance, collateral repayment, complementary redemption credit and signed net buying-power impact. It checks the documented full-notional minus effective-cost identity, keeps null `all` unknown, and never treats a borrowed advance or a one-SUSQie redemption as profit by itself. ACK savings can include borrowing from other eligible components even when the traded exchange is unlinked; its component ID and relationship IDs do not enumerate those other lenders. Repayment and trade effects still need reconciliation with official transactions/account changes. Existing attributed lots and explicitly unknown inherited basis stay intact.

## Production integration and proof obligations

The parent worker should issue one bounded census through its existing transport, journal source-bound receipts, normalize them, and expose `available`, `empty`, `forbidden`, `unavailable` or `invalid` status per surface. A successful **empty** graph is a real result and cannot be replaced with title-based relationships. A permission failure or absent floor cannot become a zero-risk investment thesis. Optional alpha-feed unavailability must not erase owned-order reconciliation or create an independent poller.

Before any package is dispatched: refresh account/holdings/open orders/fills, contract settlement evidence, relationship versions/status, component liability and all legs' executable depth. Reserve full funding in the existing ledger and keep stable logical intent identities. Non-atomic partial acceptance cannot be disguised as a completed arbitrage. The parent execution implementation and its tests decide dispatch; this task adds only reads and typed evidence.

## Validation

Focused tests cover scope/path injection, explicit tournament selection, cursor encoding and duplicate-page rejection, valid empty census, graph closure, foreign exchange references, unsupported semantics, inactive/null metadata, closed AST references, missing nullable fields, null floor, liability/advance separation and exact buying-power arithmetic. Local Rust/WSL was unavailable; Linux execution results and checked source commits are recorded in #123 by the parent before deployment. No profitability or actual account connection follows merely from passing these parser tests.

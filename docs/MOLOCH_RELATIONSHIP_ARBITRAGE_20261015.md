# Paper 2: canonical payoff packages and portfolio value

Task: [#124](https://github.com/GodlyDonuts/predictions-cup-2026/issues/124). Parent execution: [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120). Official relationship and collateral transport is developed separately in #123. This document describes source implementation and its integration contract; it does not assert that a qualifying live package was found, submitted, filled or profitable.

## The change

Paper 2 correctly changes the decision unit from an individual binary quote to a portfolio of related payoffs. The new dependency-free Rust crate `moloch/relationship-arbitrage` supplies that layer without creating another backend or account client. It can be called by the existing sole Rust worker, alongside its ordinary inventory management. The paper's historical 0.955 dual-NO observation motivates the hypothesis; it is not reused as a current executable opportunity or proof of settlement semantics.

The compiler receives exact exchange, market and contract IDs. No title matching, party name, price correlation, active-relationship flag, relationship price bound, recommended trade or collateral advance can establish a terminal payoff. Each declaration requires both its official source receipt and a separate authoritative settlement-logic receipt. Each contract requires complete settlement-rule evidence and every exceptional payout. Unknown or incomplete semantics fail closed with a specific error.

This is a necessary distinction because the current official schema describes relationship types and quoted-price constraints without supplying complete truth tables for every type. In particular, this implementation does not reinterpret `conditional_if` as material implication or infer the order of a `monotonic` relation. Those remain unsupported until exact semantics are verified.

## Implemented calculations

For each component of one to three contracts, the compiler enumerates all ordinary Boolean assignments and applies independently verified declarations:

| Declaration | Ordinary admissible states |
| --- | --- |
| Mutually exclusive | At most one member YES; all-NO remains possible |
| Exhaustive mutually exclusive | Exactly one member YES |
| Complement | Exactly one of two contracts YES |
| Implication A→B | Excludes A=YES, B=NO |
| AND/OR output | Output equals the corresponding Boolean function of inputs |
| NOT output | Output is the inverse of input |

Every declared N/A/refund payout is then independently combined with the other contracts' ordinary and exceptional payouts. Ordinary Boolean constraints are deliberately not applied to these exception combinations: a contract's cancellation may invalidate an implication or exclusion that applies to ordinary results. Even an exception paying numerically `(YES=0, NO=1)` must remain an exception. Deduplicating it into an ordinary NO state can incorrectly remove a low-paying terminal state and invent an arbitrage.

The component is rejected if its ordinary logic has no feasible assignment. The maximum is 216 enumerated states, sixteen declarations, three contracts and four exceptions per contract. Larger graph components require a separately verified projection or later larger-component solver; silently truncating a component is not authorized by this library.

For basket instruments j, share quantity q, ask depth cost C, entry fees F and safety buffer B, the check is:

```text
floor   = min over every feasible state (sum of all instrument payouts × q)
surplus = floor - full-depth acquisition cost - entry fees - buffer
```

Surplus must be positive and satisfy the configured minimum. All amounts use checked integer arithmetic in millionths of a SUSQie; no floating-point marks or top-price extrapolation enter the result. Every requested share must fit observed full ask depth. Receipt freshness and underlying snapshot freshness are checked separately. `Book.observed_ms` is the verified venue snapshot clock, or a conservative original request-start clock if the venue omits one; reading a cached book cannot refresh it. Cross-leg skew is measured on these snapshot clocks, not newly received response times. Fees and buffer are explicit inputs, not silently assumed zero. Real cash and the gross package cap must cover the full acquisition plus entry fees; possible future collateral advances do not increase either input here.

Examples:

* Two mutually exclusive candidates' NO contracts have an ordinary floor of one even if a third-party win is possible; in that third-party state both NO contracts pay one. Exhaustiveness is not needed for this lower bound.
* The corresponding YES pair has a floor of zero unless collective exhaustiveness is independently established.
* A verified A→B implication gives an ordinary floor of one to NO(A)+YES(B). Reversing the direction does not.
* A three-candidate exhaustive winner set gives a floor of one to the three YES shares. At 0.30 full-depth cost per leg, ten complete packages cost 9.0 and have a 10.0 ordinary floor before fees/refunds/buffer.
* A refund or N/A state can lower any of those floors; the actual minimum across all allowed states controls the decision.

## Leg risk and execution boundary

A profitable completed basket is not an atomic trade. The plan therefore reports a hard orphan cash-loss bound equal to the full funded acquisition cost plus entry fees. This survives disappearance of all hedge/unwind bids and unequal partial fills on every leg. A maximum over only fully acquired proper subsets would understate exposure when all legs partially fill in different proportions.

Observed full-leg-subset liquidation depth also yields an optional diagnostic gross unwind loss. It is not a hard guarantee: books can disappear and exit fees are not included in that diagnostic. Missing or malformed bid depth makes the diagnostic unavailable while the hard fully funded loss bound remains valid. The configured orphan cash cap is applied to the hard bound.

Before dispatch, the existing worker must still:

1. Reconcile exact cash, positions, recent actual fills and open orders under its existing exclusive account lock and persistent queue.
2. Persist a stable package ID, exact per-leg intent IDs, component/version, source hashes, quantities, limits, full funding and maximum orphan loss. Existing inventory and netting must be incorporated explicitly; a long-only plan by itself does not model the cash impact of offsetting an existing opposite holding.
3. Recheck relationship status/version, contract/rule semantics, source ages and full executable hedge depth at the current decision time. A cached scanner result is never dispatch authority.
4. Keep one serialized writer. Record actual fills before determining the remaining hedge. An open-order acknowledgment is not a fill, public tape is not own execution, and hedge price must remain within the durable package economics.
5. On unknown acceptance, stop the package and reconcile the exact stable intent. Never submit a replacement or next leg merely because the response was missing.
6. Preserve any residual orphan as attributed physical inventory if the hedge cannot be completed inside the bound. Do not label an incomplete package guaranteed, force a speculative hedge, or count its projected floor as realized cash profit.

This crate does not implement those account mutations. It supplies auditable calculations for their integration. The existing worker's stop controls, activation deadline, cash allocation and write budget remain authoritative.

## Official SDK adapter contract

The SDK's normalized `Relationship` retains canonical ID/version/status, member exchange and contract IDs, node roles, directed `fromExchangeIds`/`toExchangeIds` and any expression. An adapter must resolve each reference to the exact `Contract` index in this compiler, preserving direction. Its `settlement_logic_source` stays `None` until an official rule receipt proves that the relationship constrains terminal payouts, rather than quoted prices only. `compile` then rejects it with `SETTLEMENT_LOGIC_UNVERIFIED`.

The SDK's `trading_semantics_ready` is a structural prerequisite, not a settlement certificate. The adapter must also reject stale/inactive rows, missing contract IDs/outcome meanings, ambiguous direction, unknown refunds and changed versions. A generic constraint `bound` must never be copied into a payout vector. A collateral `guaranteedPayoutFloor` describes the specified actual portfolio/component and cannot certify an arbitrary proposed package without an exact composition proof.

The parent worker retains raw official receipts privately and verifies their SHA-256 before constructing `Source`. A `Source` value in this library checks hash shape, clocks and policy age, but cannot authenticate the bytes or provenance of a supplied hash. Every accepted plan includes all proof/book source references and canonical relationship versions so the durable executor can bind it to those receipts.

## Portfolio marginal value

`portfolio_floor` calculates the minimum payout of exact existing component positions. `marginal_floor` recomputes that minimum after the proposed shares are added. For example, owning five NO(A) in a mutually exclusive A/B component has standalone floor zero; adding five NO(B) raises the component floor by five. Adding another five NO(A) does not.

This incremental floor is suitable as one structural term in a portfolio reservation value. It does not estimate settlement probabilities, account for every correlated market outside the component, predict adverse selection, release collateral or establish realized P&L. Any standalone fair-value/flow/fee/inventory terms must remain separately identified and validated.

## Tests and current evidence

The focused suite covers ordinary and exceptional payoff states, exclusion versus exhaustiveness, implication direction, AND/OR/NOT, three-leg packages, within-market complements, missing rule certificates, unknown enums, duplicate/out-of-scope/inconsistent declarations, exact full-depth consumption, malformed unused levels, insufficient depth, fees/buffer, capital and orphan caps, evaporated bids, receipt age/skew/scope, deterministic scan order and marginal portfolio value. A dedicated regression preserves exception-state identity when its payout equals an ordinary binary state.

The parent execution task reproduced all 31 scanner checks on Linux for the source through `db6dc2d2`. This is mathematical and input-validation evidence, not a live profit result. The subsequent runtime adapter tests are a separate suite and require their own Linux validation. No account request, live order, collateral experiment or runtime change was made by this task.

## Runtime adapter and explicit certificates

`moloch/live-runtime/src/relationship_strategy.rs` now connects the already received official census to the payoff library. It performs no file access or account request. The parent integrates its Cargo path dependency and module separately, leaving ordinary production running during offline development. The adapter receives the `Census`, its original official relationship `Receipt`, fresh normalized books, exact attributed positions, current cash/reservations and a bounded quantity grid.

The original relationship receipt must hash to the census evidence, explicitly identify the tournament in its endpoint, be fresh and parse under the exact SDK scope. Its normalized rows must equal the current cache. A partial first page is insufficient. Mutating a cached relationship cannot reuse the original receipt hash. No price-band or collateral surface is consulted to create a payout.

A `SettlementCertificate` is a reviewed interpretation supplied by an independent rule verifier or the execution owner. This adapter does not read freeform rules and assert that it understands every outcome. A certificate's exact serialized SHA-256 must already appear in a separate trusted allowlist. Merely receiving or loading a certificate must never add its own hash to that allowlist. This is the explicit trust boundary, not a cryptographic signature proving that an API endorsed the interpretation.

Each certificate binds the tournament, canonical relationship ID/version and complete normalized-row digest, original relationship receipt digest, a named verifier and validity window, exact ordered exchange/market/contract IDs and outcome labels, and original official rule receipt hashes. Each raw rule receipt is rehashed, checked against the precise `/markets/{id}` or `/markets/{id}/nodes` path and market echo, and checked for freshness. The certificate must state ordinary binary payouts, complete exceptional payouts and whether the relationship is exhaustive. Missing, unapproved, ambiguous, expired, mismatched or incomplete certificates yield explicit blocked reasons. A changed canonical receipt requires a freshly bound certificate; no silent certificate rebinding occurs.

The first adapter supports ordinary symmetric mutually exclusive and complementary declarations with two or three members. Exclusion exhaustiveness must match both the canonical row and the reviewed certificate. Other relationship types remain visible as blocked unsupported inputs; the adapter does not guess their semantics. A new verified interpretation can be added through checked code and tests.

Gross account cash and attributed bank cash are separate inputs. Available funding is the smaller balance less existing reservations, bounded again by remaining committed capital and package/orphan caps. Collateral advances are absent from the funding input. Physical netting and cross-component holdings remain future execution responsibilities. Current component holdings are used only to report the existing floor and each candidate leg's marginal portfolio floor.

The output deliberately separates privacy surfaces:

* `Analysis.public` contains categorical feed availability, counts, constant blocked-reason codes and `executionAuthority:false`. It contains no account balance, profile, market title, exchange ID, candidate price, private filename or receipt hash.
* `Analysis.candidates` is deliberately not serializable by default. `PrivateCandidate::private_json()` explicitly emits exact amounts as decimal strings, leg identities, certificate and source references, full-depth cost, payout floor, hard orphan bound and marginal portfolio values for private evidence storage.
* Every private candidate retains `PACKAGE_AUTHORITY_AND_DURABLE_EXECUTOR_NOT_INTEGRATED`. A mathematically qualifying completed package is not an instruction to bypass the ordinary passive-entry guard, place a taker hedge or submit an order.

Candidate retention is bounded to a configured maximum of 256, while public counts distinguish total positive candidates from the retained best candidates. Ranking is deterministic. This is not a multi-package allocator: plans may share liquidity, cash and contracts and must not be submitted concurrently as if each had independent funding.

## October 15 live census result

The parent execution task reported the first production census at **2026-10-15 09:05:13 UTC**: all requested participant endpoints returned HTTP 200; the official relationship list, constraint list and collateral component list were all empty, with zero active/exhaustive relationships and zero known payout-floor components. The existing ordinary production worker continued. This is an empirically available but empty feature, not a 403 permission failure and not proof that every potentially related contract is unrelated in terminal reality.

The adapter therefore exposes `canonicalFeedState: "available_empty"` with zero candidates for that baseline. Forbidden and unavailable evidence produce different categorical states and blocked reasons. It does not manufacture party pairs to replace missing canonical declarations. Independent authoritative contract-rule verification is a separate research path, and any later support requires exact source-bound logic and exception proof.

## Continuing the research

First produce a source-bound census through the existing official worker: canonical components and versions, proved versus missing settlement/refund conditions, depth timestamps/quantities, executable package costs, floor and residual orphan bound, and separately observed collateral/liabilities. Missing information should appear as an explicit blocked condition, not disappear from the report.

Only packages passing the current source, cash, payoff and execution bounds are candidates for a durable live package executor. Measure completed-package actual acquisition cost, leftover inventory, gross cash, collateral advance, outstanding liability, repayment/redemption and settlement separately. Improving source completeness and measured package economics is the next step; increasing trade count alone is not an economic validation.

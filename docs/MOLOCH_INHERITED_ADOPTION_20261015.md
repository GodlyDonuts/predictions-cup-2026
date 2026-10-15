# Reconciled inherited-position management

Human authority: the execution owner may manage inherited holdings and buy or sell when an advantage exists. Task #122 adds exact opening attribution; parent task #120 owns actual account operations. This code makes no account requests and creates no historical fill or order.

## Native event

`actual_position_adoption` is an official-receipt event, accepted only by a live engine. `inventory::adoption(receipt, engine, tournament_id, tournament_slug, exchange_market_map, processed_at)` normalizes an already obtained complete positions receipt. The live-runtime module registry exposes the helper as `inventory`. The helper performs no network call.

The event carries `source: official_sig_api_v1`, `authority: human_moloch_inherited_management_20261015`, `complete: true`, the exact receipt hash, original request/receipt clocks, processing time, tournament, all normalized position rows, `openingCost` and `effectiveMaximumMarkets`. Lot IDs, positive quantities, outcome side, entry prices and original openedAt values remain source evidence. Signed row quantities must equal the lot sum and outcome. RFC3339 timestamps preserve up to nanoseconds, including the observed seven fractional digits.

Native admission rejects duplicate rows/lots, missing owned positions, changed scope, invalid side/quantity/basis metadata, stale receipt, insufficient cash/committed capacity, and every unresolved native order/reservation. Previously owned markets are reconciled and skipped; only markets absent from the wallet are imported. A second adoption batch is rejected. `Session::accept` makes identical repeated source delivery exactly once.

Exact open-lot principal becomes owned cost and debits attributed wallet cash. It leaves initialCash, maximumCommitted and actual official account cash unchanged. `engine.opening_inventory_cost()` and report.openingInventoryCost remain constant after later sales. The parent account-cash invariant must therefore be:

    expectedOfficialCash = initialOfficialCash + walletCash - initialAllocation + openingInventoryCost

Reported official position costBasis is preserved separately from lotPrincipal. The exact difference is classified `exact`, `cent_rounding` (at most 0.005 SUSQie), or `reported_discrepancy`. Material differences do not prevent management of all other positions. They also do not establish official realized-account P&L. Inherited closing results are explicitly lot-basis FIFO gross; `newTradingRealized` excludes that gross, retaining actual fees. Original source openedAt is retained without pretending adoption was a historical execution.

The profitable covered-exit management age starts at adoption processing time. Pure opening inventory never enters the new-position maximum-age loss timer, including after61 seconds. Only actually owned post-adoption BUY lots can activate that loss timer; ordinary new positions retain their original60-second cap. Existing holdings remain fully marked and retain profitable covered exits. Original policy maximumMarkets is increased by the adopted market count through the durable event; the original immutable manifest policy remains the replay seed. A separate fresh-market gate preserves the original new-market allowance even after inherited positions close. Existing principal still consumes the same committed allocation and per-market buy headroom.

## Exact engine identity bridge

Native changes create a new engine identity. `propose_live_engine_upgrade(bank, old_binary_sha256, approved_at)` returns a bounded private proposal and never writes or approves it. The offline `moloch-upgrade BANK OLD_BINARY_SHA256 APPROVED_AT NEW_PROPOSAL_OUTPUT` command writes a new mode600 proposal file only. It has no account access and does not activate the proposal.

The parent must first stop and verify the old pinned binary, reconcile its physical dispatch state and preserve its last committed sealed head/report. The helper acquires the bank writer lock and verifies the immutable manifest, full journal chain, exact head/report agreement and raw journal prefix hash. After inspecting the proposal under the human's production authority, the parent can install it as private `bank/engine-upgrade-v1.json`, without editing manifest.json, head.json or any old event.

The proof is an at-most16-entry sealed approval chain, bounded to8MiB. Every bridge binds original manifest hash, exact from/to engine identities, prior approval hash, old binary SHA256, approval clock, committed old head, raw journal prefix bytes/hash and sealed old report envelope. A new engine replays the prefix and must reproduce the entire old report, logical orders, fills, source cursors and actual order-binding count before accepting the identity bridge. Paper sessions do not accept this mechanism. Changed policy/seed/manifest, truncated/altered prefixes, wrong reports, target identities or approvals fail closed.

The bridge boundary stays at the historical prefix. Later suffix events, including adoption and actual fills, replay after it; ordinary restart does not require rewriting the approval or old manifest. Future approved upgrades append another bounded identity bridge. This is explicit compatibility evidence, not a general flag to ignore engine identity.

## Parent integration sequence

1. Resolve every uncertain physical acceptance by known order ID. Cancel/reconcile bound liabilities; retire only demonstrably unattempted native quotes. Adoption requires no unresolved native obligations. Do not assume the bank is unused. Stale/invalidated books during maintenance avoid immediately planning replacement quotes before the import.
2. Build and test the new native/live source separately. Propose and approve the exact engine bridge against the stopped old bank and pinned binary; keep the source prefix and original manifest intact.
3. Open the existing live Session with the original control policy/seed. Freshly obtain the complete official positions receipt through the existing single queue, normalize and append the adoption event using its own stable source ID/sequence.
4. Add openingInventoryCost to the account-cash expectation, compare all physical holdings to attributed quantities, include adopted markets in the discovery/required scope, and size REST selection using the effective policy maximumMarkets. Keep three fresh-market slots in addition to inherited obligations, without another account worker.
5. Resume the existing worker with an immutable checked binary. Verify actual order/fill receipts, accounting and fresh process health. Do not call the lot-basis inherited gain official account profit or count old fills as new Moloch fills.

Final checked integration source is `572034cbb723360fd34b9ecc47759be00b6104b3`. Linux suites pass81 native checks and33 live checks (30 unit plus3 normalization helper checks); locked release builds of the upgrade helper and live runtime/reconciler also pass. The merged native/live/SDK/account-queue source trees match that checked integration exactly. The parent still must verify the exact production prefix bridge, adoption event and new live service before deployment/health is claimed. Actual profitable production remains unproven.

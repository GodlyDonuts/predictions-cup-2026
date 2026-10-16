# Existing account cash versus Moloch allocation — 2026-10-16

The retained official tournament balance is not an additional 72,774.10 on top of Moloch's allocation. The original sealed seed captured cash 70,850.430000 (2026-10-15T07:54:10.732Z). With original initial capital 40,000, inherited opening-lot cost 24,729.485000 and the parent's observed Wallet cash 17,194.180000:

`expected official cash = 70,850.430000 +17,194.180000 −40,000.000000 +24,729.485000 =72,774.095000`.

The retained tournament observation 2026-10-16T08:44:27.894Z reports 72,774.10, consistent with its documented two-decimal rounding. This is historical evidence, not a fresh dispatch snapshot. Remaining unallocated original cash is 55,579.915000. Allocating that amount raises effective contributed capital to 95,579.915000 and attributed cash to 72,774.095000 at that same financial frontier. These figures use original lot principal; they are neither marked equity nor profit.

## Source-only implementation

`live_capital_allocation_v1` is a distinct owner-attribution event. It adds exactly the same positive delta to Wallet.cash and effective Wallet.initial, and adds that delta to current maximumCommitted. It preserves the immutable manifest and original policy.initialCash 40,000, all actual fills/orders, FIFO costs, inherited/new PnL, fees, reservations, market 10,000 limit, quantity, breadth, entry deadline and rate policies. Explicit ownerCapitalAllocation report records retain a bounded contribution chain, approved mandate/control hashes, original source evidence and prior allocation hash. Legacy reports omit this field entirely before the first event.

Session checks the real run/manifest/head/report boundary, original seed-derived remaining cash and exact official Account balance. The native transition also checks full positions against owned quantities, a complete empty official open-order projection, zero debt, zero available advance, original scoped receipt clocks and cash/buying-power consistency. Phase1 deliberately requires an all-closed physical boundary; unsubmitted ordinary reservations may remain. A positive, hydrated original SDK no-network proof plus actual native retirement can satisfy that boundary without changing its historical attempted/not-closed audit. Replayed proof JSON alone cannot.

The opaque runtime factory consumes the existing sole worker's Transport, held owner lease, original dispatch logs and original retained Account/Collateral/Positions/OpenOrders receipts. It performs no HTTP. Commit rechecks native head, actual queue/global unknown state, original held files, exact bytes and clocks; prepared capabilities expire after two seconds. The trusted mandate SHA must come from separately pinned execution-owner Control, not an API response or self-approved artifact. No worker/control activation is included.

`authority::reconciled_expected_cash` already subtracts effective Wallet.initial: the equal cash/initial increment cancels. No official balance is credited twice. A distinct allocation ID, prior chain hash, exact previous capital and the remaining seed surplus prevent repeated funding. Restarts require original journal replay. Standalone restoration cannot discard contribution provenance.

## Existing collateral accounting is not a funding mechanism

`actual_collateral_baseline` tracks official cash/debt separately; it requires current cash to match the original seed-derived view. Ordinary cash synchronization only explains actual owned terminal fills through original ACKs and complete fill tapes. Neither operation adds Wallet sizing cash. The worker currently rejects enabling collateral accounting for unsupported ordinary flows. Phase1 allocation therefore rejects an initialized collateral ledger; it does not turn that feature on.

The retained OpenAPI distinguishes Account.balance from availableAdvance and buyingPower, and distinguishes outstandingAdvance from component payoff floors. The conservative zero-advance guard avoids promising cash-only execution where venue rules can automatically draw advance. It is a reviewed constraint, not evidence that the actual current account has zero borrowing capacity. Original Account and Collateral receipts must establish eligibility.

## Promotion requirements

The execution owner must review source/test evidence, register the new runtime module, pin the mandate, update the effective Activation/Control monetary cap consistently, and install the native engine through the existing exact-prefix upgrade proof. The current manifest seed must remain untouched. Existing entry-window monetary mandates also need consistent explicit limits. The sole worker must reach and freshly verify the all-closed/zero-unknown boundary before preparing the event. No new account client, account reset, inherited re-adoption, source-clock renewal, unknown release or automatic allocation is supplied here.

Additional capital changes funding headroom only. It does not establish that more executable positive-EV opportunities exist or justify trades that fail current value/depth/source guards.

## Code/schema evidence

- `native/src/engine_capital.rs`, `native/src/session.rs`, `native/src/lib.rs`: exact contribution and replay/accounting guards.
- `live-runtime/src/capital_allocation.rs`, `collateral_feedback.rs`: original retained-source and sole-worker boundary admission.
- `live-runtime/src/authority.rs`: seed cash +Wallet.cash −Wallet.initial +opening-lot cost.
- `native/src/engine_collateral.rs`: separate official cash/debt ledger and unsupported funding semantics.
- SDK `Endpoint::Account`, `Collateral`, `Positions`, `Orders`: documented exact scoped endpoints; positions are the unpaginated full portfolio schema and Orders requires complete pagination/coverage.
- Retained official OpenAPI artifact `runtime/moloch/api-spec-ioc-20261014.json`: TournamentSummary/Account/PortfolioCollateral/AllExecutionEconomics. Cached schema is primary evidence; no new account read was performed for this audit.

## Isolated ARM64 verification � 2026-10-16T09:17:16Z

Source base7edfe62a; candidate through7e1ddfe4. A new private source directory verified all202 source/dependency/fixture files by exact SHA256 and size before testing, and verified them again afterwards. Full native tests:238 passed, zero failures, one deliberately ignored copied-live-prefix benchmark (requires separate explicit prefix/bytes/SHA inputs). This includes six new allocation tests. New runtime allocation tests:6 passed. Existing collateral-feedback regressions:13 passed. The original failed fixture/compiler logs were retained; no financial guard was relaxed to make fixtures pass.

No account HTTP, live queue/bank/control mutation, release build, engine bridge or production allocation was performed. Runtime registration and physical rollout remain execution-owner work. Private validation report SHA256:f936982823f4bc7d7ccea2bde79aaa3dbd5fa92f92e6a0677abf1096b899fbe5; final202-file source manifest SHA256:848e34031d951d692250f384bc877407c515a4d655fb8bfb824b6732e95e3c30f.

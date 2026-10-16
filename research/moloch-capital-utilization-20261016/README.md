# Capital utilization: static seed versus effective live limits

**The original `maximumMarkets:3` is not the current ordinary-entry cap.** The execution owner supplied the actual stopped observation at native sequence247652: fresh-market limit12, effective total-market limit40, effective maximum commitment95,579.915000, attributed cash72,773.995000 and owned cost22,731.515000. Thus changing the old seed's3 would target the wrong limit and is not justified by this observation.

Read-only source baseline: e7, unchanged relevant risk/order/capital modules in the buffered-JSON successor. No SSH, API, queue commands, live file reads, control changes, financial edits or deployment by this task. No new test harness was needed: this is a trace of existing checked paths, not a new policy proposal.

## Actual boundary evidence provided by the execution owner

The original `ordinary-market-breadth.json`, installed06:41:15.874UTC, reports `freshMarketLimit:12`, `effectiveMaximumMarkets:40` and authorization `human_moloch_breadth_20261016`; its rawSHA is `2022d5e50e5527e08bb202b151aa24e41ec33d2482522dc481ea32ad5f739c3a`. The supplied stopped report at247652 confirms `inheritedInventory.freshMarketLimit:12` and28 adopted exchange IDs. Original aggregate evidence is retained privately at `/tmp/moloch-paper6-capacity-audit-observation.json`. These are source observations reported by the execution owner, not an independently fetched account snapshot or a claim of current post-restart availability. The28 IDs are the retained adoption ancestry, not proof that28 still have nonzero inherited lots.

The native report's `liveMarketCapacity` and `ownerCapitalAllocation` records are the authoritative replayed values. Static Control/manifest policy remains the historical seed needed for exact Session reopening; the report records the subsequent durable events. The sidecar alone cannot amend bank policy.

## Exact limit trace

| Layer | Source and behavior |
|---|---|
| Initial inherited adoption |[engine_live.rs](../../moloch/native/src/engine_live.rs):97–143 preserves original fresh limit, adds the number of newly adopted nonzero exchanges to effective total `maximumMarkets`, debits their verified FIFO principal and keeps separate inherited attribution. Starting from3+28 gives31.|
| Existing live breadth event |[worker.rs](../../moloch/live-runtime/src/worker.rs):414–421 installs `live_market_capacity_v1` when all-markets mode is active and fresh limit is below12. [live_market_capacity.rs](../../moloch/native/src/live_market_capacity.rs):7–34 validates original scope/head/report/policy/control provenance and increases fresh3→12 and total31→40. No cash, quantity, per-market, window or write-limit change.|
| Ordinary planner occupancy |[engine.rs](../../moloch/native/src/engine.rs):1265–1309 counts distinct **exchange IDs** with nonempty wallet lots or any unretired order. New exchange admission requires total<effective maximum and non-inherited occupied exchanges<fresh limit. Both YES/NO drafts on one exchange use one slot, not two.|
| Remaining inherited exception |The fresh-count exception requires nonempty `inherited.remaining` lots on that exchange. Historical ancestry alone is insufficient. When those lots are fully consumed, a later position/draft on that exchange counts toward fresh12. The total40 ceiling remains fixed; liquidation does not create a permanent unlimited inherited exemption.|
| Reviewed fundamental entry |[reviewed_fundamental_entry.rs](../../moloch/native/src/reviewed_fundamental_entry.rs):51–60 uses the same cash/owned/reserved/per-market funding and effective total/fresh occupancy checks. Reviewed entries share the12 non-inherited slots with ordinary entries; they are not12 additional slots. Their approved principal/quantity/value/horizon bounds can be much smaller.|
| Read scheduling |[throughput_discovery.rs](../../moloch/live-runtime/src/throughput_discovery.rs):36–43 uses replayed total policy and `fresh_market_limit()`, not the original Control3. Selected reads/visible books are not extra exposure slots or permission to submit.|
| Physical dispatch |[authority.rs](../../moloch/live-runtime/src/authority.rs):77–89,159–189 binds the exact funded native quote, actual scoped positions/cash/open orders and original facts. It separately checks quantity<=10,000, per-market<=10,000, owned+reserved<=95,579.915, actual cash, source freshness, clock/key, price and any reviewed policy token. It cannot turn unused cash into a quote.|

For the observed adoption geometry, the current structural ceiling is **up to12 occupied non-inherited exchanges coexisting with the still-held inherited exchanges, within total40**. A flat never-sent draft can consume a slot while unretired; a resting physical order or unknown outcome cannot be discarded to make room. Existing source-bound never-sent retirement and actual terminal reconciliation release only the corresponding genuine liability. Scope/all-markets membership and a fresh book do not bypass economic or managed-entry eligibility.

## What the cash increase does—and does not—change

[engine_capital.rs](../../moloch/native/src/engine_capital.rs):48–92 and [lib.rs](../../moloch/native/src/lib.rs):319–339 apply a real attributed contribution to wallet cash/effective initial capital and maximum commitment. Original40,000 seed history is preserved. The same exact accounting identity continues, with inherited and new lots both charged in `ownedCost`; pending BUY reservations include their limit cost and applicable fees. No inherited holding becomes free purchasing power or new profit.

The ordinary planner's available new funding is:

```text
min(attributedCash − reservedCash,
    effectiveMaximumCommitted − ownedCost − reservedCash,
    perExchangeLimit − existingExchangeCost − pendingExchangeBuyCost)
```

At the supplied boundary, maximum commitment minus owned cost is72,848.400000, but actual attributed cash is lower at72,773.995000, and **both still require subtracting the actual reserved cash**. Reserved cash was not supplied, so spendable capital and current utilization cannot be inferred from these aggregates. Pledged/reserved capital is not the same as executed holdings.

Full usage is structurally possible under12×10,000=120,000 of per-exchange cost capacity, above95,579.915; therefore there is no universal30,000 ceiling caused by the historical3. That arithmetic is an upper bound, not evidence of enough profitable/liquid eligible opportunities. With only three genuinely eligible exchanges, per-exchange10,000 would of course permit at most30,000 **on those exchanges**, plus other attributed holdings/reservations; increasing funding does not generate additional eligible exchanges.

### Concrete sizing and turnover constraints

- Ordinary entries originate from `Target.quantity = policy.quantity` ([engine.rs](../../moloch/native/src/engine.rs):1186–1197), currently at most10,000 shares **per quote**, not10,000 cash. At.10,10,000 shares use1,000 principal; at.90 they use9,000. Cheap one-sided inventory can therefore remain quantity-limited well before reaching10,000 cash per exchange.
- For a new paired-spread quote, ordinary sizing ([engine.rs](../../moloch/native/src/engine.rs):1310–1380) conservatively funds both complementary BUY limits before assuming any netting. Initial pair unit cost is `YESbid + (1 − YESask) + fees`, and the common share size is clipped by actual capital/headroom and, when `entryDepthBps` is configured, each side's **top-level** visible depth. Hypothetical10,000-share pairs with.99 gross unit cost reserve9,900 per exchange; twelve such pairs would exceed the capital ceiling, so the cash ceiling would bind first. This is not an observed opportunity, guaranteed fill or profit claim.
- A visible deep level below a SELL limit cannot be counted at the higher limit price. Current ordinary top-depth limits and low-depth age exits can slow inventory recycling; increasing fresh slots or cash alone does not fix those independent constraints. See the separate [depth-sizing audit](../moloch-throughput-20261016/depth-sizing-audit.md).
- The ordinary entry branch additionally requires entry-window/allowlist eligibility, an unexcluded flat exchange, source continuity/current flow when configured, freshness, spread/edge and native pacing. Existing holdings enter the inventory-exit/hedge path rather than permitting unrestricted same-direction accumulation. Managed fundamental exchanges are excluded from ordinary flat entry ([worker.rs](../../moloch/live-runtime/src/worker.rs):397–406); a denied/expired reviewed forecast does not silently fall back to ordinary speculation.
- Fundamental reviewed principal/quantity and conservative value ceilings remain independently binding ([fundamental_policy.rs](../../moloch/live-runtime/src/fundamental_policy.rs):763–821,1216–1234). Account cash is not authority to increase an approved400-principal cohort or renew its horizon. Shared100-read/24-write admissions and actual demand/fill/terminal latency constrain turnover independently of funded capacity.

## A separate real three-market barrier: packages

The ordinary12 correction does **not** cover all strategies. Both native legacy and signed portfolio package reservation paths in [engine_packages.rs](../../moloch/native/src/engine_packages.rs):179–180,313 explicitly use `fresh_market_limit().min(3)` (spelled through the inherited field). [probabilistic_packages.rs](../../moloch/native/src/probabilistic_packages.rs):50 does the same. These guards count all occupied non-inherited exchanges, not just the new basket legs. Thus existing ordinary/fundamental occupancy beyond three can deny a new package even when no extra exchange would be introduced, and before the ordinary12 ceiling is reached. Runtime optimizers also retain their own package-size/new-market bounds. This is a source-level coexistence constraint, not proof it is currently blocking a qualified live candidate. No package policy was changed or deemed safe to enlarge by this audit.

## Smallest existing legitimate path

The existing no-reset path for ordinary breadth is exactly the already-installed, journaled `live_market_capacity_v1` event. It can increase a lower fresh limit to12 while keeping the historical seed/manifest and all money/quantity caps. Reopening then replays that genuine original event; no new identity migration is needed just to reinstall the same checked runtime logic. **It has already done its job here.**

There is no existing Control-only or owner-capital mandate knob for fresh breadth **above12**: native rejects `freshMarketLimit>12`, worker targets12, Activation has no market-count field, and the contribution/daily-mandate revision preserves per-market and maximum-quantity geometry ([engine_capital.rs](../../moloch/native/src/engine_capital.rs):35–39). Editing original `c.policy.maximumMarkets` changes the Session reopening identity and is not a valid migration. Expanding the12 ceiling or package `.min(3)` requires a separately authorized checked policy/source change with its genuine native identity bridge; do not smuggle it into the pending buffered-I/O rollout or reset the bank.

Before proposing such a change, root can inspect already-retained evidence: native `report.liveMarketCapacity`, `inheritedInventory.freshMarketLimit`, owner allocation, `ownedCost/reservedCash/cash`, exact unretired exchange occupancy, per-market owned+pending costs, funded target quantities versus top-depth/quantity clamps, `ordinary-entry-exclusions.json`, reviewed principal ceilings and actual rejection reasons. Distinguish insufficient eligible edge, physical/liquidity blockage and read latency from a true `MARKET_CAP` rejection. Current evidence supports **no correction to seed3** and no immediate risk-policy change.

# Entry and exit depth sizing audit

Source examined: checked revision `eac7db8c`, read-only. This report proposes future work; no policy, native source, runtime or production changes were made. The68-share entry and67-owned/one-share-best-bid example is parent-retained operational evidence; this task did not independently fetch production or account data.

## Confirmed source causes

`moloch/native/src/engine.rs:1339` applies `entryDepthBps` to the current **same-side top displayed level**: YES uses best YES bid quantity, NO uses best YES ask quantity (the latter is the complementary NO bid). In pairedSpread, the first side is also capped by the opposite target's top-level depth (`:1324`), and the mate inherits the batch quantity cap. Thus a68-share YES bid and1428-share YES ask can cap BOTH maker legs at68 when depthBps=10000, before cash/market/total/quantity caps. These levels are competition/displayed liquidity, not predicted demand for our new orders. The1428-share ask is not capacity to sell newly acquired YES at the bid price.

Inventory taker exit selection uses best executable price (`:902`) and tests age/profitability against that price (`:961`). Then `:974–1007` clips quantity to floor(best executable level size × inventoryExitDepthBps /10000). At50% of one share the cap is zero. The branch unconditionally continues even when it emits no target, suppressing the ordinary passive exit fallback for that evaluation. Large valid deeper depth is ignored.

For YES SELL, executable depth is YES bids in descending price order. For NO SELL, executable depth is the complement of YES asks: NO bid=1−YES ask, also traversed best to worse. Summing lower executable prices while retaining the top SELL limit would manufacture capacity.

## Smallest faithful future exit repair

Introduce an explicitly approved, bounded `inventoryExitMaximumSlippageTicks` policy, default0 to preserve existing policy behavior and historical replay. Keep maker entry sizing untouched. For each selected inventory side:

1. Preserve existing owned/pledged/opposite-BUY/policy quantity caps and freshness/age/authority gates.
2. Inspect only executable same-side levels no worse than best executable price minus reviewed ticks×TICK. Use checked integer arithmetic and actual returned book levels; unseen deeper levels are unavailable.
3. At each eligible prefix accumulate displayed size D. Candidate quantity is min(existing available n, floor(D×depthBps/10000)). Choose a nonzero feasible candidate; the order's limit is the actual marginal book price needed to execute that quantity, not a VWAP or the previous best price. Apply the participation haircut to the declared prefix capacity, not a fictitious quantity available at the best price. Prefer larger feasible quantity, breaking ties toward the higher actual marginal price.
4. Recompute the **profitable-age** test against the final marginal price with the existing worst-lot basis and both entry/exit fees. Do not let a profitable best-price trigger authorize unprofitable deeper shares. A maximum-age forced exit retains only its existing separate risk authority and the explicitly approved slippage bound; it is not labeled profit.
5. Before physical dispatch, validate frozen quantity/price/coverage against the fresh original book facts, full owned inventory and pending reservations. No new HTTP client or extra reads are needed. If depth changes, fail/defer using normal lifecycle; never mutate an attempted order or blindly replace unknown acceptance.

Example supplied by parent:67 owned YES, best.605×1 then.600×7348, depthBps5000. Current rule produces floor(1×.5)=0. With an explicitly permitted one-tick concession, cumulative D7349 permits floor(D×.5)=3674 and available quantity67; the actual67th share is at.600, so proposed quantity67/limit.600. Conservative minimum gross proceeds40.200, not40.535; worst limit-price concession versus selling every share at.605 is.335. Actual executable depth-weighted proceeds could be higher but cannot fund a worst-case guarantee at the old price. If basis+fees+minimumGain exceeds.600, profitable-age cannot use this candidate.

Equivalent NO example: YES asks.395×1 then.400×7348 imply NO bids.605×1 then.600×7348. The same67/limit.600 calculation applies. Do not use YES bids or complement quantities/prices twice.

The native `inventoryAON` annotation is a modeled execution style. Ordinary live requests are still individual limit orders and may partially fill/rest; this proposal does not make them venue atomic. Current ordinary physical authority checks covered SELL quantity (`moloch/live-runtime/src/authority.rs:175`) but does not itself promise all quantity immediately executable. Fresh full-depth validation is therefore necessary for the intended turnover policy, while receipt/fill/unknown reconciliation remains authoritative.

The existing reviewed inherited-exit primitive already traverses actual full depth and requires limit equal to the marginal selected price (`moloch/native/src/reviewed_inventory_exit.rs:28–31`). Its runtime-reviewed origin/value/coverage authority is separate; do not route ordinary exits through that event to bypass policy.

## Maker entry sizing is a separate economic decision

Increasing passive BUY size by replacing same-side bid68 with opposite ask1428 would confuse acquisition and liquidation. A resting maker order does not consume that ask; current ask depth is neither fill probability nor future executable exit depth. The paired minimum can also keep both legs at68 even after changing one side alone.

A later explicitly reviewed maker sizing policy could separate (a) full fee-inclusive funding/market/total/share caps, (b) per-market inventory risk and own-fill adverse-selection evidence, and (c) stressed executable liquidation depth at its **actual marginal lower price**. Any allowed liquidation concession must be charged to risk/edge, and both paired legs/cash reservations must stay consistent. Merely summing the entire bid book with no price boundary is not economically meaningful. Existing current-flow, expected-value and reviewed horizon gates cannot be replaced by volume.

## Required future regressions

- Best depth1 at50% gives0 with default0 slippage; explicit one-tick mode can reach67 at.600 with sufficient deeper depth.
- Insufficient deeper depth clips quantity; levels below the allowed price are excluded; limit equals the actual marginal selected level.
- Profitable top quote but unprofitable marginal quote yields no profitable-age taker candidate; fees can change that result.
- YES/NO complementary books produce identical economic quantities/proceeds, with no side inversion.
- Owned/pledged/opposite-buy/physical-pending/cash caps remain authoritative; stale/gap source and changed-depth physical facts defer without fake fills.
- Quantity and marginal limit remain frozen across original intent/ACK/fill/restart; no new exposure or replacement on unknown acceptance.
- Maker same-side68/opposite1428 and paired minimum behavior is unchanged unless a separate explicit maker policy is approved.

No implementation is proposed for immediate promotion in this audit. Root prioritized read nomination before a financial engine identity migration. Compilation/testing does not apply to this documentation-only change; source references and exact decimal examples were checked locally.

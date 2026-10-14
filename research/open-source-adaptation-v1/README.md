# Open-source strategy transfer: Baal research candidate

This audit followed every link in [`potentials.md`](../../potentials.md) to its actual source and license. It treats repository descriptions as claims, not measured performance. The only implementation added here is an opt-in, read-only probability-pricing diagnostic; nothing in the live or paper execution path imports it.

## What transfers

The most useful idea discovered through the linked directory is the Wang/probit transform used by [Oracle3](https://github.com/YichengYang-Ethan/oracle3-prediction-market-agent):

\[
q = \Phi(\Phi^{-1}(p) + \lambda)
\]

It can express a market-price benchmark under an explicitly supplied risk-premium sensitivity \(\lambda\). This code requires \(\lambda\) as an input and has no SIG default. It reports `marketMid - wangBenchmarkPrice` separately from the independent forecast’s expected gross payout edge at observed asks. Those values answer different questions. A benchmark residual is not an expected return; expected payout value is `independentProbability × shares − observed depth cost`, with the contest’s zero-fee assumption. A positive screen is still not a fill, a realized return, or an order instruction.

SIG’s parameter cannot be borrowed from other venues. The [working paper](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=6468338) reports different venue estimates and explicitly says a price/outcome wedge cannot by itself separate risk compensation from beliefs. Oracle3’s current [`fair_value.py`](https://github.com/YichengYang-Ethan/oracle3-prediction-market-agent/blob/main/oracle3/pricing/fair_value.py) describes a 13,274-contract Polymarket fit, while its [paper](https://github.com/YichengYang-Ethan/oracle3-prediction-market-agent/blob/main/paper/paper.md) and README describe much larger multi-venue samples. Those versions are not one reproducible estimate. The local [`data/markets.json`](../../data/markets.json) inventory has 237 open-market metadata rows and no settled labels or timestamped price history, so it cannot fit or validate a SIG-specific parameter. The transform is useful as a future, explicit sensitivity—not as a production value adjustment.

### Logical relationship constraints

Oracle3 also provides a reusable *relationship-checking* pattern: it evaluates implication, exclusivity, complements, same-event relationships, and event-sum constraints, then reports conditional net edge under the stated relationship and venue fee model. The relationship itself is an assumption that must be supported; arithmetic cannot prove that two contracts settle on the same predicate.

Baal already represents this idea at the evidence boundary. [`basket-relations.mjs`](../../live-paper/basket-relations.mjs) captures the documented official relationship graph and typed expressions (including implication and exclusivity) as evidence only. [`basket-observer.mjs`](../../live-paper/basket-observer.mjs) accepts a verified two-leg basket only when an active official mutually-exclusive relationship binds the exact contracts and cutoff; it separately encodes the ordinary-resolution payout floor and leaves refunds unverified. Neither component authorizes an order.

The available SIG market set has not yielded a tradable relation: [issue #26](https://github.com/GodlyDonuts/predictions-cup-2026/issues/26) found no multi-outcome/composite sets among 237 markets and no verified payout floors. The later [same-market complement holdout](https://github.com/GodlyDonuts/predictions-cup-2026/issues/79) found zero positive same-time pairs across 66,127 accepted points and zero profitable completed pairs in all 32 tested scenarios; the earlier [exact complement replay](https://github.com/GodlyDonuts/predictions-cup-2026/issues/22) also found none. Thus the relationship model is represented in Baal's evidence and proof gates, but it has not produced an executable SIG edge. A future candidate must start from an authoritative relation proof and show positive depth-aware net value on subsequent books; title similarity or a mathematical constraint alone is insufficient.

## Audit of the five links

| Project | What the actual source shows | License observed | Baal decision |
|---|---|---|---|
| [Harrier toolkits](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits) | [Market maker](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits/blob/main/src/bot/market_maker.rs), [cross-market arb](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits/blob/main/src/bot/cross_market_arb.rs), and [imbalance](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits/blob/main/src/bot/orderbook_imbalance.rs) entry points log “in development” and return; README strategy counts do not demonstrate running strategies. | MIT | Do not import or treat listed strategies as alpha. Only the general idea of inventory-aware quotes is useful, and Baal already has separate quote/economic experiments. |
| [Kalshi Quant TeleBot](https://github.com/yllvar/Kalshi-Quant-TeleBot) | Its [cointegration analyzer](https://github.com/yllvar/Kalshi-Quant-TeleBot/blob/main/src/arbitrage_analyzer.py) performs tests, but its later independently standardized-series difference is not a fitted cointegrating hedge residual. Its [GARCH](https://github.com/yllvar/Kalshi-Quant-TeleBot/blob/main/src/volatility_analyzer.py) and [text-sentiment](https://github.com/yllvar/Kalshi-Quant-TeleBot/blob/main/src/news_analyzer.py) helpers do not establish event-outcome calibration or executable returns. | README claims MIT, but the root `LICENSE` fetch returned 404 | No source copied. Verify license terms before reuse; do not treat advertised strategies as proven alpha. |
| [Awesome Prediction Market Tools](https://github.com/aarora4/Awesome-Prediction-Market-Tools) | A directory of links, not a trading implementation. It led to Oracle3, the most substantive probability-pricing source in this review. | No license file found at the checked path | Use it for discovery only; evaluate each linked project independently. |
| [Polymarket Prediction Bot](https://github.com/semrincaballero/Polymarket-Prediction-Bot) | Its [strategy engine](https://github.com/semrincaballero/Polymarket-Prediction-Bot/blob/main/polymarket_bot/core/strategy.py) compares a supplied fair probability to midpoint; imbalance is a threshold heuristic; its liquidity branch picks YES on wide spread without an inventory-based selection. None establishes executable edge or profitable fills. | MIT | Keep only the explicit-forecast-vs-price separation. Current main contains a native [Rust settlement-value candidate](../../backend-rust/portfolio-engine/src/settlement_value.rs) that compares supplied probability scenarios against executable price, spread, depth, slippage, and a minimum edge, then applies fractional-Kelly and event-group limits. It is a diagnostic/candidate path, not proven profitable execution; this demo heuristic adds no validated signal. |
| [PredictOS](https://github.com/PredictionXBT/PredictOS) | Its pinned [Edge Function](https://github.com/PredictionXBT/PredictOS/blob/34c7de8b63c9988dce43a11cff672d7ea94300b8/supabase/functions/polymarket-up-down-15-markets-limit-order-bot/index.ts) submits a straddle on the next 15-minute BTC/SOL/ETH/XRP market (or a price ladder); its [documentation](https://github.com/PredictionXBT/PredictOS/blob/34c7de8b63c9988dce43a11cff672d7ea94300b8/docs/features/betting-bots.md) calls 48¢+48¢ profitable only after both fills. This is a short-horizon crypto order-placement template, not evidence of matched fills or an arb in long-dated election books. | MIT | Do not adopt the advertised return. Baal’s separate paired-contract tests already found no validated profitable completed pair on available evidence. |

Oracle3’s agent source is Apache-2.0. Its companion [paper/replication repository](https://github.com/YichengYang-Ethan/prediction-market-pricing) is MIT. The implementation here is a clean mathematical implementation and does not copy either source. The Awesome directory has no license file at the checked root, so this change treats it as a discovery index and reproduces none of its contents. The Kalshi README says MIT but its root license file was absent at inspection; no code from it is reused.

## Reproduction and limits

Run:

```powershell
node --test research/open-source-adaptation-v1/wang-value.test.mjs
node research/open-source-adaptation-v1/wang-value.mjs --inventory
```

The inventory command reads only the local `data/markets.json` metadata and prints its SHA-256, status counts, settlement-label count, and whether timestamped price-history rows exist. It uses no API, account, runtime, wallet, or order path. The helper `evaluateBinaryValue` requires a separately supplied independent physical forecast and timestamped ask depth. Quote age is measured at the decision timestamp; missing depth, insufficient size, stale books, and future-dated receipts fail closed. It records neither a fill nor an order.

The code is not connected to Baal’s active executor. Use it only when a future data set supplies, for each contract, a genuinely independent forecast, the market snapshot available at that time, the actual contract settlement label, and a frozen chronological holdout. Until then, this is a tested calculation and a falsifiable research hypothesis—not evidence of profitability.



## Rechecked against current Baal main

The earlier reference to `execution/adaptive-value-policy-v2.mjs` was stale: that file is absent from current `main`. The relevant current-main modules are:

- **Probability edge:** `backend-rust/portfolio-engine/src/settlement_value.rs` prices supplied research scenarios against executable price and applies spread, slippage, depth, minimum-edge, fractional-Kelly, and event-group constraints. The supplied ranges are not guaranteed confidence bounds, and this module does not prove profitable live execution.
- **Shared portfolio capital:** `execution/shared-value-allocation-v1.mjs` ranks candidates that already passed forecast, risk, and depth checks by net edge per principal. It prevents the same cash from being allocated twice; it is an allocator, not an alpha source.
- **Inventory-aware market making:** `research/binary-inventory-maker-v1.mjs` and `research/binary-market-maker-hjb-v1.mjs` require validated own-fill-receipt and chronological transition models with at least 32 independent clusters. Without those calibrations, they remain research-only.
- **Conditional pair entry:** `execution/baal-scaled-entry-signal-v3.mjs` evaluates a same-race Party Winner NO/NO package gap. It explicitly has no fair-probability estimate or reversion promise, reserves for the directional first leg, and does not authorize an automatic hedge.

The current coordination index [issue #10](https://github.com/GodlyDonuts/predictions-cup-2026/issues/10) reports the execution stop and stopped paper collector. This audit did not change or restart any runtime. The linked bots provide no independently verified SIG-market edge to import. The reusable ideas above are represented as bounded candidates; imbalance, naive statistical arbitrage, and unpaired straddles remain rejected or unvalidated.

# Open-source bot transfer audit — 2026-10-14

## Decision

The five direct links do not provide a defensible profitable strategy to import. The Awesome directory led to Oracle3, whose Wang/probit probability-pricing method is the most substantive quantitative idea discovered here. Baal already has a clean-room, read-only implementation from completed issue #98; this audit should surface that transfer rather than claim it as new work. The method is a pricing sensitivity, not validated SIG alpha.

This follow-up corrects the index and consolidates source evidence. It adds no new strategy, data collection, service, account client, order or runtime change.

## Most useful transfer already present: Wang probability pricing

Oracle3 maps an independently estimated physical event probability (p) to a risk-adjusted price benchmark (q = \\Phi(\\Phi^{-1}(p)+\\lambda)). Its project describes calibration on resolved contracts from several other venues; that estimate is not a SIG estimate. See the [Oracle3 project](https://github.com/YichengYang-Ethan/oracle3-prediction-market-agent) and its [working paper](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=6468338). The paper's interpretation is a selected pricing measure in incomplete markets, so the price/outcome wedge alone does not prove exploitable mispricing.

Baal already reproduces the transform in `research/open-source-adaptation-v1/wang-value.mjs`. It requires an explicit `lambda`, reports the benchmark residual separately from independent-forecast expected value at observed asks, and fails closed on stale or insufficient depth. It remains read-only: no order or fill is produced.

The local inventory check reports 237 open metadata rows, zero settlement labels, and no timestamped price history. That cannot estimate a SIG-specific `lambda`; importing Oracle3's cross-venue coefficient would be an unvalidated assumption. The eight focused local tests pass, but they verify the math and gates only, not profitability. This is the genuine algorithmic transfer from the links, completed earlier in issue #98.

## Source-by-source findings

| Link | What the source actually contains | Baal decision |
|---|---|---|
| [HarrierOnChain / Prediction-Markets-Trading-Bot-Toolkits](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits) | MIT. Its current README says copy trading is the one production-ready strategy and the other nine are in development. The checked market_maker.rs, spread_farming.rs, orderbook_imbalance.rs, cross_market_arb.rs, and resolution_sniper.rs entry points only log “in development” and return. The copy-trading engine is tied to Polygon CTF logs and Polymarket EIP-712 orders. Its shared risk/execution separation is a useful architecture pattern, already present in Baal. | No strategy or Rust execution code to import. Keep Baal’s shared pipeline and existing risk gates. |
| [yllvar / Kalshi-Quant-TeleBot](https://github.com/yllvar/Kalshi-Quant-TeleBot) | No LICENSE file was present on the checked default branch, so no code is copied. The README’s strategy claims exceed what its code proves. In src/arbitrage_analyzer.py, each series is standardized independently over the whole supplied history before differencing; that leaks later observations and does not estimate a tradable hedge ratio. Its RiskManager.calculate_position_size_kelly omits the losing-probability term from Kelly’s formula, returns a positive minimum size for invalid probabilities, and does not use the actual contract price. calculate_portfolio_metrics() substitutes five hard-coded sample returns when none are passed, and get_portfolio_status() calls it with no data. | Reject its cointegration trigger and sizing/metrics. A correctly specified, causal relative-value test would be a new research strategy, not an import of this implementation. |
| [aarora4 / Awesome-Prediction-Market-Tools](https://github.com/aarora4/Awesome-Prediction-Market-Tools) | A curated directory, not a bot or tested strategy. No LICENSE file was present on the checked default branch. | Use only as a discovery index. It contributes no executable algorithm or profitability evidence. |
| [semrincaballero / Polymarket-Prediction-Bot](https://github.com/semrincaballero/Polymarket-Prediction-Bot) | MIT. StrategyEngine is a deterministic demo heuristic: probability edge compares a supplied fair value with the book midpoint rather than executable prices; imbalance emits direction without an expected-return/cost test; the liquidity signal always chooses YES. The checked code includes a bundled mock-data path; it does not provide source evidence that these heuristics are a profitable live strategy. | The pure signal/UI separation is sound, but Baal already has strategy, paper and execution separation. The underlying signals are not new and already face Baal-specific tests below. |
| [PredictionXBT / PredictOS](https://github.com/PredictionXBT/PredictOS) | MIT. The “arbitrage intelligence” flow searches by AI-generated short text and analyzes title/price data; that does not prove identical resolution conditions. Its 15-minute straddle guide calls 48¢ YES + 48¢ NO a 4¢ arb only if both legs fill; it does not remove one-leg exposure, capital lock or latency risk. The code targets short-window crypto markets, not the Cup’s election contracts. | Keep slow research and execution mapping separate. Do not import title-matched cross-venue trades or fixed two-leg quotes as risk-free arbitrage. |

## Baal evidence that decides the transfers

- [#18](https://github.com/GodlyDonuts/predictions-cup-2026/issues/18): microprice/imbalance forecast moves were about 0.001–0.009 ticks against roughly 1.21 ticks of crossed-book cost; the taker version was rejected.
- [#22](https://github.com/GodlyDonuts/predictions-cup-2026/issues/22): same-market complement scan found 0 positive pairs among 417,815 observations; all 32 chronological latency/side holdout cells had zero profitable completed pairs.
- [#26](https://github.com/GodlyDonuts/predictions-cup-2026/issues/26): no multi-outcome/composite sets or verified payout-floor proofs in the captured market set.
- [#29](https://github.com/GodlyDonuts/predictions-cup-2026/issues/29): all 12 fixed trend/reversion holdout cells lost after spread.
- [#30](https://github.com/GodlyDonuts/predictions-cup-2026/issues/30): the external-venue comparison had delayed later books and unresolved contract equivalence, so it could not support cross-venue markout.
- [#89](https://github.com/GodlyDonuts/predictions-cup-2026/issues/89) and [#91](https://github.com/GodlyDonuts/predictions-cup-2026/issues/91): passive market making is the most structurally relevant idea, but public books do not reveal our queue position or aggressor side. A paper touch is not an attributable fill; the fixed candidate needs actual fill receipts to establish economics.
- [#103](https://github.com/GodlyDonuts/predictions-cup-2026/issues/103): the later fixed queue-depletion replay produced 43 entry proxies, 0 completed exits, 0 valued open positions and 0 realized P&L. It is not profitability evidence.

## Integration result

The ideas that survive are architectural: a pure strategy proposal, a single shared execution path, inventory-aware risk checks, and a paper mode that models the complete order lifecycle. Baal already has these. The new evidence does not justify turning on a source-derived signal or changing production. Reconsider a strategy only with exact contract semantics, causal features, later executable prices, full depth and funding, realistic latency, and attributable entry and exit fills. The current execution stop remains untouched.

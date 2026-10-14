# Open-source quant ideas for Baal

https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits  
https://github.com/yllvar/Kalshi-Quant-TeleBot  
https://github.com/aarora4/Awesome-Prediction-Market-Tools  
https://github.com/semrincaballero/Polymarket-Prediction-Bot  
https://github.com/PredictionXBT/PredictOS

## What is worth carrying over

The strongest quantitative idea found through these links is Oracle3's Wang/probit probability-pricing transform, discovered via the Awesome directory. It is already implemented in Baal as a research-only diagnostic in [research/open-source-adaptation-v1/README.md](research/open-source-adaptation-v1/README.md) and [wang-value.mjs](research/open-source-adaptation-v1/wang-value.mjs). The transform maps an independent physical probability (p) to a price benchmark (q = `Phi(Phi^-1(p) + lambda)`. Baal requires an explicit `lambda`; it has no imported venue default, emits no order, and keeps the transformed market-price residual separate from expected payout value at executable ask depth.

Baal also exposes Oracle3's Table 3 feature-conditioned and Table 5 lifecycle-conditioned `lambda` estimates through a separate opt-in research diagnostic; they are cross-venue Polymarket sensitivities, not SIG parameters or trade signals.\n\nThat separation is useful: a market-price distortion is not automatically a forecast edge. Oracle3's reported calibration comes from other venues and contract populations. Baal's available local inventory has 237 open market metadata rows, no settled labels, and no timestamped price history, so there is not enough SIG-specific evidence to fit or validate `lambda`. We should not turn a borrowed coefficient into a buy signal.

| Link | Baal transfer |
|---|---|
| [HarrierOnChain toolkits](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits) | Its listed maker, imbalance, and cross-venue strategy modules are stubs; copy trading is venue-specific. Baal already separates signal, risk, and execution. |
| [Kalshi-Quant-TeleBot](https://github.com/yllvar/Kalshi-Quant-TeleBot) | No code reused: no root license was present in the checked branch; the inspected spread normalization leaks later data, Kelly sizing is incorrect, and empty metrics use hard-coded returns. |
| [Awesome Prediction Market Tools](https://github.com/aarora4/Awesome-Prediction-Market-Tools) | Discovery index only. It led to Oracle3's Wang pricing method, already represented in Baal as the read-only diagnostic above. |
| [Polymarket Prediction Bot](https://github.com/semrincaballero/Polymarket-Prediction-Bot) | Demo probability, imbalance, and liquidity heuristics add no validated edge over Baal's existing tested candidates. |
| [PredictOS](https://github.com/PredictionXBT/PredictOS) | Its two-leg crypto examples assume both legs fill; text-matched contracts do not prove equivalent settlement. Baal's pair tests have not established a profitable completed pair. |

## Result

These projects yielded useful architecture and a probability-pricing research diagnostic, but no source-backed SIG-market strategy that has demonstrated net profit. Baal already has the substantive transferable method; the missing piece is a contest-specific independent forecast and enough matched, settled and executable data to validate it. The detailed source audit and rejection evidence are in [docs/OPEN_SOURCE_BOT_TRANSFER_20261014.md](docs/OPEN_SOURCE_BOT_TRANSFER_20261014.md).

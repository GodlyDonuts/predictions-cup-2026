https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits
https://github.com/yllvar/Kalshi-Quant-TeleBot
https://github.com/aarora4/Awesome-Prediction-Market-Tools
https://github.com/semrincaballero/Polymarket-Prediction-Bot
https://github.com/PredictionXBT/PredictOS


## Audited transfer status — 2026-10-14

These links were checked against their source trees and Baal's existing tests. No profitable strategy was copied or promoted; the closest methods already have negative or unscorable Cup-specific evidence. Baal keeps the transferable architecture patterns (separate signal, risk and execution layers) already in place.

| Project | Baal transfer |
|---|---|
| [HarrierOnChain toolkits](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits) | Only copy trading is implemented; the maker, imbalance and cross-venue entries are stubs. Shared risk/execution architecture already matches Baal. |
| [Kalshi-Quant-TeleBot](https://github.com/yllvar/Kalshi-Quant-TeleBot) | No code reused: no root license, non-causal spread normalization, invalid Kelly sizing, fabricated default metrics. |
| [Awesome Prediction Market Tools](https://github.com/aarora4/Awesome-Prediction-Market-Tools) | Directory only; discovery reference, no strategy implementation. |
| [Polymarket Prediction Bot](https://github.com/semrincaballero/Polymarket-Prediction-Bot) | Demo heuristics; no new edge over Baal's already-tested probability, book-pressure and spread candidates. |
| [PredictOS](https://github.com/PredictionXBT/PredictOS) | Research/execution separation is useful; fixed paired limits and title-matched cross-venue “arb” do not prove fills or equivalent payouts. |

See the [full source-by-source transfer audit](docs/OPEN_SOURCE_BOT_TRANSFER_20261014.md), including the Baal holdout results and exact no-go reasons.
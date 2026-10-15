# External strategy review — October 14, 2026

Source list: `origin/main:potentials.md` at f21c4ac7. Sources were cloned for inspection only; no dependencies, binaries or external credential clients were executed.

| Repository and pinned commit | Actual implementation inspected | SIG conclusion |
| --- | --- | --- |
| HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits b8a31fbedfa03554475e794691f72dea53f91dd9 | `src/bot/market_maker.rs`, `src/bot/arbitrage.rs`: functions log “in development” and return success. | These functions supply no implemented quoting or arbitrage signal. Copy-trading infrastructure is a different strategy. |
| yllvar/Kalshi-Quant-TeleBot 148ca12f1eb550c16bc1362bf8fd8cbf3a8287bc | `src/arbitrage_analyzer.py`: cointegration, standardized spreads, z-scores and stationarity. | Research candidate requiring causally synchronized history and compatible settlement rules; statistical correlation alone does not fund a guaranteed arbitrage. No root license file found; code was not copied. |
| aarora4/Awesome-Prediction-Market-Tools e900f5d576c9341b144b9f3cade5f0871be79471 | Curated resource directory. | Useful discovery list, not a strategy implementation. |
| semrincaballero/Polymarket-Prediction-Bot d0ee35aa333113d6ea6c82ef460d4ee6b8526fd4 | `polymarket_bot/core/strategy.py`, `models.py`: 30% depth imbalance; four-cent spread gate; liquidity signal always chooses YES with an inventory TODO. MIT. | Independently implemented an optional signed full-depth 30% entry filter in Moloch, with existing covered exits and funding. Its directional mode is incompatible with paired entry. Tested the spread threshold separately on paired quotes. |
| PredictionXBT/PredictOS 34c7de8b63c9988dce43a11cff672d7ea94300b8 | `supabase/functions/arbitrage-finder/index.ts`: LLM-assisted cross-platform matching and analysis. | Useful research/matching workflow; externally quoted prices cannot be traded inside SIG as a guaranteed paired payout. |

## Measured strategy diagnostic

`node moloch/research/external-strategies-20261014.mjs` checks original snapshot hashes and journal chains for episodes 18–25, carries each wallet continuously and verifies the recorded baseline exactly. Results: `data/moloch-external-strategies-20261014.json`.

All variants inherit an earlier +0.025 realized gain. That gain is not credited to the new strategies. The baseline has five new fills and last fresh liquidation P&L −0.050. The 30% imbalance variant has one new fill and fresh terminal liquidation P&L −0.025 (a −0.050 change from its initial bank). The four-cent spread gate and combined gate each have zero new fills, remain flat, and retain the inherited +0.025. These gates establish no new profitable trading edge. Historical periods were previously inspected, are not pristine holdouts, and hypothetical trades do not alter the captured market. No external strategy was activated in the live wallet.

## Competition-scale paper benchmark

Original 1,000-SUSQie wallet and its losses remain immutable history. A separate policy can start at 100,000 with 40,000 maximum commitment, 10,000 per market and up to 10,000 shares per order. Optional `entryDepthBps` bounds both initial paired legs by the smaller observed top queue. Exact integer unit-cost division replaces per-share decrement sizing and supports the official quantity ceiling without a large loop. Paper fill participation, source clocks, fees, write pacing and depth-constrained inventory exits still apply. Large displayed orders do not imply filled capital or measured market impact.

`run.mjs --format journal --follow` verifies the existing collector's source chain and accepts only receipts after the new benchmark starts. It performs no network requests, authentication or actual orders. `live.mjs --rest-recovery` explicitly allows four existing scopes without realtime authentication; it retains the same serialized read limiter, original recovery windows and trade identity reconciliation. It cannot be combined with realtime/global observation mode. This is an explicit operator choice, not automatic reconnect or token renewal.

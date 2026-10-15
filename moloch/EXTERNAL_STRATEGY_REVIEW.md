# External strategy review for Moloch

Reviewed the links in `potentials.md` on remote main commit
`606938de56f05667f8593015a779b85bfbba611d`. This is an independent
Moloch assessment; Baal's source audit is background, not Moloch performance.

| Source and inspected revision | Finding | Moloch decision |
| --- | --- | --- |
| [Harrier toolkits](https://github.com/HarrierOnChain/Prediction-Markets-Trading-Bot-Toolkits/tree/b8a31fbedfa03554475e794691f72dea53f91dd9) | `src/bot/market_maker.rs` logs that it is in development and returns. | No functioning maker algorithm to import. |
| [Kalshi TeleBot](https://github.com/yllvar/Kalshi-Quant-TeleBot/tree/148ca12f1eb550c16bc1362bf8fd8cbf3a8287bc) | The Kelly routine uses `p*b/(b+1)` and forces positive allocation even for invalid probabilities. | Reject sizing. No code copied. |
| [Awesome tools](https://github.com/aarora4/Awesome-Prediction-Market-Tools/tree/e900f5d576c9341b144b9f3cade5f0871be79471) | Discovery directory, not an executable strategy. | Keep as a research index. |
| [Polymarket demo](https://github.com/semrincaballero/Polymarket-Prediction-Bot/tree/d0ee35aa333113d6ea6c82ef460d4ee6b8526fd4) | Default imbalance threshold is 30%; liquidity threshold is four cents; liquidity always chooses YES. | Test the ideas with Moloch's funded lifecycle, rather than copying the signal as an order. |
| [PredictOS](https://github.com/PredictionXBT/PredictOS/tree/34c7de8b63c9988dce43a11cff672d7ea94300b8) | Short-window crypto and two-leg strategies depend on both fills and applicable settlement semantics. | Not transferable as guaranteed Cup arbitrage. |

## Existing independent replay evidence

`research/external-strategies-20261014.mjs` already tests clean-room spread
and imbalance variants over eight original Moloch episodes (18–25).
The journals and snapshot parents are verified, each variant carries its own
cash and inventory across all episodes, and the recorded-policy replay must
reproduce the original financial states. No account orders are submitted.

| Variant | New modeled fills | Added realized profit | End owned cost | End unresolved orders | Last fresh liquidation P&L |
| --- | ---: | ---: | ---: | ---: | ---: |
| Four-cent minimum spread | 0 | 0 | 0 | 0 | +0.025000 |
| 30% imbalance | 1 | 0 | 1.600000 | 5 | −0.025000 |
| Combined spread and imbalance | 0 | 0 | 0 | 0 | +0.025000 |

Every variant inherits +0.025000 realized P&L from the parent. That amount
is not profit earned by these imported ideas. The liquidation marks include
the inherited history and are timestamped estimates, not terminal cash or
attributable venue fills. These previously inspected periods are retrospective
diagnostics, not untouched holdouts. Results live in the ignored local file
`runtime/moloch/external-strategies-20261014.json`.

The Wang probability transform surfaced by the directory is already a Baal
research diagnostic. It needs an independent event forecast and a calibrated
pricing parameter; transforming a market price does not create a forecast.
It does not address Moloch's immediate maker-fill and inventory-exit bottleneck.

## Next research criterion

Test quote placement on future captured observations with a preregistered
entry cutoff, a funded unchanged wallet, paced cancellation and covered exits.
Report strict-through and conditional queue assumptions separately. Evaluate
closed-cycle returns and adverse inventory selection; trade count alone cannot
establish an edge. The current two-sided-flow selection and queue comparison
remain candidates, not profitable strategies. No external project is activated
and no foreign bot receives credentials.

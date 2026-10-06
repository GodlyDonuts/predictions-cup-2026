# Causal trend-filter comparison

The fixed one-tick dip control and two trend filters replayed 91.08 hours / 35,245 captured books in 29.48 seconds. Both filters reject new entries when the recent upper median is at least one tick below the earlier median; their fixed splits are 30/30 and 15/45 minutes, with five observations required per segment. Shared funding, delayed receipts, zero fees, depth, pending limits and owned exits remain unchanged.

| Development case | Closed trades | Losing closes | Realized SUSQie P&L | Open attributed cost |
| --- | ---: | ---: | ---: | ---: |
| Original one-tick control | 8 | 6 | -0.065 | 0.945 |
| 30/30 trend filter | 4 | 3 | -0.030 | 1.270 |
| 15/45 trend filter | 5 | 3 | -0.005 | 2.130 |

This is development evidence, not a selected profitable strategy. Remaining positions lack fresh final executable liquidation depth, so these are closed-trade totals, not complete wallet returns. Small samples, sparse historical book coverage and different open inventory prevent ranking by realized P&L alone. Candidate counts include repeated rejected observations and must not be treated as independent trades.

The independent future cutoff was registered at **2026-10-06T07:08:27.849Z** before candidate results. A subsequent replay starts all three wallets fresh at 100, primes prior history without importing fills, and admits only later requests/availability. Its current short window has one open 0.290 position and zero closes in each correlated case. It supports no profitability or promotion claim. Future runs must extend the same cutoff and fixed policies rather than reset capital or tune thresholds after seeing results.

Independent review caught a forward-only account cursor that could admit account information from after an earlier cross-market request. The original run is preserved as invalid; the corrected per-book causal lookup and its rollback regression pass 13 checks. The corrected development rerun above uses identical sealed inputs. Evidence, source hashes, binary identity and subsequent results are in [the result record](../data/median-trend-results-20261006-v2.json).

The official pilot keeps its existing policy. Discovery v3 fixes a separately observed scheduling defect: official valid proposals and owned inventory now precede paper watches within the same four protected books and two auxiliary slots. Actual account exclusions cannot make a successfully captured protected book disappear from that budget, and they cannot become legacy paper entries. The integration checks pass 107/107.

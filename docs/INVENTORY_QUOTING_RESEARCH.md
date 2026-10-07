# Inventory-aware quoting and actual entry economics — October7

The current production signal detects a conditional ordinary-resolution gap between two opposing-party NO contracts. It buys only the cheaper leg. That gap is a property of a funded two-leg package; it is not a calibrated forecast for the bought leg or a promise of short-term price recovery. The current production policy remains unchanged while this research proceeds.

The new [actual-entry diagnostic](../data/official-entry-horizons-20261007.json) replays the original semantically validated public-book chain against14 verified purchases. It processed51,006 records in20.09seconds, with no API requests or orders. Each horizon uses the first later-request depth snapshot within a declared five-minute tolerance, values the entire purchased quantity where observed depth permits, and separates exact-depth values from one-tick stress. Unavailable observations are not zeros.

| Horizon | Complete observed entries | Negative stressed snapshot values | Unavailable entries |
|---|---:|---:|---:|
| 5minutes | 8 | 8 | 6 |
| 15minutes | 7 | 7 | 7 |
| 30minutes | 4 | 4 | 10 |
| 60minutes | 3 | 3 | 11 |
| 120minutes | 1 | 1 | 13 |

Coverage is biased toward the newer, actively captured holdings: five older purchases have no qualifying observations at these horizons. These are values from snapshots at different times, not actual sales or a funded portfolio return. Development captures have been seen before. The results do not disprove the prior closed+.780cohort, establish a confidence interval, or prove that any alternative is profitable. They do show why increasing taker turnover without a better signal can repeatedly pay spread and exit stress.

The new [inventory quote planner](../paper/inventory-aware-quote-v1.mjs) uses a causal book microprice, a declared variance rate and holding horizon, and normalized signed inventory to shift its reservation price. Its heuristic is `reservation = microprice - normalized_inventory * risk_aversion * variance_rate * horizon`. Quote width includes spread and variance risk, prices are rounded onto official ticks, buy quotes are fully funded together, and sells use owned shares. Opposite-side acquisitions and direct sales reserve disjoint old inventory so both fills cannot consume the same lot. New exposure per side is bounded.

This is a research adaptation of the inventory-risk idea in [Avellaneda and Stoikov's primary paper](https://math.nyu.edu/inmemoriam/avellaneda/HighFrequencyTrading.pdf). [Hummingbot's official implementation documentation](https://hummingbot.org/strategies/v1-strategies/avellaneda-market-making/) describes inventory-dependent reservation prices and empirical liquidity/intensity estimation. Our planner has no calibrated arrival model or queue-position estimate, and must not inherit either project's profitability assumptions. Inventory reduction may realize a loss; the planner does not manufacture a profit floor.

Four correctness checks cover later-request horizons, unavailable depth, inventory skew, non-overlapping share reservations, funding and invalid inputs. No production model bank was overwritten, no accounting reset, no new collector or account client was created, and no quoting policy was activated. Next: estimate variance and fill-conditioned returns causally, then run separately named chronological/prospective wallets with strict partial-fill, queue, cancellation and adverse-selection accounting before choosing any production policy.

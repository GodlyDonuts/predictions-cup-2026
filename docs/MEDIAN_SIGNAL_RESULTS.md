# Entry selection evidence

The broad causal price-path diagnostic covers1,477 median-dip episodes:516 target touches,208 stop touches,103 horizons,646 coverage gaps and4 unfinished. These are quoted prices, not fills or funded returns. Even optimistic observed quote differences across priced paths sum negative. Upward pre-entry trends improve the target-touch frequency, but their quoted totals also remain negative. More volume alone therefore has no demonstrated economic advantage.

A separately fixed three-arm funded replay keeps native and capped controls and adds one new filter: recent30-minute upper median at least one tick above the preceding30-minute median, with five causal observations per segment. Threshold, target, stop, quantity, funding, depth, latency and fees remain unchanged. Existing intents/exits are unaffected.

On the original sealed91.08-hour development window, both controls realize -0.065 and hold three unpriced positions. Strict uptrend plus cap realizes +0.030 from **one** closed trade and ends flat. This is a promising development observation, not repeatable profitability evidence. The future cutoff08:59:42.428UTC was registered before results; the initial subsequent window has zero proposals/trades in all arms. The filter remains paper-only and was not applied to the live pilot.

30 checks pass. The new crate preserves the original frozen kernel and validated reader. See [funded comparison](../data/median-uptrend-cap-results-20261006-v1.json) and [bulk diagnosis](../data/median-barrier-diagnostic-20261006.json). Correlated cases and quoted returns must not be added to official results.

# Stream-native dip engine

`stream-dip.mjs` is a separate paper engine for the verified stream gateway. It does not submit official orders and does not modify existing frozen experiments. Its two independent wallets use 1-second and 3-second decision latency, zero fees, a fixed 100-SUSQ cumulative purchase budget, at most three markets and 25 SUSQ per market. An admitted order expires after 30 seconds.

Historical references use the first accepted book of each receipt minute, available before the decision. At least eight minutes spanning 30 minutes are required. A broadcast burst does not increase the reference's weight. References are historical midpoint medians, not guaranteed fair values. The native dip gates are preserved; a conditional order may use a limit one tick below the observed ask when that candidate passes the same cost-adjusted gates.

Decisions use the paper consumer's actual availability timestamp. Recovery requests retain their true nanosecond timestamps; pushed books have no HTTP request timestamp. A fill opportunity requires a later advanced engine version, a receipt after the wallet's latency, continuous verified input and confirmation before the order expiry. Only one percent of qualifying displayed depth is available. Execution prices include an adverse tick capped at the frozen limit. These are counterfactual executable opportunities, not queue priority or proof of an actual order fill.

All received evidence in a batch resolves prior orders before new decisions or expiry processing. Late consumption therefore preserves an adverse pre-expiry fill, while preventing a backdated fresh decision. Invalidation cancels pending paper intents and retains inventory and losses. Partial exits release exact proportional cost; only a complete exit increments completed round trips. Missing observation windows are distinguished from observed expiry without full execution. Missing or stale liquidation depth makes total marked P&L unavailable rather than hiding held inventory.

Run correctness checks:

```powershell
node --test live-paper/stream-dip.test.mjs live-paper/stream-dip-replay.test.mjs
```

Run a read-only development replay against the existing restricted capture:

```powershell
node live-paper/stream-dip-replay.mjs --input stream-books-20261002 --output data/stream-dip-development-replay.json
```

The replay verifies the frozen historical source prefix, every gateway checkpoint chain, captured source-line hash, derived events and input state before producing its development report. It makes no API requests or runtime writes. It uses recorded gateway availability as a stand-in consumer clock and is explicitly retrospective. Its results are not evidence of prospective profitability. Live deployment still requires a durable consumer host that freezes its own future cutoff, journals actual consumption clocks, replays wallet accounting exactly, respects STOP and source-session rotation, and preserves these wallets across interruptions. This engine is not currently attached to a running paper worker or the official pilot.

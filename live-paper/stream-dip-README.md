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

The replay verifies the frozen historical source prefix, every gateway checkpoint chain, captured source-line hash, derived events and input state before producing its development report. It makes no API requests or runtime writes. It uses recorded gateway availability as a stand-in consumer clock and is explicitly retrospective. Its results are not evidence of prospective profitability.

## Durable prospective worker

`stream-dip-watch.mjs` reads the single existing gateway and raw capture. It verifies raw capture against each gateway checkpoint before consuming public events. Its new cohort freezes the capture prefix, source bank, minute-history warmup, code/import identity and its own future cutoff. All earlier books warm history without trades. Each batch records the paper consumer's actual availability, upstream verified observation watermark, decision-enabled state and resulting wallet hash. The journal is fsynced before wallet promotion. Restart rebuilds the history, validates source lineage and replays every saved batch with its original consumer timestamps. There is no network client or official order transport.

Expiry waits for the verified raw-observation watermark to pass the order expiry, so upstream catchup cannot erase a delayed adverse fill. Wall-clock-expired orders awaiting source evidence remain explicitly pending. Fresh decisions require a live, fresh upstream worker; historical evidence and owned inventory survive upstream failures. Runtime/workspace/cohort STOP markers stop the worker. Capture-session rotation ends this source cohort; it does not reset its accounting or adopt another source silently. A later source handoff must preserve any wallet inventory and unfinished outcomes.

Initialize a new named cohort once, then run or resume it:

```powershell
./Start-StreamDip.ps1 -Name stream-dip-20261002 -InputCohort stream-books-20261002 -Initialize -KeepSession
./Start-StreamDip.ps1 -Name stream-dip-20261002 -KeepSession
```

Never reinitialize an existing cohort or remove an unknown lock/STOP to force a restart. The global `stream-dip.lock` enforces one local consumer. Journals and bank files live in restricted runtime storage, while atomic `stats.json` is only a derived projection. The worker contains two prospective paper wallets; neither changes the official pilot's strategy or limits. Zero added fees do not remove spreads, adverse ticks, depth constraints or losses.

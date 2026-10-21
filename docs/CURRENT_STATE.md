# Current Moloch and Tartarus state

Observed October21,18:48UTC. This page replaces repeated historical headlines; [the previous document is preserved in full](CURRENT_STATE_HISTORY_20261021.md).

## Production

Production is stopped. R69 was installed18:43:11 as PID1021520/startTicks57784596, binary8b903763c346 and controlebca4459c87a verified, then stopped18:43:28 with `UPGRADE_FILE_BOUND` before native replay/orders. Its152,404,846-byte preserved upgrade chain exceeded the128MiB parser bound; deployment omitted that size check. Financial head773995/all433 fills and the2,552,479,691-byte prefix are unchanged. R70 raises the engineering bound to256MiB, moves parsed approval entries instead of cloning them and verifies seals by reference. It is building/qualifying. The failed R69 approval remains preserved. [Initial installation observation](../research/tartarus-learning-v2/r69-throughput-installed-20261021.json).

R68 stopped18:22:52 when fundamental attempt20264 expired before SDK admission. Original SDK archives prove no HTTP was sent. The checked repair retires such attempts and continues only for genuine expiry; clock regression and actual uncertain acceptance retain their handling. File-only recovery appended the exact no-send proof, preserved STOP until installation and did not retry the order or alter finances. Historical reconciled exposure18533 still requires its original native startup verification. [Exact recovery](../research/tartarus-learning-v2/r68-expiry-recovery-20261021.json).

R68 additionally sold433 exchange851 NO shares at0.895 on order54228551, executed18:17:32 and reconciled18:21. Original receipts and exact FIFO establish387.535 proceeds,378.875 released cost and8.66 realized gross. All430 preceding fills and remaining FIFO lots match. Earlier R67 sales realized+113.21 and−277.84, net−164.63. [Latest sale/account audit](../research/tartarus-learning-v2/r68-last-sale-20261021.json), [earlier loss](../research/tartarus-learning-v2/r67-second-sale-reconciled-20261021.json).

R69 keeps the R68 stream repairs, adds shared immutable review maps and direct single-order lookup, and expands the native market capacity to the complete237-exchange scope. It also removes a four-lifetime research-observation quota; that quota did not limit trades.130 native,5 lookup-unit,6 Session-lookup,9 capacity,17 SDK proof and228 worker checks pass. A copied773617-event prefix exactly reproduces the original sealed financial report,430 fills and20263 orders; startup additionally validates the final433-fill boundary. Ten explicit source paths differ from deployed R68 among289 source files. [Exact source composition](../research/tartarus-learning-v2/r69-source-qualified-20261021.json), [original-prefix result](../research/tartarus-learning-v2/r69-original-prefix-parity-20261021.json).

The reference updater's expired request228844cf was bound to the confirmed-dead R68 process and absent from the original committed cohort journal. Its metadata was resolved under exclusive owner/updater locks, preserving the request and active generation38; no account request, forecast adoption or financial mutation occurred. Updater1021523 now follows R69. [Resolution evidence](../research/tartarus-learning-v2/r68-reference-resolution-20261021.json).

Last reconciled native financial state:

| Measure | SUSQies |
|---|---:|
| Cash | 17,283.635 |
| Remaining attributed position cost | 78,470.845 |
| Total attributed realized gross | 174.565 |
| New-trading portion | 98.045 |
| Unchanged inherited realized portion | 76.52 |

The fresh18:37 official account observation reports77,625.58 marked position value plus17,283.635 cash =94,909.215, down5,090.785 or5.090785% from the100,000 start. These marks are not executable liquidation proceeds. Today UTC has37 fill receipts across8 distinct filled orders. The97,000 deadline was missed; no whole-account profit is established.

There remains one official worker and shared limiter. Daily windows and original monetary caps are unchanged. Laptop training creates no account client.

The individual-order lookup avoids rendering the entire20,000-order history on cache misses. A synthetic component benchmark drops the second full-render sample's144.47ms/lookup to roughly0.068ms; complete worker speed and increased fill throughput have not yet been measured. [Scope and exact limits](../research/tartarus-learning-v2/single-order-lookup-20261021.md).

The actual laptop external feeds are healthy:Kalshi237 tickers at1,193 REST admissions/minute, oldest receipt11.9s, no throttles; Polymarket378 outcome books/189 markets, configured30 REST admissions/s, receipt ages3.19s median/6.37s maximum. Both websocket feeds are receiving. A retired VPS Polymarket status file is not the live feed. These are observation freshness figures, not contract-equivalence or profit evidence. [Measured feed state](../research/tartarus-learning-v2/current-external-feeds-20261021.json).

## Learning

Active037 remains on its original frozen source. Its first island completed2,773 optimizer updates/36,730 decisions and entered full-window evaluation. Its second island completed one member, then resumed learning another. No independently profitable checkpoint has emerged.

The2,768-update checkpoint's complete October17–19 development results were-4.35% nominal/-4.94% stressed marked ROI. Its earlier founder is stronger by the human's daily-growth reward. The other island's first member produced only0.05 SUSQie nominal marked gain on100,000 initial wealth and zero stressed; this is near-inactivity, not a demonstrated edge.

Queued040 preserves the completed island populations before source migration. It includes founder retention, independent learned BUY/SELL target selection,64-transition PPO batches, faster equivalent history validation and owned-response delivery. The last optimization matched every observation, action, paid request/receipt and financial hash in four interleaved fitting-hour passes; measured collection speedup was1.040x alongside existing GPU training. That is not end-to-end training speed or ROI.

A separate optional process-evaluation implementation passed22 unique focused checks and exact parity. Four interleaved CPU passes over the same five-minute development slice took116.24s median locally versus154.12s with child worlds:32.6% slower including startup/IPC. It remains disabled and is not added to037/040. No optimizer step or protected-test access occurred. [Negative benchmark](../research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

The full requested competition-history training plus4-day evolution/4-day independent test remains incomplete. Current detailed development uses2 fitting days and2 development days. ProtectedOctober21–25 is unconsumed; no learned model is deployed. [Implementation and evidence](../research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md).

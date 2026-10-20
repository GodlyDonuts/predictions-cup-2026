# Faster development evolution and explicit API metadata

The current weakness is not a positive-only reproduction gate. Negative models already reproduce, and historical island champions now survive worse generations. GPU018 enables that rule. Its first three completed development rewards remain negative. No independently profitable checkpoint exists.

## Reduce time between generations

The former profile uses eight members, eight 900-second training worlds per member and four selection cases spanning two 1,499.5-second calendar blocks. Even without model/IO overhead, that is substantial inference before a replacement is produced.

`development_selection_episode_ms` is an optional **development-only** screen. Each fixed chronological selection stratum supplies its centered interval of that duration. Every member and cycle sees the same calendars, seeds and stresses. No decision cap or new action mask is added; virtual clocks, observation delivery, rate limits and execution remain unchanged within each episode. The default remains full-window evaluation. Full-history experiments reject this option.

The intended next profile uses 120-second training worlds and two 120-second selection intervals under both nominal and adverse execution. Relative to the former profile, training horizon per member is 7.5 times shorter and screen horizon is about 12.5 times shorter. These are simulated-exposure reductions, **not measured wall-time speedups or improved ROI**. Startup, feature computation, updates, checkpoint writing and early WAIT endings affect actual speed.

Each newly improving positive screen winner automatically receives full-window development confirmation with its frozen weights and no optimizer. The confirmation is recorded separately and cannot replace its short-screen selection score. Failure of confirmation is not hidden or called qualification. Screening and confirmation both reuse inspected development history and can overfit it; neither is the requested untouched four-day test. A frozen finalist still needs genuinely unused later data and calibrated execution before any profitability claim.

## Make legal actions easier to understand

The optional `api_target_affordances=true` profile uses architecture `tartarus-raw-api-named-targets-gru-v9` and protocol `sig_named_api_targets_v8`. It appends 31 metadata features to the existing 438 columns:

- Eight explicit read endpoint categories: book, price, trade flow, account, positions, owned orders, collateral and realtime token.
- Shared contract and market identity features linking read targets, contract rows and local packet-planner rows.
- Read-request signature and ordered bulk-packet membership metadata.

These fields describe public catalog identifiers or already delivered cursor filters. They do not disclose prices, inventory, cash, fills or undelivered endpoint responses. External market correspondence remains configured mapping, not independently verified resolution equivalence. Bulk catalog presets are fixed chunks of at most 100 IDs; arbitrary ordered packets and duplicates remain legal. The legacy profile retains its original grouping.

Migration requires the full bulk/component/order/query profile chain and explicit architecture/protocol migration. Every existing tensor value transfers; the 31 new encoder columns start at zero. Optimizers, trajectories, worlds and scores start fresh. Changed observations and preset choices mean action equivalence is not claimed. Older frozen workers retain their semantics.

## Status and evidence

Eight focused metadata checks cover hidden-state independence, matching identifiers, ordered duplicate packets, stable catalog grouping, checkpoint transfer, actual accepted PPO and sealed episode reconstruction. Three development-calendar checks cover fixed chronological screening, full-history rejection and automatic full-window confirmation of a synthetic positive screen. Fixture positives are not market results.

At implementation time GPU018 remains the sole learner on source `4ae154396a3649c4cea25b2cd150ed894806c328`. A source-frozen replacement, real GPU capacity acceptance and fresh accepted updates are required before calling the faster profile active. No production worker, account client, STOP, activation, risk limit or financial ledger is changed by this work.

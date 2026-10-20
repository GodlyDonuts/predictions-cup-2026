# Faster development evolution and explicit API metadata

The current weakness is not a positive-only reproduction gate. Negative models already reproduce, and historical island champions now survive worse generations. GPU018 enables that rule. Its first three completed development rewards remain negative. No independently profitable checkpoint exists.

## Reduce time between generations

The former profile uses eight members, eight 900-second training worlds per member and four selection cases spanning two 1,499-second calendar blocks. Even without model/IO overhead, that is substantial inference before a replacement is produced.

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

The CPU suite passes 354 checks in 70.433 seconds with four CUDA checks skipped while the learner owns the device. A separate integration from frozen commit `741397cc2fb6d27c7beb9dc7625551a05c52c9dd` ran the actual recorded three-venue factory for two members/two worlds/two cycles, accepted 38 updates across 1,026 decisions, and produced four zero minimum-marked-ROI short screens. Every old founder parameter matched its parent; new input columns were zero and optimizer state empty. Runtime source pins matched the frozen source. This demonstrates wiring, **not an edge**. [Recorded integration](fast-evolution-recorded-001.json).

GPU019 is now verified learning from source `741397cc2fb6d27c7beb9dc7625551a05c52c9dd`, actual worker PID45560. At 01:04 UTC it had 2,206 decisions and 14 accepted updates. Its 58,187,885-parameter/469-feature model passed a real eight-world/32-step GPU PPO capacity check with exact serial/batched finance, orders, fills and clocks. New input columns start at zero; every original parameter in all eight saved parents matched its new founder exactly. Compact rollout statistics are enabled; unused acting statistics are skipped while the full learning loss remains unchanged. [Active-run evidence](population-019-fast-evolution-001.json).

GPU018 was stopped through its exact retained training-process handle before the capacity probe. Three completed members, the unfinished fourth member's accepted snapshot and four queued founders were preserved. Twenty-seven accepted updates after that saved snapshot were **not carried forward**; prior archives and evaluations remain intact. This is a corrective weights transfer, not completion of the old generation or resumption of its worlds. New profile scores start fresh. Least-loss evolution and historical island-champion retention are enabled. No whole-generation wall-time speedup or positive ROI is established by the startup proof.

No production worker, account client, STOP, activation, risk limit or financial ledger was changed by this work.

Subsequent checked source adds an [exact public-metadata cache](api-metadata-cache-20261019.md). Its paired CPU observation measurement is 1.3043 times faster, with all 80 input/mask pairs exact; this is not a whole-run speed claim. It is not patched into frozen GPU019 and does not cause a new restart.

Next learning work investigates [credit assignment against complete training outcomes](credit-assignment-20261019.md). GPU019 remains active; a full-return comparison must wait for a completed-generation handoff and its own full-size collection/replay/update capacity proof. No positive ROI is implied by preparing that comparison.

[Five-member result review](population-019-five-member-review.md): four negative screens and one flat, zero-fill candidate. The flat policy currently ranks best but is not profitable; the generation and actual reproduction are still incomplete at that observation.

[Selection-horizon audit](selection-horizon-opportunity-20261019.md): displayed quote paths are much sparser in the two-minute screens than in complete development strata. Hindsight counts are not executable profit. Short screens and positive-only confirmation do not establish all longer-horizon strategy quality; that evidence remains outstanding. The active/queued learners were not changed by the audit.

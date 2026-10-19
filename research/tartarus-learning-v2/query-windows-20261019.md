# Actor-controlled reporting windows — October19,2026

The new opt-in policy can choose whether a fresh SIG trade-history query uses the default full available history or explicit from/to timestamps. Both timestamps are independently sampled float64 latent choices over every nonnegative millisecond through year9999 UTC; sorting produces the requested half-open interval, including an empty interval. This is a query capability, not extra historical data. Future upper bounds never reveal future receipts. Page sizes1–200 and continuation choices remain available.

The cached official `/exchanges/{id}/trades` schema specifies inclusive from, exclusive to, default from beginning/default to now, a maximum200-row page and opaque cursors. Original OpenAPI SHA256:fbdf23e54d317392f9fb4a659b9511a01fad065e7a384f28091ef04894a732c9. Cursor snapshot/order/reporting projection remain explicit uncalibrated replay assumptions. Continuations retain the original query bounds, open clock and processed-source watermark; every request uses the same serialized paid read budget/latency. Only available source facts within the chronological role enter a response.

`QueryWindowPolicy` is a new architecture, `tartarus-raw-api-full-depth-query-window-gru-v4`, with protocol `sig_scoped_finance_query_window_v2`. Separate filter and time heads preserve the existing full order/basket/cancel/read actions. Query density is included in PPO only when meaningful: filter choice on fresh trade queries; time density only when filtering is selected. Default queries and continuation do not acquire meaningless time-density penalties. Own sampled query choices are visible in the existing local-action token.

Use these explicit configuration fields with full-depth observations:

```json
{
  "trade_query_windows": true,
  "full_depth_observations": true,
  "observation_protocol": "sig_scoped_finance_query_window_v2",
  "migrate_observation_protocol": true,
  "migrate_query_window_heads": true
}
```

Migration from a development FullDepthPolicy retains every old weight exactly, initializes the added heads, discards old optimizer/rollouts/scores and records an immutable artifact. Legacy40-feature parents also require the separate full-depth migration flag. Architecture, observation protocol, source files, data and calendars remain pinned; old checkpoints cannot silently acquire new semantics. Full-history qualification does not follow from a development migration.

Frozen mutation probes now preserve every mask, including trade_query. Local mutations/head resets include the new heads, and descriptive action distance includes their outputs; the reported family/target KL remains a component rather than full joint-action KL.

## Verification and current state

194 action-suite and12 realism checks pass. Focused coverage includes millisecond representability, half-open filtering, future receipt exclusion, paid latency, optional filters, continuation semantics, teacher-forced meaningful densities, old-weight preservation, checkpoint roundtrip, carried recurrent PPO, query-mask tamper rejection, mutation and a complete two-cycle synthetic population/evolution exercise.

The actual existing three-venue development corpus also passes an eight-world16-step CPU probe with235,677parameters: exact serial/batched wallets/orders/fills/clocks, recurrent tolerance, a1.296s accepted PPO update and a changed optimizer guard. Collection was1.075s serial/0.978s batched under concurrent training; this small CPU measurement is not the58M GPU capacity result or a profitability result. The initial probe001 requested eight priming books before the real source had discovered them and failed before calibration; it is retained. Probe002 uses the already-inspected120-second offset. [Exact proof and runtime source hashes](query-window-cpu-proof-001.json).

The active015 learner and queued016 frozen source have not been hot-loaded. This query profile requires a checked successor source and its full-size GPU capacity/update proof before being called active. The current learner continues training throughout development. No independent test, account client or production state was changed.

Remaining gaps include complete historical three-venue coverage for the requested daily split, complete order/reporting/collateral/lifecycle/official-valuation parity, execution calibration and independently positive ROI. The new query fields do not close those gaps.
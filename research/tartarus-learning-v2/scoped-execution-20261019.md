# Scoped order receipts and private fills — October19,2026

The previous replay protocol published a full cash/positions/open-orders/advance snapshot after every write, including rejected requests. Its derived private stream also published a full account snapshot after fills. The official API returns scoped placement results and private fill/order arrays instead. Those old observation assumptions let the actor obtain unrelated financial information without paying for its REST endpoint. They remain frozen historical experiment semantics, not current API parity.

The opt-in `sig_scoped_execution_receipts_v3` protocol closes that leak:

- Placement ACKs expose only the operation's canonical order results, quantities, fills' total notional and last outcome-side execution price. New visible ACK tokens include order/contract identities, open state and local receipt age. They do not expose unrelated cash, holdings, advance or order metadata.
- CANCEL closes its known target; cancel-all updates only previously known targets in its requested scope. A rejection does not reconcile any financial resource.
- Private fills coalesce into the existing one-second modeled publication window and retain YES-normalized price, quantity, order/contract identity and execution time. Their raw tokens are visible and affected known remaining quantities change; cash/positions/advance are not replaced with hidden full snapshots. REST account/positions/collateral baselines remain charged and authoritative.
- Per-order internal revision fences prevent stale ACKs, delayed fills or older complete order reads from resurrecting canceled or pre-fill state. Independent older order baselines can still update other orders. Missing order metadata after a fill requires reconciliation; it cannot be invented from hidden physical state. Internal revisions never enter feature tensors.
- An expiry removes physical resting liquidity as before, but becomes actor-visible through a delayed connected private update or paid order baseline. Disconnection/token changes fence queued old private events. Private event buffers reset on a new connection and do not replay undisclosed historical fills.

Actual simulated cash/FIFO/fills/fees/realized values, admission, matching, idempotency and the shared limiter are unchanged. This is an observation correction, not a new capital or trading restriction. The policy retains all existing order, batch, basket, cancel and query choices.

## Configuration and evidence

```json
{
  "trade_query_windows": true,
  "full_depth_observations": true,
  "scoped_execution_receipts": true,
  "observation_protocol": "sig_scoped_execution_receipts_v3",
  "migrate_observation_protocol": true,
  "migrate_query_window_heads": true
}
```

The query architecture remainsv4, but protocol identity is distinct. Existing weights transfer only through explicit development migration; old optimizer/rollouts/scores are not reused. The full-depth-to-query architecture adapter now preserves the explicitly migrated protocol. Source, data and calendars remain pinned; qualification cannot be inherited.

205 action-suite and12 realism checks pass. New checks cover ACK/rejection scoping, local receipt clocks, paid finance refresh, per-order merging/tombstones, private multi-fill publication, disconnection/expiry/reconciliation, hidden-revision nonleakage, late ACK after an unknown fill, explicit protocol-preserving architecture migration, accepted recurrent PPO and two complete synthetic population cycles. A forced-command comparison produces identical legacy/new physical finance after every operation.

The existing real three-venue development corpus passes a fresh eight-world16-step CPU probe:235,677parameters, serial/batch wallets/orders/fills/clocks identical, recurrent tolerance and accepted PPO in1.270seconds. All101 runtime source hashes verify against the tested files. This is a small engineering capacity check, not the full58M GPU profile, an independent test or positive ROI. Probe001 (before the migration-adapter fix) also accepted; both private runs are retained. [Proof and exact source hashes](scoped-execution-cpu-proof-001.json).

Official source: cached OpenAPI SHA256fbdf23e54d317392f9fb4a659b9511a01fad065e7a384f28091ef04894a732c9, POST/orders response and POST/realtime/token account_batch documentation. Modeled publication delay, receipt latency, source-private ordering and delivery guarantees remain uncalibrated. Raw topic revision gaps/drops, complete private settlement/refund/collateral deltas, endpoint pagination, exact lifecycle timing and complete financial response parity remain unfinished. No calibrated production matching claim is made.

At this publication the original015 trainer18300 and checked query-v2 pending controller29172 remain on their frozen sources; this correction is not hot-loaded. The new protocol is checked code and CPU evidence, not yet active full-size training. A successor must preserve weights/source calendars, explicitly migrate the protocol, measure actual GPU capacity and show an accepted update before being called active. No independent testing data, account client or production financial state was changed. Full-history coverage and independent profitable ROI remain unproved.
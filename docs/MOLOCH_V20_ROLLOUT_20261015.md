# Moloch V20 checked rollout

Execution task [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120). The human requested production remain active during development. Candidates are built and checked separately from the immutable live release; promotion uses one stopped-owner boundary with the same bank, attributed lots, account queue and activation deadline.

## Changes and purpose

Public source events use bounded durable batches, reducing repeated report publication and filesystem synchronization. Eight complete websocket frames per drain prevent an unlimited stream backlog from monopolizing the serialized worker. This is not a hard wall-clock reconciliation guarantee. [Batch limits and measured offline results](MOLOCH_SESSION_BATCH_20261015.md).

Expired never-submitted inventory exits can request fresh account/contract/book facts and produce a new native quote. Only that new identity may proceed through the original activation wait and normal physical authorization. Exact owned quantity, other reservations, pending physical orders, global uncertainty, STOP and immutable head/lifecycle bindings remain checked. No expiry is extended and no old intent is retried. [Exit flow and regression evidence](MOLOCH_EXIT_REFRESH_20261015.md).

The SDK can understand an explicitly migrated v2 queue through typed summaries and exact lookup. **Production remains v1:** the worker's legacy queue status path deliberately refuses v2, and memory/capacity/migration checks remain outstanding. Merely shipping adapter source does not migrate or reset the account queue. [Adapter evidence and limits](MOLOCH_QUEUE_V2_ADAPTER_20261015.md).

## Verification before promotion

ARM64 locked offline checks:103 native,173 combined runtime and150 SDK library checks pass. Native and combined runtime locked releases pass; runtime release took2m34s. The checked runtime binary SHA256 isff286193026880b255a6734c76feb88e17451082e82d026f435229c40b2bc57f, release log SHA256395c14dce92e2b9f212b0f52667abdbad77d403f19a9674c204ec4b669b30f03. Nine changed runtime/native/SDK source hashes matched the parent checkout and build host exactly. Synthetic batch474-event ingestion improves3.553s to0.3015s with exact byte/restart parity. The saved16,535-event original financial prefix has identical report/orders/fills/cursors. [Sanitized artifacts](../research/moloch-session-batch-20261015/checks.json).

The Session change alters native identity. Promotion must gracefully stop the sole worker, verify all physical attempts have acknowledgments and the manifest-selected queue has no unknown/active/manual-review state, then generate an exact stopped-prefix financial equivalence bridge. Preserve prior bridge entries. Bind only the checked new binary hash in control, preserve14:44:34.413Z entry cutoff and existing risk limits, then start the same single service. No new collector, ledger reset, idempotency eviction or inferred fill is permitted.

## Production observations before promotion

V19 PID151390 stopped at11:45:17.225Z withLIVE_STREAM_FAILED. The parent discovered this on the12:00:40Z health check; earlier running observations did not prove uninterrupted uptime. The latest pre-stop dispatch contained129 attempts, each bound to one original/recovered acknowledgment; the selected account queue had no unknown/active operation or manual review. At12:01:51Z the parent archived only that exact stream-failure STOP and restarted the unchanged checked V19 release as PID165813. Risk, history, queue and deadline remained unchanged. The new process was still replaying history at12:03:13Z; active systemd status alone does not establish resumed trading or a healthy complete cycle.

The prior realized ledger split was17.935000 inherited gross and−0.110000 new trading, total17.825000 SUSQies. The new525 NO purchase on exchange1012 is an owned fill, not realized profit. Neither correctness nor faster replay establishes a profitable strategy or a predicted leaderboard finish. Fresh production checks and source-bound realized outcomes remain required after promotion.

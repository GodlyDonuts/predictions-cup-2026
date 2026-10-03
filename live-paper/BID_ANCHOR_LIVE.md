# Receipt-aware paper companion

`bid-anchor-live-host.mjs` is a separate prospective cohort over the unchanged bid-anchor strategy and exact wallet engine. The original foreground replay bank and executable-edge bank remain frozen. Its actual consumer journal records each batch's availability time, complete source-byte boundary, source-chain hash and wallet state hash. Restart replays those original availability times rather than assigning historical books prompt consumption.

The adapter stamps every newly prepared or repriced intent with actual batch availability. A later request must satisfy its latency fence before a hypothetical fill. Quotes consumed more than twenty seconds after receipt cannot create entries or fills. They invalidate the affected observation and cancel pending buys while retaining positions and unsold risk. Recovery requires a request strictly after the actual invalidation barrier. A source gap likewise imposes a consumer-time barrier; a response already in flight cannot restore continuity.

The host has an exclusive nonce/PID lock, frozen code/bank checks, original source-prefix and per-batch integrity checks, append-and-fsync journal, checkpoint replay and explicit STOP fences. It has no account client or competition-order interface. Counterfactual fills remain approximate next-snapshot opportunities, with zero fees, one-percent depth, adverse ticks, exact FIFO and fixed one-/three-second latency wallets. Journaled observation latency is not proof of attainable contest execution.

It is hosted inside the existing observed-edge process:

```
node live-paper/executable-edge-observe.mjs --name executable-edge-20261003-v2 --bid-anchor bid-anchor-live-20261003-v1
```

The optional companion cannot initialize or overwrite a bank from this wrapper. Its startup or consume failure stops that companion and writes a sanitized failure record under the original edge directory while the original observer continues. Graceful wrapper shutdown releases both independent locks. No additional background process or API traffic is created.

For an explicitly selected new cohort, the foreground host command with `--initialize` creates a new immutable bank and genesis journal; an existing name is rejected. Subsequent foreground one-shot consumption is available for testing or repair only when the companion's live lock is absent. Do not race the deployed owner, remove a live lock or revive retired workers.

`BID_ANCHOR_LIVE_STOP`, workspace/runtime STOP or the cohort's STOP prevents its activity. The original observer's OBSERVER_STOP stops the combined process. Correctness tests cover two journals in one wrapper, actual versus publication-time latency, stale-backlog rejection, strict recovery barriers, exact restart hashes, exclusive ownership, STOP, and isolated companion failure. They do not establish profitability or authorize competition activation.

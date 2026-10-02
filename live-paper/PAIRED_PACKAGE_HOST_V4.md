# Session-aware paired host v4

This is a compatible host upgrade for an existing frozen paired-v3 bank. It cannot initialize a replacement experiment. The original engine, policy, cutoff and bank bytes remain unchanged. Historical checkpoints replay with exact v3 semantics; only new checkpoints carry the immutable v4 host source hash and session-aware version. Reopening validates both the original bank identity and the new host hash chain.

A verified collector session closure invalidates quotes and confirmation anchors and withholds liquidation marks. The process waits for a new validated same-scope publication session while retaining lots, cash, realized PnL and pending intent evidence. It never invents fills during an outage. Pending TTL expiry is resolved on the next verified publication batch; second legs remain sized to actual first-leg fills. New session-start events invalidate old quotes even after an abrupt collector failure without a stop record. Source chain, scope, clock, sequence, unique-session and file replacement checks remain mandatory.

STOP still ends operation. The existing single paper lock prevents two cohort writers. No API, credential or contest-order capability is added. Old host/engine files must remain available for original bank identity validation. Synthetic restart successes prove continuity/accounting, not strategy profitability.

Run `node --test live-paper/paired-package-watch-v4.test.mjs live-paper/paired-package-watch-v3.test.mjs live-paper/paired-package-paper-v3.test.mjs execution/exact-money-receipts.test.mjs`.

# Preserve early market frames during subscription startup

R71 stopped at 20:06:41.093 UTC on October 21 with `LIVE_STREAM_JOIN_FAILED`, after restoring the original financial ledger. Its generic error erased whether the first item was an early broadcast, a typed socket failure or a join timeout. The specific historical cause is therefore unproven.

The native protocol legitimately emits market broadcasts once the public subscription has joined, even while the private-account subscription acknowledgment is pending. The worker previously required its first incoming item to be the combined JOIN. R72 retains up to64early frames/4MiB until the combined acknowledgment, then admits them with their original payloads and clocks. No frame is treated as evidence of a completed subscription.

The same12second waiting window remains bounded. Typed socket failures, subscription timeouts and disconnected startup receivers use the existing persisted attempt budget: at least60seconds between attempts, three per rolling10minutes, including the original token-mint marker. Each attempt rechecks STOP/control state and the quiet shared account queue. Rejected subscriptions, malformed protocol data and source/durable errors remain separate failures. No contest order is retried.

Five focused checks cover frame ordering, original timestamps, unconsumed post-JOIN frames, bounded missing acknowledgments, rejected/protocol failures, transient socket failure and both message/byte overflow. The worker uses the unchanged R71 native and runtime libraries; no new financial-engine bridge is needed. The optimized ARM64 worker builds and its CLI loads. Exact staged worker/helper source hashes match the isolated build. [Qualification and original startup failure](r72-stream-join-qualified-20261021.json).

R72 completed restoration and its subscriptions joined at 20:35:34 UTC. Fresh trading was independently verified at 20:36:52; every original fill and the complete original financial journal prefix match. Version 3 review activation remains in progress. [Fresh live proof](r72-fresh-live-20261021.json), [installation](r72-stream-join-installed-20261021.json). Profitability is not established.

# Full-account production release — October16

This original funding deployment is now continued by SIG503120/source31aefba5.
The binary-only writer replacement preserved the funding and liabilities below;
fresh trading returned12:57UTC. [Current successor evidence](../moloch-hotpath-20261016/deployment-README.md).

Historical follow-up12:22:54UTC: worker494293 remains fresh, with healthy realtime,
matching binary and no STOP. It has34 new official acknowledgments/one actual
post-launch fill row, and its complete-cycle status now matches the new PID.
Capital and attributed realized remain unchanged. The linked live-verification
file is the latest dated derived observation; the initial observation below
remains historical. No buffering candidate has replaced this release yet.

At11:51:16UTC, the sole SIG worker494293 was verified trading with healthy
realtime, the expected immutable binary and no execution STOP. Since launch it
had seven new official acknowledgments and one newly executed fill row. Its
per-scope progress was fresh; the first complete-cycle status was still pending.
This is evidence of execution, not profitability or a promised trading rate.

The owner contribution committed at11:47:41.231UTC, followed by the explicit
daily-mandate revision. Original initial capital40,000 plus55,579.915000 of
previously unallocated owner cash gives effective initial capital and maximum
commitment95,579.915000. Cash72773.995000 plus attributed owned cost22731.515000
equals95505.510000. Ordinary realized−119.535000 and inherited gross45.130000
give combined attributed realized−74.405000. Owner funding credited no profit
and did not change the official account balance. Dashboard valuation is a
different quantity.

## Exact deployment and preserved history

Source `e7bb62d1fe20dbce5fd54e35134a569f686e9409`, runtime SHA256
`d0a3113c3cc3cbae052388a7018b366ae78aafdcd93f338e6efc4c609cc2f739`,
engine identity
`653fa63717821cccf346799b5e9af7d2f95c24c38d7124b9ffa87d25a7ffeac3`.
The original account worker, bank, durable dispatch history, credentials and
exclusive capture lock continue. No additional SIG account client was created.

The controlled stop at11:26:20UTC captured237,382 original events and729,558,692
bytes with no unknown/active/manual-review SDK request. The actual new Session
replayed that entire prefix and genuine proposed eleventh upgrade bridge on a
detached private copy in424,344ms. Cash, inventory, orders, fills, native head
and source cursors matched; original source bytes were unchanged. Only then
was the bridge installed and the new service started at11:38. Earlier fill
rows recovered after restart remain attributed to their original execution
times; they are not new post-launch trades.

The first attempted upgrade was rejected on the detached copy because the
8,773,184-byte bridge exceeded the old8MiB reader limit. It did not modify the
original bank. The old worker resumed while the reader bound was corrected to
32MiB; the16-entry limit, seals and financial verification remain. Fourteen new
reader/copy checks and four optimized artifacts passed.90 runtime,265 native
financial and10 breadth checks are retained evidence for unchanged source,
explicitly not reruns of those suites.

The release adds full-account allocation and mandate migration, bulk negative
screening, and128MiB rotating all-market capture with an8GiB run bound. Existing
per-market/quantity limits and order-uncertainty recovery remain. No borrowed
collateral, certified semantic floor, calibrated Kalshi price oracle or prize
optimizer was activated by increasing capital availability.

## Research and uptime

The read-only observer496337 now consumes the new all-market capture without
another SIG request. At11:51:14UTC it had378 decisions,487 future labels and188
observed markets. It preserves independent REST/WS histories and explicit
causal lag anchors. Missing future books remain missing labels. Original
datasets are preserved. This larger dataset has not yet qualified a live rule.

A systemd drop-in was loaded at11:49:32UTC without restarting494293. It adds two
STOP conditions, on-failure restart after60seconds and a maximum of three starts
per hour. It removes neither STOP nor queue leases. Static verification and
actual effective properties passed; no production failure was injected. The
legacy SDK pathname lease can survive SIGKILL/OS crash, so this is bounded
failure handling, **not qualified automatic crash recovery**. See the
[source-backed uptime review](../moloch-uptime-20261016/README.md).

## Evidence

- [Release, exact-prefix parity and contribution hashes](production-rollout.json)
- [Dated actual trading/capital observation](live-verification.json)
- [Actual observer deployment and sample counts](observer-live-verification.json)
- [Effective restart properties and their limits](uptime-live-verification.json)

These are dated derived aggregates. Original receipts and financial journals
remain private, unchanged source evidence. No profitability is established.

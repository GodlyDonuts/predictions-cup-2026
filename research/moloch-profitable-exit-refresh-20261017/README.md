# Profitable inventory exit refresh — October 17

The existing worker can rebuild an expired maximum-age exit from fresh facts,
but excludes a profit-only `inventoryAON` draft from that path. Its shared
selection also lets speculative read hints compete with a currently executable
profitable SELL. This is a strategy execution barrier, not evidence of alpha.

The [frozen actual-source audit](original-evidence.json) found **1,301 native
profit-only drafts for inherited exchange 948 NO**, each with an original
13,000 ms window and a durable `not_sent` proof with zero network attempts.
They refer repeatedly to the same inventory and must not be counted as missed
fills, added together as profit, or treated as independent opportunities.

The source is the sealed report at sequence 396960 /338 fills, exact dispatch
prefix 18127 and the original 1,258,635,132-byte native journal prefix. Their
full raw hashes were checked. A bounded final 24 MB slice supplies only limited
earlier-book coverage; stale, missing and gap states remain explicit. Showing
depth at a historical price does not prove that another order would have filled.
The full 1,301-row derived audit remains private; this folder retains the last
eight rows and original source pins. Boundary basis is not an independently
reconstructed basis at every draft time.

The candidate adds an explicit profit-only read-hint kind, current native
worst-lot/fee/minimum-gain checks, and priority below risk exits for genuinely
current marketable profitable SELLs. A refresh must use the original seven
fresh receipts and a **new actual native quote**. It cannot extend an old TTL,
reuse an attempted qid, become a maximum-age loss exit, or override managed
forecast/package inventory. Unknown outcomes and existing ownership controls
remain authoritative. No native engine, rate limit, source clock, monetary cap
or profit floor changes.

**Status: source 4bbeae46 /binary c734ca4c installed at 04:39:58 UTC as
worker 611948; restoring, fresh trading pending.** The prior worker stopped
at exact sequence409030 /351 fills. [Actual installation evidence](deployment/README.md).
The [build proof](build-proof/strategy-check-summary.json) records 849 actual
passing checks, including explicitly reused unchanged checks, the native identity
probe and verification of eleven settings/four original model generations.
Two earlier test-fixture failures and their corrections are retained. Root
independently compared all 262 built source files with committed source; the
read-only independent review found no concrete blocker. Original 13-second
windows can still expire during serialized preparation, so no latency or fill
improvement is claimed. A source fix is not a fill or a profit result. Fresh new-process trading/fill evidence will be added separately. Infrastructure
work remains deferred unless essential.

`original-audit-source.py` is the exact one-off owner audit used on retained
private VPS paths. It reads fixed original prefixes and writes a new temporary
derived artifact; it makes no API request or production mutation. Reproduction
requires access to those private originals and an unused output directory.

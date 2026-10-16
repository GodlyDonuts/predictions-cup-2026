# Owned exit latency: exchange 1019, October 16

Read-only observation at 07:30–07:32 UTC, production source `ea733f6f`, PID419050. No account API, queue commands, order, control, service or bank mutation was performed. This is a scheduling diagnosis, not an execution or profitability claim.

**The 60-second inventory policy is not an achieved exit-time ceiling.** The newly owned position had no maximum-age exit draft, so the existing expired-draft recovery path could not request its missing current book. A second independent obstacle is the native top-level depth calculation rounding a one-share bid to zero at 50% participation.

## Original evidence

Private retained packet: `Temp/moloch-owned-exit-latency-20261016`. Original report bytes SHA256 `5f1f16fbe084e6d3fc5bd983e184b3fd380089c61b43de3f1e5156658d402355`; original dispatch bytes SHA256 `b88a786d5bdf19a9765a28730ad7e0291f2ba904427c8a5a5daaa838addedb14`. Report seal verified at sequence182099/time07:29:53.287, with its previous hash anchored inside the separately verified journal tail. Report/head/diagnostics were copied while the worker ran; they are not one atomic snapshot.

Original journal range `[514168362,539334186)` contains24MiB, SHA256 `0c7596dad1195ef74138f5438f9aee7e4eb1e79a63ce7ef11ab4c1ffc3bf0b0c`. After dropping the initial partial line, all8,047 complete rows/chain links verified, sequences174400–182446, ending07:31:23.519. The derived target-event extraction is not substituted for these original bytes.

| Evidence | Actual observation |
|---|---|
| Entry | q4510 BUY68YES at.605, actual order42202435, ACK07:21:14.579 |
| Fills | Six original fills,67shares total, executed07:21:25.946–07:21:51.055; owned principal40.535, zero fees |
| Opposite order | q4509 BUY68NO at.390 (not q4511), canceled07:22:10.533, native authoritative terminal sequence180980 |
| Remaining entry | q4510 canceled07:22:33.565; complete-fill terminal sequence181009 at07:22:47.129 records67filled, no remaining physical liability |
| Only SELL | q4520, ordinary fixedGain67YES at.610, created07:22:24.819, later canceled/retired with original `not_sent` proof; no physical SELL attempt |
| Age trigger | Oldest actual execution plus60seconds =07:22:25.946,1.127seconds after q4520 was created |
| Latest target book | Native sequence180998: source07:22:28.6854731, received07:22:29.179, processed07:22:40.097; already11.412seconds old when processed |
| Target gap | Sequence181049: received07:22:40.675, processed07:23:05.262; no later target book in the verified range |
| Depth | Latest target bid.605×1, next bid.600×7348; current native exit depth rule uses only best-level quantity |
| Read pass | Selection07:20:35 placed1019 first; progress07:30:00 still24/31scopes in that pass |

At the retained report, all1019 BUYs and q4520 are retired, owned67YES remain, and **no maximumAge/inventoryAON SELL exists in this exchange's order history**. q4511 has an unrelated never-sent record; it is not the opposite1019 order. The globally latest `exit-refresh.json` is from01:08 and is not evidence that this holding's exit was refreshed. New-trading realized remained−119.100000; the open position is not realized profit or loss.

## Exact mechanism

In [native desired()](https://github.com/GodlyDonuts/predictions-cup-2026/blob/ea733f6f/moloch/native/src/engine.rs#L834), freshness is checked before owned-lot age. Age is calculated at956 and exit generation at974. The last usable pre-threshold book produced an ordinary fixed-gain draft just before the age threshold. Later delivered books were already too old to produce an age exit; the subsequent gap invalidated the scope. Increasing the number of ticks cannot turn those books into current evidence.

In [latest_refresh_hints()](https://github.com/GodlyDonuts/predictions-cup-2026/blob/ea733f6f/moloch/live-runtime/src/worker.rs#L692), a read hint requires an existing expired maximumAge/inventoryAON SELL. This position has none, and its canceled normal SELL does not qualify. The ordinary scope loop at1290 revisits only its cycle-start selection; a fill arriving after the first scope does not insert an urgent read into the remaining pass. Thus owned fill reconciliation continues, but cannot itself supply a fresh book or manufacture an exit draft.

Separately, native974–997 computes `floor(bestBidQuantity * inventoryExitDepthBps / 10000)`. With observed quantity1 and5000bps, permitted exit quantity is0. The deeper.600bid is not considered. A refreshed identical book would still generate no age SELL; any claim that scheduling alone guarantees an exit is unsupported. Changing this financial policy or native depth logic requires separate reviewed source, tests and native bridge.

## Minimal runtime-only remedy proposal

Add a pure `owned_age_reads` nomination helper bound to the actual Session head, native lot `executedAt`, current policy age and exact Scope. A positive newly owned post-adoption lot whose execution age is due can nominate a **READ without any prior exit draft**. Do not apply the ordinary60second loss timer to inherited lots lacking new execution timestamps, or override independently reviewed Hold/package protection.

After normal full owned reconciliation and before each selected source scope, recompute due nominations; prioritize one due scope even if it was already read earlier in the pass. Use the same sole Transport and original REST source/tape recovery, with an explicit bounded fairness/cooldown marker tied to a completed actual read (not a nomination). This is a scheduling addition, not authority to release reservations, cancel, sell, reuse stale receipts or bypass unknown/order closure. Subsequent native desired/physical preflight must still prove current full book, depth, funding, owned quantity and original liabilities.

Required regressions: actual partial fill just after its scope was read; zero prior age draft; stale/gapped book; original execution versus reconciliation timestamp; inherited-only lots; managed Hold; unresolved opposite BUY; actual-read-only cooldown; and same one-share best bid proving that no exit is invented by the read nomination.

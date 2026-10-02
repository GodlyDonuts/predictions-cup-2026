# Observed performance of the official quant bot

`Start-Performance.ps1` starts a local read-only projection beside the sole execution collector. It verifies the existing source journal hash chain once, then processes only appended complete rows. It makes no network requests, reads no credentials, and submits no orders. The owned ledger is independently reconstructed from reconciled bot fills; legacy account positions do not enter this projection.

`performance-stats.json` reports actual realized P&L, owned cost, open P&L from observed full bid depth, and a separate midpoint estimate. Displayed depth does not reserve liquidity or guarantee fills. The estimate assumes access to all displayed bid quantity; the active execution controller retains its 1% depth cap and can exit in smaller pieces. Added fees are zero.

A book must match the tournament, market and YES exchange, have a receipt after the latest own fill, and remain within twenty seconds of receipt. Invalid or stale depth leaves full liquidation P&L unavailable. Thin depth exposes unsold quantity. Each report retains source sequence/hash and the exact reconciled execution snapshot identity. Malformed capture, outages and missing execution evidence cannot silently retain a healthy mark.

The Next.js dashboard reads this single fixed local report and shows bot position estimates separately from official tournament valuations. The route checks freshness and exact current bot ledger identity again. Stale prices do not become fresh merely because the observer is alive. Restarting this observer reconstructs the projection; no wallet, strategy bank, API pacing or execution ownership changes.

Use `PERFORMANCE_STOP` in the restricted runtime to stop only this read-only worker. Workspace/runtime `STOP` also applies. The collector and existing risk exits remain independent of this projection.

The collector now appends each successful normalized public order book immediately to `public-books.jsonl`, before execution handling or the later cycle commit. This removes the measured cycle-publication delay without adding an API read or changing frozen source journals. Each record is fsynced, scope-bound, hash-chained and identified by collector session; only public book fields are allowed. A failed read invalidates that market, account outages invalidate all books, and collector shutdown closes the session. A durable recording failure stops the collector rather than pretending the record was saved.

The observer verifies this stream incrementally and reports its publication identity alongside the original journal identity. Once the stream exists, it is the sole source of marks: invalidations, session changes and closed sessions never fall back to older cycle books. The legacy cycle projection remains compatible before the first publication stream is created. The observer refreshes every two seconds; the twenty-second quote-age limit and original polling interval remain in force, so an honest unavailable mark can still occur between reads.

## Entry diagnostics

`analyzeEntryMarkouts` from `entry-markouts.mjs` is a pure offline analyzer for one reconciled owned YES buy-fill and normalized, explicitly tournament-scoped book evidence. Callers establish attribution and verify their source journal before passing evidence. It performs no network request or runtime write.

The fixed 5/15/30/60-minute measurements use the first subsequent receipt within at most sixty seconds of the target time, walk full bid depth for the original fill quantity, and retain exact evidence clocks and delay. Thin or invalid first evidence remains unavailable; a later favorable receipt cannot replace it. Missing horizons stay missing. Source/version conflicts, future data and requests begun before the fill cannot supply a valid mark.

The analyzer separately reports first observed top-bid profit/stop passages within the first hour and the quantity available at the threshold. These are observations, not guaranteed exits or continuously observed crossings. Sample gaps and repeated source versions are explicit. Markouts continue to describe the original entry quantity after an actual sale, so they must not be summed into account P&L. Multiple fills of one intent are not independent strategy trials. Zero added fees still leaves the bid/ask spread and depth costs.

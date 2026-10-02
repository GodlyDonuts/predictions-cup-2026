# Observed performance of the official quant bot

`Start-Performance.ps1` starts a local read-only projection beside the sole execution collector. It verifies the existing source journal hash chain once, then processes only appended complete rows. It makes no network requests, reads no credentials, and submits no orders. The owned ledger is independently reconstructed from reconciled bot fills; legacy account positions do not enter this projection.

`performance-stats.json` reports actual realized P&L, owned cost, open P&L from observed full bid depth, and a separate midpoint estimate. Displayed depth does not reserve liquidity or guarantee fills. The estimate assumes access to all displayed bid quantity; the active execution controller retains its 1% depth cap and can exit in smaller pieces. Added fees are zero.

A book must match the tournament, market and YES exchange, have a receipt after the latest own fill, and remain within twenty seconds of receipt. Invalid or stale depth leaves full liquidation P&L unavailable. Thin depth exposes unsold quantity. Each report retains source sequence/hash and the exact reconciled execution snapshot identity. Malformed capture, outages and missing execution evidence cannot silently retain a healthy mark.

The Next.js dashboard reads this single fixed local report and shows bot position estimates separately from official tournament valuations. The route checks freshness and exact current bot ledger identity again. Stale prices do not become fresh merely because the observer is alive. Restarting this observer reconstructs the projection; no wallet, strategy bank, API pacing or execution ownership changes.

Use `PERFORMANCE_STOP` in the restricted runtime to stop only this read-only worker. Workspace/runtime `STOP` also applies. The collector and existing risk exits remain independent of this projection.

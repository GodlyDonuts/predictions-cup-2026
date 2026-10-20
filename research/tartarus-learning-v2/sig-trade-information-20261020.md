# Paid historical SIG trades and restored external capture

Checked source `259cadf3` adds a causal SQLite reader for complete, independently audited SIG public trade receipts. The builder reruns the full scope/cursor/receipt audit against its pinned report before indexing. It preserves exact decimal fields, null prices, and distinct legacy/engine facts sharing an ID. Publication rounds submillisecond source times upward; per-role queries cannot see future information.

Optional `SIG.trade_information` bindings add 200-row historical reads alongside candles. They share the existing simulated SIG account limiter and request latency. Every delivered trade reaches the historical neural input, with outcome side distinguished from aggressor side. Public prints create no executable quotes, fill depth, or attributed fills. Existing frozen GPU024 source/configuration is unchanged; this addition will require an explicit subsequent experiment.

Nineteen focused CPU tests pass, covering complete audit/data parity on fixtures, repeated IDs, decimal precision, null prices, submillisecond causality, source/database changes, role boundaries, read limits, shared-budget admission, and neural delivery. These tests do not establish historical execution fidelity or profitability.

The real reader package is deployed at `/home/archon/sig-trade-reader-259cadf3`. Controller948569 is waiting for the full acquisition audit from auditor948141, then will build the actual index without account requests. Reader948111 remains active in the original account queue. At19:42:51UTC, acquisition had986,399 trades/170 completed scopes. Build completion and data admission are still pending.

## External capture restored

A fresh process census found no Kalshi or Polymarket collector, despite stale status files claiming collection. Both were restarted at19:46UTC. Kalshi uses the existing stable `paper6-ws-5` release, approved credential source, original cache and shared backoff/limiter; its lock reclaim verifies the old process is absent. Supervisor23172 owns collector16232. Fresh status reports19–20 read starts/second and237 cached tickers.

Polymarket3732 uses the existing universe and a new output `forward-polymarket-005-prospective`, capped at30 REST admissions/second with public WebSocket capture. It reached378 cached books. WebSocket status contains dropped/invalid frames and a reconnect; this is not gap-free evidence. Contract equivalence remains unverified.

The existing file-only archive was restarted with the new Polymarket journal added, keeping every old source and the100GiB reserve. Its first cycle preserved four more files/70,605,937 bytes, reaching3,656 preserved source files. [Fresh metadata and source pins](external-capture-restored-20261020.json). Capture is allowed across the reserved test dates, but those observations cannot be consumed for development. Prospective SIG capture is still required after the sole historical reader finishes. No production orders or financial-state changes occurred.

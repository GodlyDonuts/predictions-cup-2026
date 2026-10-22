# R92: one historical report per bounded public-stream drain

The human targets at least one distinct filled physical order per minute. At12:32UTC the completed hour contained four such orders and56 empty UTC minutes. R91 is active while this candidate is qualified. This change is not yet deployed and cannot establish either fill cadence or profitability.

## Change

The full `drain_stream` path now checks stream health and takes one queued frame before opening an existing deferred-report scope. Empty drains return without rendering the historical report. The first frame remains part of the same64-frame maximum; all remaining frames use the unchanged bounded drain implementation. Each native batch retains its original event append and durable-head fsync before the next batch. The large derived report is published once on successful completion or on an ordinary stream error after committed input. Session poison/durable errors retain their existing handling.

Limited drains inside existing source/entry/physical-preflight projection scopes call the inner drain directly and do not open another scope. Original normalization, receive and processing clocks, account invalidation, source coalescing, read/write pacing, risk budgets and actual submission checks are unchanged. Native engine/runtime libraries are unchanged.

## Evidence and qualification

The original R91 derived-report profile is50,973,797bytes. Orders occupy28,581,881bytes and reviewed rank reservation proofs occupy21,603,597bytes inside the report body. These are serialized shape measurements, not Rust timing benchmarks. Repeated batch projections were observed in live stacks. This candidate reduces report frequency; it does not delete historical orders, reservations, source events or fills.

Two new checks compare exact event/head/report bytes against serial admission over65 normalized source events, prove original batch heads are durable while the report remains deferred, reopen the same session to compare restored state, and verify a stream error preserves its original type/message and publishes all committed input. Existing worker checks are also run against the same compiled candidate. Memory-backed fixture execution tests behavior and original fsync calls; it does not certify physical-disk latency or power-loss behavior.

## Production transition

Prepare and qualify the complete release first. Then stop the sole current worker through the existing planned STOP/drain path; retain the full financial prefix, original engine approval chain, dispatch journal, current review generation, limits and daily windows. Verify unknown acceptance is absent and exclusive locks/queue are quiet. Install one replacement, preserving the original engine identity. The existing rolling review updater must retain all eleven added execution-mode policies. Do not claim live improvement until the replacement finishes native replay and fresh official execution evidence is audited.

Cold replay previously took about19minutes. Avoid additional runtime replacements merely for diagnostic changes. Real exchange fills and matched exit proceeds, rather than submissions or partial receipts, determine cadence and return.

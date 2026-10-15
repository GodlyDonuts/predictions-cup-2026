# Rust runtime migration

The new `src/bin/moloch-collector.rs` is a native serialized GET collector, not
yet the deployed service. It has no order endpoints. The existing Node release
and disabled systemd service remain untouched until the whole replacement is
verified.

Verified on the Linux ARM64 VPS during this migration:

- `cargo test`: thirteen tests passed on ARM64: five accounting, two collector,
  four replay, and two event-engine integration tests. The resolved dependency
  lock is included.
- Event-engine parity compares 668 intermediate reports in 20 scenarios against
  the existing implementation, including strict/conditional fills, delayed
  executions, fee rounding, unresolved reservations, cancellation coverage,
  seeded changing books, and the causal current-flow entry gate.
- `cargo run --bin moloch-collector -- check`: Rust runtime, configured 100 reads
  per minute, network/trading disabled, both stop markers present.
- Disabled capture invocation exited 1 before creating output.
- `moloch-capture.service` remained inactive.

Collector budget tests exercise 1,000 admissions, strict rolling 100/minute
capacity, 600ms spacing, and clock reversal rejection. This is a simulated-clock
test, not proof of current SIG traffic. No SIG requests were made.

The accounting library now implements integer decimal parsing, tick validation,
upward fee rounding, per-share reservation headroom, partial fills, FIFO inventory,
opposite-side netting, and atomic rejection of invalid wallet mutations. Tests
preserve realized losses and enforce cash + owned cost - initial = realized.
The library is connected to the Rust event engine and both replay executables.
`moloch-paper EVENTS_JSONL POLICY_JSON NEW_REPORT_JSON` replays original events
without waiting or networking. `moloch-replay` now includes the financial engine
directly; its report no longer asks the user to run a JavaScript wallet.

Both release executables successfully replayed the twenty-book public archive
with an empty environment and `/usr/bin:/bin` PATH. They agree with the original
engine on eight quotes, 19.900000 reserved, no fills, and zero P&L. The integrated
replay took 0.010904 seconds for 50.859 source seconds. This small fixture is not
a sustained-throughput benchmark or evidence of profitable trading.

Unsupported optional policy fields fail explicitly with `POLICY_NOT_PORTED`;
they are never silently ignored. Inventory AON/age/hedge extensions, complete
historical wallet continuation, and realtime source integration are still pending.

Remaining before replacing the deployment: persistent server Retry-After detail,
paginated tape coverage, realtime transport, causal event normalization, remaining
inventory strategies and full journal parity, restart/source-journal verification,
Rust readiness/service wrappers, Rust-only release installation, and removal of
the dedicated Node installation after the replacement passes verification.

Baal and Moloch must be evaluated under the same fill, fee, capital, and timing
assumptions before choosing the combined strategy. Keep existing losses and
wallet histories; no reset or transfer is implied by a new executable. One bot,
one collector and one account-wide budget is the requested final deployment.

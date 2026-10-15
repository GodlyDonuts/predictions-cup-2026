# Rust runtime migration

The new `src/bin/moloch-collector.rs` is a native serialized GET collector, not
yet the deployed service. It has no order endpoints. The existing Node release
and disabled systemd service remain untouched until the whole replacement is
verified.

Verified on the Linux ARM64 VPS during this migration:

- `cargo test --locked`: five accounting tests, two collector tests and four
  existing replay tests passed on ARM64. The resolved dependency lock is included.
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
This library is not yet connected to the event engine or persisted wallet replay.

Remaining before replacing the deployment: persistent server Retry-After detail,
paginated tape coverage, realtime transport, causal event normalization, exact
financial accounting and strategy parity, restart/source-journal verification,
Rust readiness/service wrappers, Rust-only release installation, and removal of
the dedicated Node installation after the replacement passes verification.

Baal and Moloch must be evaluated under the same fill, fee, capital, and timing
assumptions before choosing the combined strategy. Keep existing losses and
wallet histories; no reset or transfer is implied by a new executable. One bot,
one collector and one account-wide budget is the requested final deployment.

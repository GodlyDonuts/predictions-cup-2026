# Rust runtime migration

The new `src/bin/moloch-collector.rs` is a native serialized GET collector, not
yet the deployed service. It has no order endpoints. The existing Node release
and disabled systemd service remain untouched until the whole replacement is
verified.

Verified on the Linux ARM64 VPS during this migration:

- `cargo test --locked`: twenty-five tests passed on ARM64, including source cursor,
  initial-resync, Retry-After, and source-parity tests. The resolved dependency
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
they are never silently ignored. Inventory AON/age/hedge extensions and complete
historical wallet continuation are still pending.

`moloch-source RECEIPTS_JSONL CONFIG_JSON NEW_EVENTS_JSONL NEW_STATS_JSON` performs
offline receipt conversion through the same adapter as collection. Configuration
contains `pairs`, `tournamentId`, and `startedAt`. Initial book events stay invalid
until a subsequent authoritative book and complete tape window cover startup.
Cursor coverage requires the complete original window and up to five correctly
ordered pages, taking the minimum projection checkpoint. Source events are also
compared exactly against four scenarios from the existing source adapter.

A sanitized pre-existing SIG capture (48 receipts, four markets) produced 454
events in Rust. Its paper result matched all 17 checked financial report fields:
998.200000 cash, 1.800000 inventory cost, 4.975000 reserved, zero realized P&L,
and two unresolved orders. This is an independent default-policy migration
fixture, not the historical Moloch wallet and not realized profit. It made no
new SIG market-data requests.

`moloch-paper --journal` also verifies sequence, previous hash and full event hash
before applying each row. Replaying a 454-event hash chain reproduced the raw
event replay exactly. Changed, skipped and repeated journal rows are rejected.

The Rust collector now appends raw receipts, normalized events, and a hash-chained
source journal. Persistent failures retain numeric or dated Retry-After delays
with a 60-second minimum. It does not automatically reset a blocked budget.

`moloch-collector benchmark NEW_REPORT` is a credential-free loopback benchmark.
On the ARM64 VPS it completed 100 HTTP requests in 59,407ms, minimum 600ms start
spacing and maximum 100 requests in a rolling minute. It makes zero SIG requests.

## Realtime transport

`moloch-collector capture PAIRS NEW_OUTPUT CYCLES --realtime` uses a native TLS
WebSocket and the private tournament aggregate channel. One serialized REST
client obtains a short-lived token; no token is logged, persisted, automatically
retried or renewed. Authentication has a separate persistent 60-second interval.
The worker waits for the channel acknowledgment, checks heartbeat references,
caps incoming frames and the queue, and stops on disconnect or expiry. Channel
gaps invalidate the affected scope until both REST book and tape recover it.
Live normalized events preserve receipt/execution clocks and record processing
time separately, including when REST response latency delays queue draining.

The implementation follows the [Supabase v1 protocol](https://supabase.com/docs/guides/realtime/protocol).
Native socket tests exchange actual local WebSocket frames, without SIG access.
Twelve reference checkpoints cover mixed REST/realtime precision, revisions,
duplicates, compact resyncs, dirty books and recovery.

A second offline migration fixture verifies the full original 14,754-row captured
stream chain before deriving 431 relevant mixed records. Rust converts them into
786 events and matches all 17 financial fields of the existing engine: 998.200000
cash, zero realized P&L, 1.800000 inventory cost, 14.925000 reserved and seven
unresolved orders. The +0.015000 liquidation mark is not closed profit. This is
an independent default-policy test, not a replacement for the original wallet.
No live SIG authentication or connection was attempted. The `--realtime`
disabled probe exited before creating output, and the service stayed inactive.

Remaining before replacing the deployment: remaining
inventory strategies and full journal parity, restart/source-journal verification,
Rust readiness/service wrappers, Rust-only release installation, and removal of
the dedicated Node installation after the replacement passes verification.

Baal and Moloch must be evaluated under the same fill, fee, capital, and timing
assumptions before choosing the combined strategy. Keep existing losses and
wallet histories; no reset or transfer is implied by a new executable. One bot,
one collector and one account-wide budget is the requested final deployment.

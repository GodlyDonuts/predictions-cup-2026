# R91 maker entry packet — qualified candidate

Production's accepted flat maker scopes1011,1013 and1069 reached planning but
returned INSUFFICIENT_ORIGINAL_PACKET_WINDOW before any physical order. Live
stack samples found full historical derived-report serialization in fresh-book
admission. The current report is roughly50MB.

The candidate moves one existing report-deferral scope around the reviewed
fundamental entry packet, after its separate source catch-up. Preliminary book
admission, the initial seven original receipts, local reservation, activation
and final seven original receipts now precede one derived-report publication.
The private final-packet body is shared with the existing scoped helper; there
is no nested report scope. Original event/head writes and fsyncs, source clocks,
financial admission, final physical checks and unknown-outcome handling remain.
Every normal or semantic-error return publishes the committed derived prefix.
The native engine and runtime libraries are unchanged.

Two added worker checks cover ordered original14-read planning/final packets
under one projection, and quote invalidation before final submission while
preserving the committed prefix. Existing wall-clock-sensitive entry fixtures
now use a fixed local replay clock, preserving their intended7.5-second book
age and exact write-gap behavior despite host fsync delays. Production clocks
and freshness limits are unchanged. Failed offline qualification attempts are
retained on the build host; none was deployed.

All258 worker checks passed on the same compiled test binary with memory-backed temporary files; the original disk-backed run had257 passes and one timing-sensitive fixture failure and remains retained. This isolates test files from production disk contention; source clocks and fsync calls are unchanged. The149 inherited runtime checks are unchanged, not a new runtime-library run. The release binary loads and its native/runtime library hashes are unchanged. See[r91-maker-packet-qualified-20261022.json](r91-maker-packet-qualified-20261022.json). Deployment is pending. A running R90 worker, passed source
checks or accepted review generation do not establish a live R91 speedup, fills,
profitability or sustained one-filled-order/minute cadence.

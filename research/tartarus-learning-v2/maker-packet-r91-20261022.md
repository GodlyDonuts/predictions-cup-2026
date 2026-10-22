# R91 maker entry packet: qualified release

Accepted maker scopes1011,1013 and1069 reached planning but returned
INSUFFICIENT_ORIGINAL_PACKET_WINDOW before any physical order. Live stack
samples found full historical report serialization during fresh-book admission.
The report is approximately50MB.

R91 keeps one existing report-deferral scope around reviewed fundamental entry,
after separate source catch-up. Preliminary book admission, the initial seven
original receipts, reservation, activation and the final seven original receipts
precede one derived-report publication. The final packet body is shared with the
existing scoped helper, without nested deferral. Original event/head writes and
fsyncs, source clocks, funding admission and physical execution checks remain.
The native engine and runtime libraries are unchanged.

Two new worker checks cover the ordered14-read planning/final packets and final
quote invalidation while preserving the committed prefix. Test-only entry
fixtures use a fixed local replay clock for their intended7.5-second book age;
production clocks and freshness limits are unchanged.

All258 worker checks passed on the same compiled binary with memory-backed
temporary files in1.69seconds. Original fsync calls remain. The preceding
production-disk run had257 passes and one timing-sensitive fixture failure;
failed offline attempts remain retained and were never deployed. This test
filesystem isolates fixture I/O from the production journal; it is not a
physical-disk power-loss qualification. The unchanged runtime retains149 prior
checks. The release binary loads and original native/runtime library hashes
match. See[r91-maker-packet-qualified-20261022.json](r91-maker-packet-qualified-20261022.json).

Sourceffefefbc is published to main. R91 was installed at11:34:18UTC as worker1096712 and is restoring the original
ledger. All576 prior fills and the complete original financial prefix are
retained. [Installation](r91-installed-restoring-20261022.json). Fresh trading
and cadence are not yet verified.
A running old worker or accepted generation does not establish R91 fills,
profitability or sustained one-filled-order/minute cadence. The pre-replacement
hour had one newly filled order and59empty UTC minutes; partial receipts from an
older order cannot create another new trade in the cadence report.


The retained pre-replacement report is50,973,797bytes. Its order history takes
approximately28.58MB; the report body21.60MB field
reviewedRankCampaignReservations dominates its22.18MB body. The largest retired
order alone renders1.95MB. Source cursors number only7 and render188bytes, so
cursor growth is not the measured cause. These are Python compact-JSON field
sizes, not a Rust latency benchmark. The profile is read-only and uses the
retained immutable replacement snapshot. Original financial history must remain;
a future optimization should remove repeated rendering/copying of historical
proofs from hot processing, rather than erase the evidence. No additional
production change is made by this profile. See[r91-derived-report-shape-20261022.json](r91-derived-report-shape-20261022.json).

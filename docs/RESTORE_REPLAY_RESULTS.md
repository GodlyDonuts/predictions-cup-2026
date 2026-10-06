# Offline restore acceleration

The new Rust utility verifies every recorded funded checkpoint and compares complete archived wallet reports in one process. It preserves cash, filled cost, pending reservations, inventory and prior losses. It creates no API client, daemon or live writer.

Kansas restored 1,490 checkpoints in 37.80 seconds. The frozen host's captured startup took 294.42 seconds, about 7.8 times longer. The frontiers differ slightly, so this is an operational comparison rather than an identical-head benchmark. Texas verified 1,656 historical checkpoints and 8,661 authoritative overlay checkpoints in 77.31 seconds. Its improvement over the earlier 84.08-second native run is modest; no speed advantage over the Node overlay is established.

Three tests plus actual sealed-prefix and archived-wallet parity checks pass independently. Bank/code pins, file identity, final complete prefix rehash and exact overlay frontier hash are checked. An older public prefix was rejected; it was not accepted by weakening the gate. No original bank, journal or live inventory was overwritten.

See [the measured proof](../data/settlement-restore-replay-proof-20261006.json) and [utility instructions](../backend-rust/settlement-restore-replay/README.md). This is staged infrastructure. It has not replaced the collector's live restore path while the official pilot owns positions.

# Offline median cap plus strict positive trend

Three separately funded controls retain the original native one-tick policy:
original protective limit, fixed original-reference entry cap, and that same cap
with a strictly positive 30-minute versus previous 30-minute median gate. No
active runtime, collector, bank, controller or journal is modified.

All points are known before the ORIGINAL book request and within the preceding
hour ending at actual consumer availability. Drop the newest eligible point.
The original 20-point/30-minute reference warmup remains. Each trend segment
needs five retained samples. Intervals are `[availableAt−1h, availableAt−30m)`
and `[availableAt−30m, originalRequest)`. Use observation-weighted upper medians
on irregular timestamps. Admit a new proposal only when the recent median is
at least one .005 tick above the earlier median. Flat/declining/unknown histories
are separately counted; no qualifying entries is an honest result.

Only permission for a NEW BUY changes. Existing pending proposals, owned exits,
one-share sizing, fixed limits, cash/reservations, 3s independent-request delay,
1% integer rung depth, zero fees, four-tick stop, one-tick target and two-hour
risk horizon remain unchanged. Cap is fixed at proposal creation:
`min(original ask + tick, original causal median bid − tick)`.

The original frozen kernel is included byte-identically. Source readers are
copied from validated entry-cap-v2, including per-book account partition lookup
(global cross-market book request clocks may regress). The original controls
are kept regardless of results. Old development data is not profit proof.

From `backend-rust`, using the existing configured toolchain:

```text
cargo test --offline --manifest-path median-uptrend-cap-replay/Cargo.toml
cargo build --offline --release --manifest-path median-uptrend-cap-replay/Cargo.toml
median-uptrend-cap-replay/target/release/baal-median-uptrend-cap-replay.exe ABSOLUTE_CONFIG.json
```

Config has sealed `publicSource`, `accountSource`, `priceSource` objects with
absolute `path`, `bytes`, `sha256`, plus absolute `universeBank`, fresh absolute
`output`, and optional ISO `experimentCutoff`. Omit cutoff for development;
provide the parent's independently registered future cutoff for fresh wallets
with old data priming history only. Requests and availability must both be after
that cutoff. No old positions, fills or cash changes are imported.

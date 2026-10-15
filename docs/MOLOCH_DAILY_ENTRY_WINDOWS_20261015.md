# Moloch daily entry authorization — October 15, 2026

## Human authority and limits

The human answered **“Continue with daily windows”** to continuing production daily entry windows with the existing **40,000.000000 SUSQies total commitment** and **10,000.000000 per market** limits. This authorizes a finite schedule through the current tournament; it does not establish profitability or reset available capital each day. Maximum single intent quantity remains 10,000. Fixed UTC 24-hour windows use the existing entry cutoff 2026-10-15T14:44:34.413Z as anchor. The final partial window ends 2026-11-04T17:00:00.000Z, taken from the official tournament receipt observed 2026-10-15T18:19:16.939Z. Its sorted compact JSON SHA256 is fa1cf72d0ab5819ed899de078aa6309bc2655cdf22d7d5bfa5c6322cf67fd885. The worker independently checks the fresh official end at startup; physical buys require a fresh tournament receipt whose end covers the durable current window.

## Durable renewal

The original activation and bank manifest retain their original cutoff. A strict optional dailyEntryWindows control object binds an authorization ID, approval timestamp, original anchor and finite campaign end. The existing immutable control-file hash protects it during execution. A live_entry_window journal event changes exactly policy.entryStopAt and records an immutable eight-field mandate containing those fields, fixed 86,400,000ms interval and unchanged financial caps. Native validation rejects changed caps, changed mandate, stale prior cutoff, future approval, pre-anchor renewal, arbitrary future windows and renewal at/after campaign end. Offline restarts can advance straight to the current window without replenishing cash or deleting history.

For anchor A, UTC time t and campaign end C, the half-open current window ends min(C,A+(floor((t-A)/86400000)+1)*86400000), for A<=t<C. It is independent of local time zones and daylight-saving changes. Daily authority is bound only after the actual event is durably accepted and its replayed bank report/policy match the control mandate. A changed window clears any prepared order. The final network guard checks the prepared cutoff again; merely computing the next date grants no authority. Existing owned sells and cancellations remain subject to the ordinary safeguards after entry campaign end.

## Production continuity and promotion

Task start found V22 stopped at 2026-10-15T15:14:19.664Z on STREAM_DNS. All 300 physical attempts were ACK-bound, and the manifest-selected shared queue had 433 intents, no unknown acceptance, no active admission and no manual review. After verifying the exact unchanged release and controls, the owner archived only that typed STOP and restarted V22 as PID182582. Fresh healthy progress at 18:26:19.780Z recorded 302 acknowledged orders, 300 terminals and 86 actual fills. That process managed existing holdings under the expired original entry deadline while this candidate was built.

The daily-window release changes the native financial identity, so promotion requires exact historical-prefix report, orders, fills and source-cursor equivalence plus a fourth explicit stopped-owner bridge. Existing three approvals, bank manifest, seed, positions, queue v1, pacing, recovery budget and risk caps remain. No second SIG client or daily balance reset is introduced. Build checks, prefix proof and fresh deployment evidence will be appended after verification.

## Verification scope

Tests cover exact UTC boundaries, skipped offline days, final partial window, campaign end, immutable original-manifest restart, malformed authorization, tampered caps/mandates, paper-bank rejection, atomic rejection and durable authority binding. Existing native/runtime regressions remain required. Successful tests demonstrate implementation behavior, not a profitable trading strategy.

## Checked candidate

All **121 native** and **194 runtime** checks pass on ARM64 with locked release builds. The unchanged SDK retains its prior150 passing checks. Seven changed source hashes match the build host exactly. The immutable V23 binary SHA256 is `3e1e010c57e74ecada3608cecec00dbf58151a4cbe20b86586d352d50d3886dd`; native financial identity is `8520add404082af22ae484409cfcb972a895ffe612dec014c9d668c70b7ca282`. The saved16,535-event original prefix replays with exact report, order, fill and cursor parity, zero account requests and no source mutation. [Build evidence](../research/moloch-daily-windows-20261015/checks.json) and [frozen-prefix proof](../research/moloch-daily-windows-20261015/frozen-prefix-parity.json).

Fresh stopped-prefix proof and actual V23 deployment are not yet claimed at this source checkpoint.

# Existing-collector median discovery opt-in

This changes only the existing collector. It creates no API client or service. The current four planned depth20 receipts, scheduler, frozen banks and journals keep their meanings. Every third cycle, after four successful unique protected books and the existing bulk sweep, the collector may append at most two depth20 reads through the same serialized `api.request` queue. Bulk prices are never execution prices.

Absent `LOCALAPPDATA/SigTradingCup/paper-runtime/median-discovery.json`, or `enabled:false`, disables the feature. The parent alone owns activation and collector restart. After deploying the checked module and runner, prepare this setting with the actual current universe and `medianDiscoveryIdentity(workspace)`:

```json
{
  "schemaVersion": 1,
  "owner": "Codex",
  "enabled": true,
  "universeHash": "ACTUAL_PRICE_UNIVERSE_HASH",
  "everyCycles": 3,
  "maximumAuxiliarySlots": 2,
  "protectedBookSlots": 4,
  "maximumAccountAgeMs": 20000,
  "codeIdentity": {"EACH_MEDIAN_DISCOVERY_FILES_PATH": "CURRENT_SHA256"},
  "riskSource": {
    "cohort": "baal-median-prospective-20261006-v1",
    "bankHash": "fe61bfcee3e0fbc2bda5170de3fff0f3841c82d451529df2a20403a027098331"
  }
}
```

`medianDiscoveryIdentity` exports the exact six-file identity: this module, runner, market-price-publication, runtime, transport-runtime and execution/ledger. Do not copy example placeholders or silently accept different pins. Restart only the current collector after preserving its rate/backoff state and checking its real PID/lock/STOP provenance; do not run a second collector. Keep the paper observer and its frozen bank/binary unchanged. Allow verified source bootstrap and a full60-second startup admission-history window before optional reads.

The screen checks every original market-price hash-chain row and scoped quote, restores history from the existing source, and evaluates current ask against strictly earlier bid receipts from the past hour: at least20 receipts spanning30 minutes, one tick minimum dip. Repeated receipts are not independent trades. It keeps valid signals from earlier sweep batches, rather than replacing them with the last100 quotes. Current signal age is at most20 seconds. Shared admission timing must leave four extra reads of recovery headroom; cooldown/manual review/STOP suppress the suffix. Complete positions and paginated orders are refreshed if needed, through the same client. Their original account-eligibility publication precedes candidate requests. Account knowledge must precede the actual request and remain fresh at consumption.21-second responses are not published.

`median-discovery-attempts.jsonl` records immutable pre-request intents, receipt clocks and outcomes. Candidate receipts are watched for at least150 seconds, including after the dip disappears and after cold restart. Watch servicing does not extend an empty watch indefinitely. Pinned stats at `median-paper-cohorts/<cohort>/stats.json` supply the union of attributed held market IDs across three independent alternative wallets; balances and pending counts are never summed. Held IDs get priority, with oldest-attempt rotation. Because the frozen observer exposes pending counts without IDs, **any positive pending count blocks new discovery** and retains held/watch risk. Stale or unavailable stats also block new discovery. The feature still services preserved risk when it can obtain fresh causal account/book receipts.

Two auxiliary slots cannot guarantee quick exits across an arbitrarily large union of held markets. With N held/watched markets outside the protected four, a full revisit needs up to `ceil(N/2) * 3` collector cycles. Append-only discovery also delays the next protected revisit. Inspect this cadence and fresh quote clocks before scaling paper inventory; do not interpret the optional reads as production execution authorization.

`median-discovery-status.json` and runner status expose observed/lag-ready/fresh markets, one/two/four-tick qualifying dip counts, protected-only qualifying counts and source frontier. `MEDIAN_DISCOVERY_STOP` stops optional reads immediately; ordinary collection remains under the existing collector STOP controls. Setting/code changes fail closed until a deliberate pinned restart.

Checked:12 helper/shared-ReadClient correctness cases and77 actual runner regression cases (89 total). These cover causal prior-only history, no self-warmup, current account refresh, unchanged protected order, one-second completion pacing,42990-second backoff, stale-response rejection, STOP/config pins, durable watches/rebound/cold restart, held exits, and the realistic four-protected-watches plus one unidentified pending intent regression. Helper fixtures exercise the actual existing ReadClient; default-disabled runner regression fixtures replace only module bindings. They do not establish live activation or profitability. No authenticated API requests, orders, settings or collector launches were performed by this task.

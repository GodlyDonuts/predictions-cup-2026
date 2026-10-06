# Version2 staged median collector risk union

This is a new module. The active v1 module, runner, six pinned source files, banks, journals and runtime are untouched by this task. Parent owns any later runner import change, checked deployment and pinned restart.

Use the same `openCollectorMedianDiscovery` API and callbacks. For v2, set `schemaVersion:2` in `median-discovery.json`, preserve the original `riskSource`, and optionally register up to eight explicitly pinned probability sources:

```json
"additionalRiskSources": [
  {
    "kind": "probability_receipt",
    "activeFile": "source-value-paper-active.json",
    "cohort": "mn-republican-source-value-20261006-v1",
    "bankHash": "EXACT_FROZEN_NEW_BANK_HASH"
  }
]
```

Pointers are basename-only and cohort/bank bindings are immutable. Each pointer must match `{cohort,bankHash}`. Its existing `probability-receipt-cohorts/<cohort>/` directory must contain the original bank, `stats.json`, `worker.lock`, `read-focus.json` and `journal.jsonl`. Original bank schema2 `source_probability_actual_receipt_paper` is supported, along with reviewed continuation schemas3/4 using their existing validators. No source valuation is recalculated or judged profitable here.

The reader enforces the existing probability-read-focus contracts: validated bank/universe; scoped cohort and exact bank hash; original live worker PID/lock/projection hash/code hash/stats hash; no STOP; zero reported account traffic/orders; intact checkpoint tail and state hash. It also binds the exact public source frontier at `stats.sourceBytes`, sequence/hash and canonical row, rejects future source/consumer clocks, and requires20-second stats/publication age before treating observations as fresh. Incomplete or mismatching evidence cannot clear existing risk or justify new discovery.

Owned risk comes only from attributed wallet positions and pending intents. A separate bounded unowned bootstrap read may use the exact registered bank's validated `depth_required` research hint; it never creates or relabels holdings. The bootstrap requires fresh bound worker/stat/publication evidence, resolved settlement/flags, original source and recorded clocks already known before request, and original research expiry/age. It is revalidated after account refresh and checked at actual receipt consumption. Owned risk and retained watches take priority; bootstrap shares the existing two-slot suffix, and no third request is added. An explicit durable `research_bootstrap_read` event precedes the normal read intent, carries `unowned:true`, bank/research/source bindings and sanitized causal eligibility. Its initial receipt creates the original150-second watch for later virtual intent/fill receipts; subsequent actual pending/held reports enter the attributed union. Missing/stale/stopped/expired/future research never grants bootstrap admission. Instruments map to the configured market/exchange universe. Explicit pending market/instrument IDs join the owned union; any unidentified pending blocks new discovery while preserving known held/watch risk. Independent latency wallets are never summed. Validated stale holdings can be retained conservatively for read service; stale reports cannot remove prior risk. Fresh, complete reports can close their own prior risk. Removing a registered source never silently discards its previously attributed IDs: those stay in the durable risk journal and disable new discovery until a deliberate reviewed continuation reconciles them.

The existing `median-discovery-attempts.jsonl` remains append-only and compatible with prior v1 watch/held events. New `additional_probability_risk_projection` records retain source key, cohort/bank, exact market/exchange IDs and source clocks. Cold restart replays those projections. A probability-owned361 therefore retains exit-read priority after the median wallet has exited361 and after its dip disappeared.

Collection stays unchanged: four protected depth20 books first, at most two suffix depth20 reads every third cycle, same serialized client and account freshness/backoff/headroom gates. Owned risk takes suffix priority and rotates by oldest attempts; new entries cannot displace it. This creates no API client, daemon, production activation or order. With N outside-protected risk markets, servicing still takes up to `ceil(N/2)*3` cycles.

V2 source identity uses the complete transitive `.mjs` graph through `collectFrozenPaperFiles`. Generate `medianDiscoveryIdentity(workspace)` after the parent updates the runner import to v2, then pin that exact graph. The old v1 configuration must not be silently reused. Historical bootstrap validates code/config before and after the fully checked price hash chain, retains per-row STOP checks, and still validates code/config before live callbacks and admissions; it avoids rereading all source files on every historical row.

Checks:21 independent v2 cases pass, covering inherited shared-client pacing/freshness/STOP/backoff/watch behavior plus real original bank/projection/journal contracts, explicit pending IDs, stale held retention, changed pointer/tail/projection/future clocks/universe/STOP, source traversal/duplicates, cold restart/deregistration preservation, original probability-engine first book→pending→independent later fill→held-priority progression, and expiry during actual serialized account refresh. Fixtures establish correctness only; no live account requests or orders occurred.

# Corrected stream source and prospective paper cohorts

The version-2 modules connect `passive-capture-v2.mjs` to a separately frozen gateway, replay and paper consumer. The original version-1 import closures, banks and journals remain unchanged. This duplication preserves those experiments' exact source identity; it is not a new collector or an API client.

The correction lets a verified newer REST engine version whose source time passes an old expired stream timestamp supersede that obsolete barrier. REST expiry knowledge stays unknown. `expiryKnown: false` with `nextExpiryAt: null` never means that no expiry exists. Older, unverified, pre-expiry and conflicting same-version evidence remains invalid. Buffered newer pushes retain their actual receipt, version and known expiry. All existing continuity, latency, depth, fee and loss-accounting gates remain in force.

The gateways share `stream-book-gateway.lock`; the paper consumers share `stream-dip.lock`. At most one of each can run across both versions. A live source migration must first confirm the old paper wallets are flat and have no pending outcomes, stop their workers with attributed cohort STOP markers, verify terminal status and released locks, and preserve their active manifests and exact final journal/state hashes. If inventory or pending orders exist, an explicit accounting-preserving handoff is required instead of resetting them. Retired cohorts remain historical evidence, including rejected and interrupted observation windows.

Start only new named cohorts after that retirement:

```powershell
./Start-StreamBooksV2.ps1 -Name stream-books-v2-20261002 -Initialize -KeepSession
./Start-StreamDipV2.ps1 -Name stream-dip-v2-20261002 -InputCohort stream-books-v2-20261002 -Initialize -KeepSession
```

Resume an existing cohort without `-Initialize`. Never overwrite banks or remove unknown STOP/lock files. Workspace/runtime STOP markers apply to both versions; their version-specific and cohort STOP markers remain independent. Source-session rotation ends a cohort and preserves its accounting. The version-2 paper engine is the exact existing `stream-dip.mjs`, with two independently funded 1-second and 3-second latency wallets and zero fees. The consumer journals its own actual availability and verified source watermark, and full read-only replay works alongside the live lock.

This corrects collection, not profitability. No official trading policy, ownership, account credentials, API pacing or competition risk budget changes as part of this migration. Paper opportunities are local counterfactuals, distinct from official orders and queue fills.

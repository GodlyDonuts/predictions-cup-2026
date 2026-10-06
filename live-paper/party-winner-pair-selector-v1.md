# Generic pair read selector

`selectPartyWinnerPairCapture` is a pure deterministic request proposer. It
consumes `screenPartyWinnerNoPairs` output, current sanitized whole-account
exclusions, prior selector state and capture/history state. It constructs no API
client and writes no files. Integrate it only into existing collector price and
eligibility publication hooks; the cache adapter is a one-shot inspection tool,
not a second live collector or an instruction to scan historical journals.

Call with `{universe,screen,account,state,sourcePlan,currentRequest,capture,
terminalRecords,boundPlanHash,collectorCanRead,now}`. Account is the screen's
sanitized shape: complete, tournamentId, sourceHash, excludedExchangeIds,
unknownAcceptance, availableAt, positionsAt and ordersAt. Every nonzero signed
position (including NO) and every open order is excluded. No existing position
is adopted as an owned pair lot.

Explicit source-plan shape:

```json
{
  "schemaVersion": 1,
  "owner": "Codex",
  "id": "bounded-source-v2",
  "mode": "allowed_universe_pair_read_candidates",
  "universeHash": "64 hex",
  "createdAt": "UTC ISO milliseconds",
  "expiresAt": "UTC ISO milliseconds",
  "protectedBookSlots": 4,
  "maximumAuxiliarySlots": 2
}
```

The window must already be installed/authorized by the caller, with maximum
30-minute duration. A missing/expired plan returns an inspection hint but no
request. Request expiry never exceeds that plan. Nothing renews the old outer
window or a trading activation.

Terminal records are `{sourceId,updatedAt,name,marketIds,phase,
positiveConditionalScreen}`. Digest plus timestamp detects an edited source ID.
A negative, failed or interrupted candidate cools that pair for five minutes.
Pair-specific root hashes, actual quote receipt clocks and prices form a source
fingerprint; reading the same cache again does not create a new source generation.
New actual receipts can create a later eligible generation, without bypassing
cooldown. `nextState` must be persisted before publishing the proposed request;
an already persisted nomination blocks another nomination until resolved/expired.

No switch occurs during an active capture, a pending request, or a bound REST
plan. Passing `boundPlanHash` freezes selection without overwriting any plan,
order, pending intent or inventory. Positive unexpired captures await the owner.

Only current/cofresh quotes, a current account and protective cost at most .990
can propose one pair. Stale roots are metadata-refresh hints: the existing paired
capture re-reads/revalidates metadata and future depth/account causality. A current
account may be newer than cached bulk prices for this read-only choice; it must
still be known before the FUTURE executable book packet. Raw bulk quotes never
provide quantities, fills, or write authority. Protected four reads and existing
two auxiliary slots remain the caller's enforced shared-budget responsibility.

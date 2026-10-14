# Fresh REST books inside an exit window

Explicit `live.mjs --rest-recovery ... --exit-priority-books` publishes active, funded AON windows from its own ledger and the already-verified watched paper ledgers. It requires scoped REST recovery with watched banks; it cannot run in realtime mode. The existing ten-second post-activation validity, fresh-depth checks, limit-price accounting and full-liability reconciliation are unchanged.

Before an ordinary tape admission, the capture may swap in an already scheduled book for the same approved scope if the predicted budget admission falls inside an active exit window. Source directory, generation age, exact scope and integer clocks are validated. Each window is promoted at most once. Cursor/fixed-window packets and typed DNS retries retain their ordering. No request is added: the same finite queue, five-read rolling cap, twelve-second completion pacing, backoff and 120-receipt ceiling remain. Promotion logs record scheduling rather than successful requests or fills.

This addresses the measured 25-second alternating book/tape cadence without extending a simulated order's lifetime or reusing stale depth. It cannot guarantee an in-window observation under slow responses, competing scopes or backoff. Unobserved or unsupported exits remain unfilled and funded as before.

All 127 core Node tests and six focused PowerShell checks pass; the capture parser passes. Tests cover valid promotion, duplicate suppression, exact expiry, cursor protection, foreign scopes, stale publication, active-intent filtering and scope identity.

Phase50 was registered at 21:33:09.861 UTC before startup. Both preserved flat banks re-enable their existing one-tick candidate with zero new capital and no changed quantity, fee, capital, attribution or execution-age limits. The prior conditional 0.060 is excluded from phase performance. The REST collector is connected with priority mode enabled and the same financial scope1027. No live priority promotion or profitable forward cycle is claimed from startup alone. Registration: `data/moloch-exit-priority-registration-20261014-50.json`.

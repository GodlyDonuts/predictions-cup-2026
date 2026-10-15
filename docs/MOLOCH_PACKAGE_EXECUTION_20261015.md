# Paper 2 package execution: implementation and remaining work

Paper 2 is **not complete**. This checkpoint distinguishes reviewed code, deployed behavior and economic evidence. The sole production process still uses ordinary orders; the new package financial engine is not deployed. See [production state](CURRENT_STATE.md), [runtime recovery](MOLOCH_V24_CONNECTION_RECOVERY_20261015.md) and [task143](https://github.com/GodlyDonuts/predictions-cup-2026/issues/143).

## Exact package accounting

October 15 follow-up: the merged source passes292 runtime/157 SDK/142 native checks. Positive local-not-sent feedback now verifies the original retained V1 queue generation and an adjacent transition using the actual queue State implementation, exact request/binding, unchanged global uncertainty, and the re-read durable dispatch head. It releases only Attempted packages without ACK/fills; Unknown remains blocked. Compacted-away evidence, V2 generations and mere queue emptiness cannot prove no-send. This is integrity evidence from the trusted sole worker, not a cryptographic authenticity claim. Complete Transport-bound call evidence, startup recovery and production worker wiring remain required; see task143.

The native engine now supports two/three-leg, distinct-exchange BUY packages. Reservation checks the unchanged bank, run, manifest, original funding limits, current books, complete executable depth, entry window and aggregate share cap. It reserves the **sum of quantity times each fixed wire limit**, rather than optimistic weighted-depth costs or future collateral. Runtime admission recomputes a source-certified payoff candidate and rejects it when conservative wire limits remove its minimum surplus after buffer.

Package phases are reserved, attempted, acknowledged, unknown, no execution and complete. A physical attempt binds the immutable SDK request hash and idempotency key and rechecks current depth after the native activation delay. Changed source observations can still qualify at the same fixed limits; worse prices, insufficient depth, gaps or expiration cannot.

Unknown execution keeps funding reserved and blocks new submissions. A package acknowledgment maps all component order IDs atomically while retaining the original full multi-leg receipt digest. It does not create fills or cash. Only independent complete official per-order fill tapes move inventory. Genuine terminal observations close liabilities; completion requires every component fully filled and reconciled. A definitive typed official zero-execution response can release an attempted package, with retained raw-body and metadata/request evidence. A reserved package can release only with a verified never-attempted boundary. Local pre-network rejection after a native attempt still requires complete worker/native integration; no generic error or missing order substitutes for zero execution.

All non-rejected package holdings, including completed packages, are protected from ordinary quote cancellation, age-based liquidation and leg-by-leg exits. This preserves the proven portfolio floor. A profitable whole-package unwind and settlement accounting remain separate implementation requirements. Existing wallets and historical journal prefixes are retained.

## Reviewed facts and physical dispatch

An opaque admission is constructed by rerunning the scanner, never by deserializing a proposed trade. It binds full certificate/source evidence, exact bank and caps, gross funding, wire legs, minimum payout, buffer and expiration. Expiration is clipped by the current relationship/rule evidence, reviewed certificate, book lifetime and authorized daily window. An admission expiring before native activation is rejected.

Version1 certificates preserve their exact receipt/hash behavior. Explicit version2 certificates pin original review provenance and full rule-body hashes while validating fresh unchanged evidence. Relationship semantics omit only changing node current-price metadata; status, version, contract identity, direction and every other semantic field remain bound. Newer changed rule evidence prevents reuse even if an older matching receipt is still fresh. No certificate can approve itself: its digest must already be in the trusted set.

Physical authorization checks the native attempted package against that admission, exact SDK request bytes, stable key, ordered components, full funding, fresh scoped account/cash/positions/open orders, reviewed contract bodies and full fixed-limit depth. Gross cash must reconcile to the attributed ledger; unused advance is not substituted for cash. Components initially must be flat. The enclosing production authority retains STOP/control/binary, stream, daily-window and ownership guards. Every prior ordinary component acknowledgment must have authoritative terminal/fill closure. Package orders cannot pass through ordinary-order preparation.

The separate package dispatch journal retains the entire frozen request and proof. It uses an exclusive writer lock, chained durable rows, bounded parsing, path checks and poisoned-write handling. Attempted or unknown physical outcomes block replacement across restart. An acknowledgment resolves physical acceptance, not the native financial lifecycle. A local not-sent outcome requires an in-process, single-use attempt token, the matching SDK error and explicit original queue completion evidence; an empty queue alone is insufficient. No manual-complete method exists.

The sole worker has not yet connected admission, physical dispatch, all outcome branches and startup recovery into one production path. These helpers do not authorize a new account client or an independent service.

## Validation at this checkpoint

- Native package lifecycle:141 checks passed, including20 new cases. The original16,535-event frozen prefix replayed with exact financial report, order, fill and source-cursor parity and zero account requests. A fresh stopped production prefix is still required before a financial upgrade.
- The merged runtime passes all 267 Linux checks and its locked ARM64 release build. This includes the package/certificate/feedback/journal/authorization/collateral helpers, real Session fixtures, two enclosing production-authority boundary tests and 12 connection-retry checks. The candidate financial-package engine remains undeployed.
- Fill economics:16 tests passed, then the private86-fill production snapshot passed exact FIFO parity. Aggregate evidence is [available separately](../research/moloch-fill-economics-20261015/actual-aggregate.json).
- Owned-fill markout analyzer: 19 synthetic checks passed, followed by a real-source diagnostic on the original sealed 85,454-event prefix. Its byte-chain and report-seal checks passed. New BUY post-fill coverage is sparse and supplies no positive conditional markout edge; see the [aggregate and limits](../research/moloch-owned-fill-markouts-20261015/README.md#actual-source-diagnostic). This is not native financial replay or fresh account reconciliation.

The actual snapshot has25 fully closed new BUY lots:3 gross wins,18 losses and4 flat. All9,428 new shares closed through maximum-age SELLs, with zero opposite-BUY pair redemptions. New-trading realized is **−115.405000**, separate from **+38.075000 inherited gross**. Remaining unmarked cost is22,772.515000; total liquidation P&L is unavailable. The confirmed inventory-age rule is60,000ms and the shortest observed new holding was78,061ms: this sample does **not** demonstrate premature age exits. Earlier ten-minute maturity labeling was a diagnostic error; ten minutes is the order lifetime, not the inventory-age rule.

## Full Paper 2 scope and evidence needed

| Requirement | Current evidence | Remaining implementation/verification |
|---|---|---|
| Authoritative connected contract graph | Official scoped census and graph adapters; observed tournament registry empty | Refresh through the sole worker, verify any newly available components and retain current provenance |
| Terminal payoff compiler and structural scanner | Exact two/three-leg ordinary and exceptional-state tests; conservative executable cost | Current real qualifying packages, broader supported logic/templates and continuing rule validation |
| Collateral characterization and accounting | Official parsers and diagnostic cash/debt consistency planner | Integrate actual advances, repayments and settlement into the native ledger; reconcile independent source evidence before sizing with buying power |
| Atomic package execution | Funded native lifecycle, SDK, admission, authorization, feedback and physical journal checked | Complete worker/startup/error wiring, fresh-prefix equivalence, controlled financial rollout and actual package fills |
| Portfolio marginal value | Offline scanner accounts for existing portfolio floor | Live admission currently flat-component only; implement marginal acquisition and whole-package unwind |
| Queue/flow and conditional markout models | Existing research plus new owned-fill diagnostic | Verified current coverage, causal training/holdout and integration specifically for package acquisition/hedging |
| Opportunity and rank-aware allocation | Existing finite risk caps | Implement ranking by executable package economics, remaining time and leaderboard gap without treating leverage as profit |
| External election residual forecasts | Separate historical research exists | Validated rule-equivalent/current inputs, out-of-sample calibration and residual sizing after structural allocation |
| Production and economic validation | Existing sole Rust worker; actual losses separated from inherited gains | Successful package deployment, measured filled-package economics, exact collateral/settlement reconciliation and continued improvements |

The available empty relationship registry and independent refund exceptions are substantive evidence constraints. Neither title matching nor the historical0.955 package observation supplies a current guaranteed trade. Missing features and unavailable observations remain explicit; passing checks do not establish profitability or completion.

## Cancellation read reduction

A checked follow-up uses a disjoint cancellation fact type containing only the four actual account observations needed by DELETE authorization. Ordinary BUY/SELL preparation keeps all seven receipts. The worker removes the three unused market, node and book operations before a cancel; it does not fabricate placeholder receipts or reuse stale reconciliation facts. SDK pagination and bounded read recovery may still require additional physical HTTP requests. Native order ownership, exact request/permit, fresh scoped account data, control, binary and STOP remain enforced. All 275 Linux runtime checks, including eight new cancellation checks, and the locked ARM64 release pass. [Check record](../research/moloch-package-execution-20261015/cancellation-checks.json). This follow-up is not deployed; current V25 stays the verified production release.

Deployment update: cancellation reduction is included in runtime-only V26, launched20:24:28 UTC after393/393 acknowledged attempts and a quiet queue. Its financial identity and existing bridge are unchanged. Fresh trading verification follows restoration; the older V25 verification above remains dated history. [Discovery/release evidence](../research/moloch-discovery-recovery-20261015/README.md).

The merged no-send checkpoint also passes a locked ARM64 release; [aggregate check evidence](../research/moloch-package-execution-20261015/local-nosend-checks.json) pins counts and logs. At20:34:55 UTC V26 has fresh healthy trading progress and395ACK/86fills (two newACK, no newfills); full cycle pending. No financial-package deployment is inferred.


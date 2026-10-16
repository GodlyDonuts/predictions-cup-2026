# Reviewed forecast positions in the existing Moloch worker

The Paper 2 forecast path values an ordinary settlement payout, rather than assuming every position should exit after sixty seconds. It uses the existing account worker, queue, native reservations, official receipts and attributed financial journal. The policy sidecar supplies reviewed authorization; actual official fills determine accounting.

## October16 extension: independent entries and inherited positions

The initial deployed policy only gated existing ordinary quotes. A required REST read did not create a forecast opportunity: the ordinary planner still selected pairs using spread, flow and scarce fresh-market slots. The new candidate adds an independent read-only entry nomination, original seven-receipt preflight and opaque native reservation plan. Missing or stale native quotes can trigger fresh reads without granting order authority. Native `reviewed_fundamental_entry_reserved_v1` creates one genuine funded BUY at the current same-side best bid, clipped by reviewed principal/quantity, actual available cash and reservations, total/per-market commitments, fresh-market slots, quantity, depth and write pacing. It cannot cross the ask, duplicate a target liability or manufacture a fill. A resting reviewed BUY remains pinned against unrelated ordinary replanning until actual terminal feedback or the existing retirement paths.

Inherited positions use a distinct verified origin, with no fabricated entry qid. The original committed adoption event, official receipt hash, original position row, exact remaining FIFO lots, financial head/fill prefix, original dispatch log and actual quiet account queue must agree. An immutable review explicitly pins that origin. Known historical fills and costs retain their inherited attribution.

After fresh facts and positive retirement of blocking never-sent drafts, native `reviewed_inventory_exit_reserved_v1` can reserve a genuinely covered SELL at current executable depth. A current reviewed value, explicit current revocation or the original fixed deadline supplies its reason. This allows an economically justified sale below historical cost; the eventual official fill records the real loss. Basis is accounting evidence, not an obligation to wait for break-even. A stale forecast alone cannot authorize an adverse-value sale before the fixed deadline. Native source/head/book/position/depth/quantity/budget checks and existing physical authority still apply.

Both new native events change the financial engine identity. Deployment requires the explicit stopped-process identity bridge and exact sealed-prefix report/head comparison; code compilation alone is insufficient. The extension is deployed in source124bbd9e / immutable runtime548b5f3d8606, verified02:40UTC October16 with fresh trading and three new ACKs but no new fills. Current production evidence is in CURRENT_STATE; Paper2 remains incomplete.

One lifecycle restriction remains: a forecast entry durably bound before preliminary physical authorization consumes its single cohort even if it later expires without a network attempt. Only that same exact still-eligible unattempted quote can resume. Safe abandonment/replacement needs a separate event proving original not-sent status, zero fills/holdings and no physical liability; absence from a snapshot or timeout cannot provide that proof. This may miss trades and is not a completed re-entry strategy.

## Entry and valuation

An immutable review names the exact tournament, market, exchange, contract, side and original contract-root hash. It records original forecast, official candidate/ballot and official contract-node source bytes, their hashes and availability clocks. The execution owner explicitly supplies an ordinary payout probability, exceptional-event allowance, minimum exceptional payout, margin, principal cap, quantity cap and fixed holding deadline. These are assumptions, not a statistical lower bound or guaranteed arbitrage certificate.

The maximum entry price is floored in exact decimal units:

`((1 - exceptionalAllowance) * ordinaryProbability + exceptionalAllowance * minimumExceptionalPayout) - margin`.

A new entry requires a flat exchange, no unresolved account submission, a current approved review and source window, a valid native quote and a passive price below both the observed ask and reviewed ceiling. Existing unrelated holdings are not relabeled. Exposure in companion contracts remains correlated; no cross-market offset or settlement guarantee is invented.

The physical entry quantity is explicitly fixed before submission to the minimum of native quantity, reviewed quantity cap and floored principal cap divided by limit price. The original native quote and its conservative full reservation remain unchanged until the official ACK admits the smaller quantity through the existing financial engine. The sidecar seals that exact physical request. Both direct POST and recovered GET acknowledgments must match its quantity; an arbitrary smaller or larger admission is rejected for reconciliation.

## Holding and exits

Only actual attributed fills create the forecast holding. The normal market maker's opposite BUY and sixty-second liquidation are suppressed on that managed exchange. A value exit requires sufficient fresh bid depth and a physical SELL limit at least equal to reviewed expected payout before the entry margin. An improved displayed bid cannot authorize an old, lower SELL limit.

The fixed deadline or an explicit current revocation permits a covered risk exit. Stale forecasts block additional entries and cancel known outstanding entry orders; they do not become fresh sell forecasts. The original terminal GET and fill tape must reconcile an entry before a subsequent exit. The sidecar cannot renew its original holding horizon or repeatedly reopen a completed cohort.

## Physical dispatch and restart

The order sequence is review assessment, durable exact binding, opaque policy capability, original production authority, tagged original intent, original anchored attempt, sidecar recheck and the existing guarded SDK send. The capability binds the exact native head, quote, report, request, permit, immutable review authority, original sidecar bytes and original dispatch inode. The current official contract-node receipt must match the reviewed complete root semantics at the physical boundary.

Only the exact same bound entry may continue before any original intent or attempt, with unchanged request and fresh evidence. This handles a stale preflight or process exit before submission without granting a replacement or retry. Unknown acceptance follows the original fail-closed reconciliation path. Both managed BUY and SELL require the capability; legacy maker routing and package execution cannot consume managed inventory. Cancellation authority remains available.

The initialized marker is consumed once. Lost, truncated, edited or replaced sidecars cannot silently initialize again. Existing tagged original intents detect lost binding history. Restart reconstructs from original records; process capabilities do not survive restart. Forecast attempts are excluded from the short-horizon maker learning denominator while their real resting liquidity remains in depth adjustments.

## Initial Delaware review candidate

The prospective first target is Democratic Delaware Senate YES, market353/exchange1042/contract354. The companion Republican exchange1075 has separately attributed inherited inventory. Original structured official metadata names the Democratic Party, General election2026-11-03, race62957/stage98087. Full resolver language is unavailable, so exceptional treatment remains an explicit assumption.

The October15 [Call the Map forecast](https://www.callthemap.com/senate/delaware) reports Coons99.5%, while [Who Shows Up](https://whoshowsup.net/race/senate-de) reports above99%. The [official candidate list](https://elections.delaware.gov/candidates/candidatelist/genl_fcddt_2026.html) identifies Chris Coons as a qualified Democratic U.S. Senate candidate. These forecasts are not independent calibration; no nominee-matched polling average is claimed. Original usable HTML is retained privately. Rejected sample-ballot response bodies are excluded from evidence.

The execution-owner candidate assumes probability.95, exceptional allowance.01, zero exceptional payout and margin.01, giving ceiling.9305 and expected payout.9405 before margin. Its initial new principal cap is400, separately from the existing overall40,000 and10,000-per-market limits. At.89 this would permit449 shares costing399.61; actual quantity and price require fresh native and physical evidence. That arithmetic is conditional expected value, not realized profit. Original source clocks mean first-observed availability where exact intraday publisher timestamps are unavailable; no earlier historical availability is claimed.

The review is active. The earlier worker acknowledged437YES at.915 as order41655759/q3085 at02:13:59UTC; its complete cancellation at02:22:05UTC proves zero fills. The consumed cohort still blocks a second entry pending genuine closure archival. The new release also manages inherited Massachusetts7061YES under a separate pinned original-lot review, with no extra-buy authority. Fresh acknowledgments, actual fills and realized results remain separately recorded in CURRENT_STATE. Paper2 remains incomplete.

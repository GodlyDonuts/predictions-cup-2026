# Ordinary-entry quarantine and native reservation release

Task [165](https://github.com/GodlyDonuts/predictions-cup-2026/issues/165) changes entry selection and local reservation management. The financial engine, SDK, account queue, source/model clocks, quantities, entry windows and risk limits are unchanged.

## Behavior

The old ordinary path could submit a new-exposure BUY when no qualified maker decision existed. Its flat paired drafts also reserved native cash before reviewed entry comparisons. At the authenticated 09:36:13 UTC snapshot, four such drafts reserved 3,593.940 of 3,597.585 cash. This was a dated planning constraint; the later 10:03 observation reserved only 78.750. Reservation pressure fluctuates as native events regenerate or retire drafts.

The new worker filters ordinary new-exposure BUYs before expensive preflight. At common submission, after fresh physical facts, it requires an actual current fundamental decision, an actual validated maker decision, or a whole-quantity inventory-reducing opposite BUY. Config presence, a managed-exchange name and a reviewed marker alone are insufficient. Reviewed top-ups and their existing limits continue through the genuine capability path.

Covered opposite BUYs are checked against the exact current native quote and free opposite holdings after competing SELL/netting BUY claims and unresolved original attempts. Partial coverage is insufficient. Managed holds, reviewed origins and protected package inventory retain their guards. This exemption does not establish profitable pricing or waive funding, book, account or final authority checks.

When no maker policy is configured, a separate opaque selector can retire flat ordinary paired drafts without an age or stale-book requirement. Original no-intent/attempt/ACK/native-binding/fill proof is mandatory. Both sides must have zero holdings. An unclosed intended or actual mate protects its entire exchange. Managed exchanges, package scopes and reviewed drafts are excluded; unresolved attempts fail closed. The prior stale-draft selector keeps its existing 60-second and unavailable-book conditions.

Selection binds the current native head, original dispatch descriptor, scope, managed set and real clock. The sole worker validates the whole selection, then records each original `not_sent` proof and existing `unsubmitted_intent_retired` event individually. This releases only native reservations. It creates no cash, fill, cancellation, price change or execution permission. Financial history remains append-only; partial completion is individually durable.

Cleanup repeats after genuine book admission, before reviewed comparison, because serviced source reads can recreate drafts. The callback refreshes the real current clock after retirement; original receipt/model clocks are not extended. A second cleanup follows the first physical fact packet before native plan preparation. The final independent physical packet remains required.

## Qualification and limits

Regressions cover young/fresh selection versus old stale behavior, current head/log/scope/clock binding, intended and actual mates, exact opposite coverage, unchanged financial fields, restart replay and source-regenerated reservations. Independent source review found and corrected a prefilter clock captured before durable cleanup. It found no further blocking source defect. It did not execute the production preflight-to-final-guard coverage-loss race; that remains a test limitation.

Actual ARM64 qualification and production results are recorded separately when verified. Source presence does not establish deployment, released live cash or additional profit. The historical ordinary cohort lost 184.970; quarantining that unqualified class is a strategy judgment, not evidence that reviewed forecasts are calibrated or that subsequent trades will win.

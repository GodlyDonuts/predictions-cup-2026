# Paid data collection and model-selected wake timing — October 20, 22:40 UTC

The optional managed data profile is implemented and checked on real recorded development cases. It supplies a working market-data service while preserving model control, paid request timing and the shared account budget. It is not yet installed in GPU027, and the measured untrained-policy outcomes do not establish profitability.

## Implemented behavior

`managed_data_service: {"profile":"model_pausable_book_reference_service_v1","initially_enabled":true,"finance_period_ms":30000}` enables a local simulated collector. It requests known SIG books and known live Kalshi/Polymarket references through the existing `request_observation` path. It does not fabricate external prices when those sources are absent, reconstruct historical depth from candles, or open a network client. Optional account/positions/open-order reads use their existing endpoint-scoped, delayed receipts. The financial-read period is an explicit experiment setting, not an exchange limit.

Automatic and model-requested operations consume the same SIG budget. External requests use the same existing venue limits. A foreground SIG request that had to wait gets the next actor turn before background polling competes for that slot. Choosing a different action abandons that scheduling preference; it does not create or dispatch a durable trading intent. Unknown acceptance remains authoritative. Background data collection neither submits orders nor changes cash or positions.

The model has a new local control token that pauses or resumes the service. Pausing fences future polls without deleting already paid in-flight receipts, resetting a budget or rewinding a cursor. Existing manual API operations remain available.

Under this explicit profile, WAIT's patience bit chooses between the existing interruptible wake behavior and sleeping until the requested deadline. All original clock events, fills, receipts and daily reward boundaries still execute during uninterrupted waiting. The actor can therefore choose its inference frequency while data collection continues. Orders retain their existing patience/expiration semantics. Legacy profiles retain their original WAIT behavior.

The profile is pinned in checkpoints and on-policy behavior contracts, checked against the environment, and requires explicit migration for inherited weights. New trajectories, optimizer state and scores are required. Acting, batched training, evaluation and sealed episode replay share the same alarm implementation. World cleanup closes the local service. The checks cover paid delivery, all three shared budgets, foreground priority, cancellation of an abandoned scheduling preference, pause/in-flight races, unknown acceptance, scoped finance reads, WAIT probability density, checkpoint/profile mismatch, exact sealed replay, and complete daily reward boundaries during one long wait. Thirty-seven CPU checks passed; one CUDA check was skipped while GPU027 remained active. These checks do not establish economic performance.

## Paired recorded-data measurement

Both profiles used the same original GPU027 founder weights, endpoint-balanced sampling, paired seeds, two exposed ten-minute training windows and the same recorded 10 ms compute assumption. No optimizer ran, and all checkpoint parameters remained unchanged. The managed profile adds both the collector/control token and the new WAIT choice, so their individual effects are not isolated.

| Case | Manual-data marked ROI | Managed-data marked ROI | Manual / managed fills |
| --- | ---: | ---: | ---: |
| Ordinary | -0.084265% | -0.5162125% | 5 / 7 |
| Stress | -0.3398375% | 0% | 9 / 0 |

The ordinary managed case paused its data service after 23 book reads. It had two contract-targeted actions with a valid cached book before the action, but all seven eventual fills had an invalid cached book at fill time. Its result was worse. The stress managed case kept collection active and made no executed trade. It admitted 938 book requests plus 60 scoped finance requests, 12,000 Kalshi reference requests and 17,648 Polymarket reference requests over ten virtual minutes. Admissions do not imply that every final in-flight receipt arrived before the horizon. It made 34 policy decisions rather than 431 in the manual baseline. Zero ROI came from not trading; it is not alpha.

This establishes working paid collection and controllable inference timing, not a useful trained strategy. The manual baseline exactly reproduced the prior paired study's two marked ROI values. [Full comparison](managed-data-paired-result-20261020.json), [source/job identity](managed-data-paired-job-20261020.json).

## Original prospective SIG source recovered for the next adapter

One completed October 20 capture segment was copied locally as original bytes and verified against its archive and member hashes: 15,088 source envelopes plus 638 REST receipt records. The original Node `payloadHash` verified every producer sequence/hash link. Canonical JSON transport retains those original producer hashes; the existing strict Python byte-hash verifier then passed every one of the 15,088 canonical rows. Original files remain unchanged. This is transport verification, not market semantic admission.

The source contains 237 subscription records and 14,213 broadcasts. Its public stream receipt clock is monotonic. Twenty-six older `rest_reference` metadata receipts were appended after later stream events; they must not be mistaken for backwards WebSocket delivery clocks or retimed quotes. The next adapter must join the actual REST receipt payloads and original broadcast payloads with their own receipt clocks, preserving subscription/resync gaps and contract identity. It must not weaken the original chain checks to fit the old breadth envelope layout.

The first subscription was recorded 241 ms after the first REST-reference metadata record, and the first broadcast 1.218 seconds after it. The collector subscribes while its REST scan continues. No capture-controller change was needed on the theory that subscription waited for the full scan. This observation concerns one completed segment, not proof of global continuity. [Original export hashes](prospective-sig-fixture-export-20261020.json), [full-chain audit](prospective-sig-fixture-audit-20261020.json), [reproducible audit script](audit-forward-fixture-20261020.mjs).

From a repository checkout, the one-off audit command is `node research/tartarus-learning-v2/audit-forward-fixture-20261020.mjs ORIGINAL_EVENTS EXPORT_MANIFEST NEW_CANONICAL_OUTPUT execution/ledger.mjs`. It has an explicit October 21 cutoff and creates its output exclusively; it is not a general future-test admission command or the unfinished market decoder.

## Active learning and remaining work

GPU027 remains the sole GPU learner on its original frozen source. Its first full evolutionary cycle completed. The first scored member in cycle one improved worst-case marked ROI from cycle zero's best -18.8378% to -12.2765775%; both cases are still negative and repeat already exposed development calendars. At the latest progress check it had exceeded 51,000 decisions and committed 677 updates. These results do not establish generalization or the complete-day reward qualification. [Scored checkpoints](population-027-first-evolution-20261020.json).

The completed longer SIG cache and running external receipt compilation still need integration. Then a fresh pre-October-17 lineage must train with useful observations and be evaluated chronologically. The requested full-history coverage, full daily-growth training/evolution, four-day independent test and independently profitable model remain incomplete. The October 21–25 independent calendar is reserved and unopened. This work changed no production financial state or execution authority.

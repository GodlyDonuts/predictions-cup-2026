# Chronological three-venue learning — October 20

## What changed

Checked source `f4df828db9fe70240e887905450400da60bea4f7` combines:

- `2594a790`: causal, pinned joins of the two Kalshi and Polymarket history partitions, and optional complete-day development selection using the human's growth formula. These scores remain explicitly ineligible for independent/source qualification.
- `d263a3f8`: streaming merge of the original multi-day SIG observations and archived external REST references. Original normalized rows and trade bytes are retained; gaps are not filled.
- `f4df828d`: an optional price vocabulary relative to the exact book observation supplied to the actor. Market orders and every legal tick remain available for both outcomes. Founder price preferences are trainable biases, not restrictions.

The optional managed data service uses the existing simulated queues, rate budgets, delays, and receipts. The actor can pause it. Endpoint-balanced read sampling changes exploration probabilities without removing query targets. Both profiles are pinned in checkpoints and behavior contracts.

Focused verification included 26 economics/history checks, two real compiler-to-cache merge fixtures, and 32 price/full-depth/managed-service/sealed-episode checks. Synthetic checks establish mechanics, not returns.

## Recovered data

The merged replay cache is complete: **20,636,573,696 bytes**, manifest SHA-256 `f066e68bbf40fa5c70386054e517eabe29ad9b2b3ad24aaae258fc8f97387957`.
Dataset manifest SHA-256: `533255d8f1718f24cfaf72cadcbd0e116febfed92cf073aec85dcfdf6b25bd3c`.

It includes 5,062,890 SIG book observations, 71,506 SIG trade observations, 3,577,294 Kalshi references and 5,947,570 Polymarket references, plus explicit gaps. SIG originals span October 15–19; archived external references extend into October 20. There are **no original external reference snapshots in the pre-October-17 fit partition**. Causal, paid historical information queries are available there, but do not manufacture missing live quotes or executable liquidity.

This is partial research data. Full competition depth, historical catalog availability, reference equivalence, continuous source coverage and execution calibration are not established. Neither cache completion nor this document admits it to independent profitability qualification.

## Fresh chronological population

The new lineage starts with four fresh models, importing no weights from population 027, which already used October 19 training data.

| Purpose | Window / setting |
|---|---|
| Fit | Available original SIG history on October 15 through October 17 00:00 UTC |
| Training episodes | Random one-hour windows; eight parallel worlds |
| Development selection | October 17 00:00 through October 19 00:00 UTC, two complete consecutive days |
| Selection reward | Human daily log-growth/consistency/evidence formula; nominal and stressed mechanics |
| Short training reward | Disclosed development log-wealth proxy; not the complete-day qualification |
| Model | Recurrent policy, hidden size 1,024; approximately 5.27 million parameters |
| Population | Four members, two islands, 16 configured cycles; retain/select the better candidates even if all lose |
| Learning | Sealed on-policy episodes and full-episode return replay; no fabricated fitness for unfinished paths |
| Independent reservation | October 21–25 UTC, unopened and excluded from fitting/selection |

Initial preferences favor reads and quoting near an observed book. They change trainable logits only. API-supported price, quantity, timing, memory and action domains remain available. There is no new artificial decision cap.

The local preparation controller verifies source/data pins and rehearses all four founders on CPU. Only after that proof does an exact retained Windows process handle stop the old simulated trainer and hand the single GPU to the new supervisor. A completed old generation, checkpoints and unfinished traces remain in place. The new supervisor does not automatically restart failed training.

## Economic and operational limits

Population 027's first evolved candidate improved its repeated-development worst marked return from approximately -18.84% to -12.28%; both remain losses. No independently profitable learned policy is established.

At 23:11 UTC the original prospective SIG collector was capturing segment 16, with 16 completed segments. The production financial STOP retained SHA-256 `1b1b24f1379d9e3a11c13991afd588d085dc16d99c392eda2a3920769c0b2043`. This work changes neither contest financial state nor trading authority. The prospective source still needs semantic replay admission; transport hashes alone are insufficient.

## Rehearsal and initialization correction

Population 028's four CPU founders completed 116 actual API-policy decisions in total and reloaded their pinned checkpoints successfully. Its supervised process started at 23:14:28 UTC. Before any progress record was written, an initialization audit caught a configuration mistake: family 3 is CANCEL; the intended READ preference belongs to family 10. All four corrected bias vectors were checked on CPU. Source, observations, replay, scoring and legal action domains were unchanged.

The exact population 028 process was stopped and its files retained. Population **029** starts fresh with the corrected priors; it imports neither 028 nor 027 weights. Configuration SHA-256: `aece0a99e5d8a74b501fdd5ce0e22f912b920eaa0d196e9620691d6bdb2b26af`. The earlier CPU mechanics proof remains applicable; it is not relabeled as a rollout of the corrected actor.

At **23:20:44 UTC**, sole GPU process **10104** had collected **1,142 decisions** in the first eight-world cohort. Its measured local p95 inference cost was **11 ms**. No cohort had closed and **zero optimizer updates** had committed yet; this is collection evidence, not a usable trained checkpoint or ROI result. See `population-029-gpu-collection-20261020.json`. The full ML objective remains incomplete.

## Measured replay startup bottleneck

A live Python stack identified the role metadata check in `Store.world_view` as a startup delay. SQLite chose the contract-first index, causing scattered full-row reads. Checked source `85ef1450` forces the existing chronological index for the identical missing-metadata predicate. On the complete fit window, the forced query returned the same expected zero count in **2.75 seconds**. No matched before/after timing ratio is claimed. Thirteen disk replay checks passed, including rejection of a missing original clock inside its exact role and exclusion of the same row outside that role.

This fix is **not hot-patched into frozen population 029**. That run continues with its original source and trajectories. A 15-second profile of collection also found substantial JSON decoding and disk-book reconstruction overhead; optimizing it requires separate parity evidence. It is not evidence that extra GPU utilization would improve the model's return.
